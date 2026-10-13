use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, Semaphore};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::{
    events::*,
    options::{CopyOptions, OverwritePolicy, VerifyPolicy},
    plan::{CopyPlan, CopyTask, TaskKind},
};

pub struct CopyEngine {
    options: CopyOptions,
    events_tx: mpsc::Sender<CopyEvent>,
    cancel: CancellationToken,
}

impl CopyEngine {
    pub fn new(options: CopyOptions, events_tx: mpsc::Sender<CopyEvent>) -> Self {
        Self { options, events_tx, cancel: CancellationToken::new() }
    }

    pub fn cancel_token(&self) -> CancellationToken { self.cancel.clone() }

    pub async fn run(&self, plan: CopyPlan) -> Report {
        let start = Instant::now();
        let mut report = Report::default();
        // Fix 2: cuenta solo files
        let total_files = plan.tasks.iter().filter(|t| t.kind == TaskKind::File).count();
        let total_tasks = plan.tasks.len();
        let total_bytes = plan.total_bytes;

        let _ = self.events_tx.send(CopyEvent::Planned { total_files, total_bytes }).await;

        let sem = Arc::new(Semaphore::new(self.options.parallel_files.max(1)));
        // Fix 2: canal por tareas totales
        let (res_tx, mut res_rx) = mpsc::channel::<FileResult>(total_tasks.max(1));
        let mut handles = Vec::with_capacity(total_tasks);

        for task in plan.tasks {
            // Fix 5: no spawnea si ya cancelado
            if self.cancel.is_cancelled() {
                break;
            }

            let permit = match sem.clone().acquire_owned().await {
                Ok(p) => p,
                Err(_) => break,
            };

            let opts = self.options.clone();
            let tx = self.events_tx.clone();
            let rtx = res_tx.clone();
            let cancel = self.cancel.clone();

            handles.push(tokio::spawn(async move {
                let _permit = permit;
                let result = copy_task(&task, &opts, &tx, &cancel).await;
                let _ = rtx.send(result).await;
            }));
        }
        drop(res_tx);

        while let Some(result) = res_rx.recv().await {
            match result {
                FileResult::Copied { bytes } => {
                    report.files_copied += 1;
                    report.bytes_copied += bytes;
                }
                // Fix 2: dirs aparte
                FileResult::DirCreated => {
                    report.dirs_created += 1;
                }
                FileResult::Skipped { path, reason } => {
                    // Fix 3: skip centralizado
                    let _ = self.events_tx.send(CopyEvent::FileSkipped { path: path.clone(), reason }).await;
                    report.files_skipped += 1;
                }
                FileResult::Failed { path, error } => {
                    // Fix 3: fallo centralizado
                    let _ = self.events_tx.send(CopyEvent::FileFailed { path: path.clone(), error: error.clone() }).await;
                    report.files_failed += 1;
                    report.errors.push((path, error));
                    if !self.options.continue_on_error {
                        warn!("cancelando por error (continue_on_error=false)");
                        self.cancel.cancel();
                    }
                }
            }
        }

        for h in handles { let _ = h.await; }

        report.duration = start.elapsed();
        let _ = self.events_tx.send(CopyEvent::Finished { report: report.clone() }).await;
        report
    }
}

enum FileResult {
    Copied { bytes: u64 }, // Fix 8: solo bytes
    DirCreated, // Fix 2: separa dirs
    Skipped { path: PathBuf, reason: SkipReason },
    Failed { path: PathBuf, error: String },
}

async fn copy_task(
    task: &CopyTask,
    opts: &CopyOptions,
    events: &mpsc::Sender<CopyEvent>,
    cancel: &CancellationToken,
) -> FileResult {
    if cancel.is_cancelled() {
        return FileResult::Failed { path: task.src.clone(), error: "cancelled".into() };
    }

    match task.kind {
        TaskKind::Dir => {
            if let Err(e) = fs::create_dir_all(&task.dst).await {
                return FileResult::Failed { path: task.dst.clone(), error: e.to_string() };
            }
            // Fix 2: no cuenta como archivo
            FileResult::DirCreated
        }
        TaskKind::Symlink => {
            // TODO: leer con fs::read_link y crear con std::os::unix::fs::symlink / windows
            debug!("symlink omitido por ahora: {:?}", task.src);
            FileResult::Skipped { path: task.src.clone(), reason: SkipReason::Excluded }
        }
        TaskKind::File => copy_file(task, opts, events, cancel).await,
    }
}

async fn copy_file(
    task: &CopyTask,
    opts: &CopyOptions,
    events: &mpsc::Sender<CopyEvent>,
    cancel: &CancellationToken,
) -> FileResult {
    let start = Instant::now();

    // --- política de sobrescritura ---
    if let Ok(dst_meta) = fs::metadata(&task.dst).await {
        let skip = match opts.overwrite {
            OverwritePolicy::Never => true,
            OverwritePolicy::IfNewer => match fs::metadata(&task.src).await {
                Ok(src_meta) => matches!(
                    (src_meta.modified(), dst_meta.modified()),
                    (Ok(s), Ok(d)) if d >= s
                ),
                Err(_) => false,
            },
            OverwritePolicy::IfDifferent => {
                // Fix 6: compara size + mtime
                match fs::metadata(&task.src).await {
                    Ok(src_meta) => {
                        let same_size = dst_meta.len() == task.size;
                        let same_mtime = matches!(
                            (src_meta.modified(), dst_meta.modified()),
                            (Ok(s), Ok(d)) if s == d
                        );
                        same_size && same_mtime
                    }
                    Err(_) => false,
                }
            },
            OverwritePolicy::Always => false,
        };

        if skip {
            // Fix 3: lo avisa run()
            return FileResult::Skipped { path: task.dst.clone(), reason: SkipReason::AlreadyExists };
        }
    }

    let _ = events.send(CopyEvent::FileStarted {
        path: task.src.clone(),
        size: task.size,
    }).await;

    // --- asegurar directorio padre ---
    if let Some(parent) = task.dst.parent() {
        if let Err(e) = fs::create_dir_all(parent).await {
            return FileResult::Failed { path: task.dst.clone(), error: e.to_string() };
        }
    }

    // --- abrir origen ---
    let mut src = match fs::File::open(&task.src).await {
        Ok(f) => f,
        Err(e) => return FileResult::Failed { path: task.src.clone(), error: e.to_string() },
    };

    // --- escribir a archivo temporal (atomicidad + resume-friendly) ---
    let tmp_path = tmp_path_for(&task.dst);
    let mut dst = match fs::File::create(&tmp_path).await {
        Ok(f) => f,
        // Fix 7: reporta dst, no tmp
        Err(e) => return FileResult::Failed { path: task.dst.clone(), error: e.to_string() },
    };

    let mut buf = vec![0u8; opts.buffer_size.max(64 * 1024)];
    let mut copied: u64 = 0;
    let mut last_emit = Instant::now();

    loop {
        if cancel.is_cancelled() {
            drop(dst);
            let _ = fs::remove_file(&tmp_path).await;
            return FileResult::Failed { path: task.src.clone(), error: "cancelled".into() };
        }

        let n = match src.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                drop(dst);
                let _ = fs::remove_file(&tmp_path).await;
                return FileResult::Failed { path: task.src.clone(), error: e.to_string() };
            }
        };

        if let Err(e) = dst.write_all(&buf[..n]).await {
            drop(dst);
            let _ = fs::remove_file(&tmp_path).await;
            return FileResult::Failed { path: task.dst.clone(), error: e.to_string() };
        }

        copied += n as u64;

        // emitir progreso como máximo cada 100ms para no saturar el bus
        if last_emit.elapsed().as_millis() >= 100 {
            let _ = events.send(CopyEvent::Progress {
                path: task.src.clone(),
                bytes_copied: copied,
                total: task.size,
            }).await;
            last_emit = Instant::now();
        }
    }

    if let Err(e) = dst.flush().await {
        drop(dst);
        let _ = fs::remove_file(&tmp_path).await;
        return FileResult::Failed { path: task.dst.clone(), error: e.to_string() };
    }
    drop(dst);

    // --- verificación ---
    match opts.verify {
        VerifyPolicy::None => {}
        VerifyPolicy::Size => {
            if let Ok(m) = fs::metadata(&tmp_path).await {
                if m.len() != task.size {
                    let _ = fs::remove_file(&tmp_path).await;
                    return FileResult::Failed {
                        path: task.dst.clone(),
                        error: format!("size mismatch: {} != {}", m.len(), task.size),
                    };
                }
            }
        }
        VerifyPolicy::Hash => {
            if let Err(e) = verify_hash(&task.src, &tmp_path).await {
                let _ = fs::remove_file(&tmp_path).await;
                return FileResult::Failed { path: task.dst.clone(), error: format!("verify: {e}") };
            }
        }
    }

    // --- permisos ---
    if opts.preserve_permissions {
        if let Ok(meta) = fs::metadata(&task.src).await {
            let _ = fs::set_permissions(&tmp_path, meta.permissions()).await;
        }
    }

    // --- rename atómico ---
    if let Err(e) = atomic_replace(&tmp_path, &task.dst).await {
        let _ = fs::remove_file(&tmp_path).await;
        return FileResult::Failed { path: task.dst.clone(), error: e.to_string() };
    }

    // Fix 1: progreso final
    let _ = events.send(CopyEvent::Progress {
        path: task.src.clone(),
        bytes_copied: copied,
        total: task.size,
    }).await;
    let _ = events.send(CopyEvent::FileCompleted {
        path: task.dst.clone(),
        elapsed: start.elapsed(),
    }).await;

    // Fix 8: sin path sin uso
    FileResult::Copied { bytes: copied }
}

fn tmp_path_for(dst: &Path) -> PathBuf {
    let mut s = dst.as_os_str().to_owned();
    s.push(".copyforge.tmp");
    PathBuf::from(s)
}

#[cfg(unix)]
async fn atomic_replace(tmp: &Path, dst: &Path) -> std::io::Result<()> {
    // En Unix, rename sobreescribe atómicamente
    fs::rename(tmp, dst).await
}

#[cfg(windows)]
async fn atomic_replace(tmp: &Path, dst: &Path) -> std::io::Result<()> {
    // En Windows, rename falla si el destino existe → intentar borrar primero.
    // (Para máxima robustez: ReplaceFileW vía crate `windows`)
    match fs::rename(tmp, dst).await {
        Ok(()) => Ok(()),
        Err(_) => {
            let _ = fs::remove_file(dst).await;
            fs::rename(tmp, dst).await
        }
    }
}

async fn verify_hash(src: &Path, dst: &Path) -> Result<(), String> {
    let s = hash_file(src).await.map_err(|e| e.to_string())?;
    let d = hash_file(dst).await.map_err(|e| e.to_string())?;
    if s != d { return Err("hash mismatch".into()); }
    Ok(())
}

async fn hash_file(path: &Path) -> std::io::Result<[u8; 32]> {
    use blake3::Hasher;
    let mut file = fs::File::open(path).await?;
    let mut hasher = Hasher::new();
    let mut buf = vec![0u8; 128 * 1024];
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 { break; }
        hasher.update(&buf[..n]);
    }
    Ok(*hasher.finalize().as_bytes())
}