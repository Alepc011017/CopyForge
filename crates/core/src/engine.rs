use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, Semaphore};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::{
    error::CopyError,
    events::*,
    options::{CopyOptions, DurabilityPolicy, OverwritePolicy, VerifyPolicy},
    plan::{CopyPlan, CopyTask, TaskKind},
    platform,
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

            let opts = self.options.clone();
            let tx = self.events_tx.clone();
            let rtx = res_tx.clone();
            let cancel = self.cancel.clone();
            let sem = sem.clone();

            handles.push(tokio::spawn(async move {
                // P0.3: el permiso se adquiere DENTRO de la tarea para no
                // serializar el arranque. Sin deadlock: el semáforo solo se
                // usa aquí, con N>=1 permisos, sin adquisición anidada.
                // Se libera justo tras copy_task, antes del send.
                let result = {
                    let Ok(_permit) = sem.acquire_owned().await else {
                        return;
                    };
                    copy_task(&task, &opts, &tx, &cancel).await
                };
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
                FileResult::Symlinked { src, dst } => {
                    let _ = self.events_tx.send(CopyEvent::Symlinked { src: src.clone(), dst: dst.clone() }).await;
                    report.symlinks_created += 1;
                    report.symlinked.push((src, dst));
                }
                FileResult::Skipped { path, reason } => {
                    // Fix 3: skip centralizado
                    let _ = self.events_tx.send(CopyEvent::FileSkipped { path: path.clone(), reason }).await;
                    report.files_skipped += 1;
                }
                FileResult::Failed { path, error } => {
                    // Fix 3: fallo centralizado. Frontera tipada→display:
                    // FileResult lleva CopyError; el evento/reporte públicos
                    // siguen siendo String para no romper la API.
                    let msg = error.to_string();
                    let _ = self.events_tx.send(CopyEvent::FileFailed { path: path.clone(), error: msg.clone() }).await;
                    report.files_failed += 1;
                    // P0.7: cap del Vec para no agotar memoria con millones
                    // de fallos; el resto solo incrementa el contador.
                    if report.errors.len() < self.options.max_errors {
                        report.errors.push((path, msg));
                    } else {
                        report.errors_truncated += 1;
                    }
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
    Symlinked { src: PathBuf, dst: PathBuf },
    Skipped { path: PathBuf, reason: SkipReason },
    Failed { path: PathBuf, error: CopyError },
}

async fn copy_task(
    task: &CopyTask,
    opts: &CopyOptions,
    events: &mpsc::Sender<CopyEvent>,
    cancel: &CancellationToken,
) -> FileResult {
    if cancel.is_cancelled() {
        return FileResult::Failed { path: task.src.clone(), error: CopyError::Cancelled };
    }

    match task.kind {
        TaskKind::Dir => {
            if let Err(e) = fs::create_dir_all(&task.dst).await {
                return FileResult::Failed { path: task.dst.clone(), error: CopyError::Io { path: task.dst.clone(), source: e } };
            }
            // Fix 2: no cuenta como archivo
            FileResult::DirCreated
        }
        TaskKind::Symlink => copy_symlink(task, opts, events, cancel).await,
        TaskKind::File => copy_file(task, opts, events, cancel).await,
    }
}

async fn copy_symlink(
    task: &CopyTask,
    opts: &CopyOptions,
    events: &mpsc::Sender<CopyEvent>,
    cancel: &CancellationToken,
) -> FileResult {
    if cancel.is_cancelled() {
        return FileResult::Failed { path: task.src.clone(), error: CopyError::Cancelled };
    }

    if opts.follow_symlinks {
        // Seguir el enlace: copiar el contenido resuelto.
        // El caso directorio recursivo ya viene expandido por el scanner
        // (WalkDir con follow_links=true); aquí basta con asegurar el dir
        // destino o delegar a copy_file si es un fichero.
        match fs::metadata(&task.src).await {
            Ok(m) if m.is_dir() => {
                if let Err(e) = fs::create_dir_all(&task.dst).await {
                    return FileResult::Failed { path: task.dst.clone(), error: CopyError::Io { path: task.dst.clone(), source: e } };
                }
                debug!("symlink dir seguido, dir asegurado: {:?} -> {:?}", task.src, task.dst);
                FileResult::DirCreated
            }
            Ok(m) if m.is_file() => {
                let file_task = CopyTask {
                    src: task.src.clone(),
                    dst: task.dst.clone(),
                    size: m.len(),
                    kind: TaskKind::File,
                };
                copy_file(&file_task, opts, events, cancel).await
            }
            Ok(_) => {
                debug!("symlink especial omitido: {:?}", task.src);
                FileResult::Skipped { path: task.src.clone(), reason: SkipReason::Excluded }
            }
            Err(e) => FileResult::Failed { path: task.src.clone(), error: CopyError::Io { path: task.src.clone(), source: e } },
        }
    } else {
        // Recrear el enlace sin seguirlo.
        let link_target = match fs::read_link(&task.src).await {
            Ok(p) => p,
            Err(e) => return FileResult::Failed { path: task.src.clone(), error: CopyError::Io { path: task.src.clone(), source: e } },
        };

        if let Some(parent) = task.dst.parent() {
            if let Err(e) = fs::create_dir_all(parent).await {
                return FileResult::Failed { path: task.dst.clone(), error: CopyError::Io { path: task.dst.clone(), source: e } };
            }
        }

        if fs::symlink_metadata(&task.dst).await.is_ok() {
            match opts.overwrite {
                OverwritePolicy::Never => {
                    return FileResult::Skipped { path: task.dst.clone(), reason: SkipReason::AlreadyExists };
                }
                _ => {
                    if let Err(e) = remove_existing_dst(&task.dst).await {
                        return FileResult::Failed { path: task.dst.clone(), error: CopyError::Io { path: task.dst.clone(), source: e } };
                    }
                }
            }
        }

        // P1.1a: creación delegada a platform:: (sin cfg aquí; en Unix
        // ambas ramas usan symlink(), en Windows eligen file/dir).
        let is_dir = fs::metadata(&task.src).await.map(|m| m.is_dir()).unwrap_or(false);
        // TODO(P1.6): symlink roto apuntando a directorio se recrea como file en Windows (CreateSymbolicLink no valida el target). Ver docs/PLAN.md P1.6.
        let res = if is_dir {
            platform::symlink_dir(&link_target, &task.dst)
        } else {
            platform::symlink_file(&link_target, &task.dst)
        };
        if let Err(e) = res {
            return FileResult::Failed { path: task.dst.clone(), error: CopyError::Io { path: task.dst.clone(), source: e } };
        }

        debug!("symlink recreado: {:?} -> {:?} (target {:?})", task.src, task.dst, link_target);
        FileResult::Symlinked { src: task.src.clone(), dst: task.dst.clone() }
    }
}

async fn remove_existing_dst(dst: &Path) -> std::io::Result<()> {
    let meta = fs::symlink_metadata(dst).await?;
    if meta.file_type().is_symlink() || meta.is_file() {
        fs::remove_file(dst).await
    } else if meta.is_dir() {
        // No borrar árboles por seguridad: solo vale si está vacío.
        fs::remove_dir(dst).await
    } else {
        fs::remove_file(dst).await
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
            return FileResult::Failed { path: task.dst.clone(), error: CopyError::Io { path: task.dst.clone(), source: e } };
        }
    }

    // --- abrir origen ---
    let mut src = match fs::File::open(&task.src).await {
        Ok(f) => f,
        Err(e) => return FileResult::Failed { path: task.src.clone(), error: CopyError::Io { path: task.src.clone(), source: e } },
    };

    // --- escribir a archivo temporal (atomicidad + resume-friendly) ---
    let tmp_path = tmp_path_for(&task.dst);
    let mut dst = match fs::File::create(&tmp_path).await {
        Ok(f) => f,
        // Fix 7: reporta dst, no tmp
        Err(e) => return FileResult::Failed { path: task.dst.clone(), error: CopyError::Io { path: task.dst.clone(), source: e } },
    };

    let mut buf = vec![0u8; opts.buffer_size.max(64 * 1024)];
    let mut copied: u64 = 0;
    let mut last_emit = Instant::now();

    loop {
        if cancel.is_cancelled() {
            drop(dst);
            let _ = fs::remove_file(&tmp_path).await;
            return FileResult::Failed { path: task.src.clone(), error: CopyError::Cancelled };
        }

        let n = match src.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                drop(dst);
                let _ = fs::remove_file(&tmp_path).await;
                return FileResult::Failed { path: task.src.clone(), error: CopyError::Io { path: task.src.clone(), source: e } };
            }
        };

        if let Err(e) = dst.write_all(&buf[..n]).await {
            drop(dst);
            let _ = fs::remove_file(&tmp_path).await;
            return FileResult::Failed { path: task.dst.clone(), error: CopyError::Io { path: task.dst.clone(), source: e } };
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
        return FileResult::Failed { path: task.dst.clone(), error: CopyError::Io { path: task.dst.clone(), source: e } };
    }

    // --- durabilidad pre-rename (P0.6a): fsync del temporal según política ---
    if let Err(e) = apply_durability(opts.durability, &dst, &task.dst).await {
        drop(dst);
        let _ = fs::remove_file(&tmp_path).await;
        return FileResult::Failed { path: task.dst.clone(), error: e };
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
                        error: CopyError::VerifyFailed(task.dst.clone()),
                    };
                }
            }
        }
        VerifyPolicy::Hash => {
            if let Err(e) = verify_hash(&task.src, &tmp_path, &task.dst).await {
                let _ = fs::remove_file(&tmp_path).await;
                return FileResult::Failed { path: task.dst.clone(), error: e };
            }
        }
    }

    // --- permisos ---
    if opts.preserve_permissions {
        if let Ok(meta) = fs::metadata(&task.src).await {
            let _ = fs::set_permissions(&tmp_path, meta.permissions()).await;
        }
    }

    // --- rename atómico (P1.1b: vía platform::atomic_replace) ---
    if let Err(e) = platform::atomic_replace(&tmp_path, &task.dst) {
        let _ = fs::remove_file(&tmp_path).await;
        return FileResult::Failed { path: task.dst.clone(), error: CopyError::Io { path: task.dst.clone(), source: e } };
    }

    // --- durabilidad post-rename (P1.1c): solo Full sincroniza el dir padre ---
    if opts.durability == DurabilityPolicy::Full {
        let parent = task.dst.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
        if let Err(source) = platform::sync_dir(&parent) {
            return FileResult::Failed { path: task.dst.clone(), error: CopyError::Io { path: parent, source } };
        }
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

/// Fase pre-rename de [`DurabilityPolicy`]: asegura los datos del temporal.
///
/// - `None` → nada (comportamiento anterior).
/// - `File` | `Full` → `sync_all()` del handle abierto, antes del rename.
///
/// Sin `unwrap`/`expect`.
async fn apply_durability(
    policy: DurabilityPolicy,
    file: &fs::File,
    dst: &Path,
) -> Result<(), CopyError> {
    match policy {
        DurabilityPolicy::None => Ok(()),
        DurabilityPolicy::File | DurabilityPolicy::Full => {
            file.sync_all().await.map_err(|e| CopyError::Io {
                path: dst.to_path_buf(),
                source: e,
            })
        }
    }
}

async fn verify_hash(src: &Path, tmp: &Path, dst_for_error: &Path) -> Result<(), CopyError> {
    let s = hash_file(src).await.map_err(|e| CopyError::Io { path: src.to_path_buf(), source: e })?;
    let d = hash_file(tmp).await.map_err(|e| CopyError::Io { path: tmp.to_path_buf(), source: e })?;
    if s != d { return Err(CopyError::VerifyFailed(dst_for_error.to_path_buf())); }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_std_error<T: std::error::Error + std::fmt::Display>() {}

    #[test]
    fn failed_cancelled_constructs() {
        let r = FileResult::Failed {
            path: PathBuf::from("/tmp/a"),
            error: CopyError::Cancelled,
        };
        match r {
            FileResult::Failed { error: CopyError::Cancelled, .. } => {}
            _ => panic!("expected Failed(Cancelled)"),
        }
    }

    #[test]
    fn failed_verify_failed_constructs() {
        let p = PathBuf::from("/tmp/b");
        let r = FileResult::Failed {
            path: p.clone(),
            error: CopyError::VerifyFailed(p.clone()),
        };
        match r {
            FileResult::Failed { error: CopyError::VerifyFailed(q), .. } => assert_eq!(q, p),
            _ => panic!("expected Failed(VerifyFailed)"),
        }
    }

    #[test]
    fn copy_error_is_std_error_and_display() {
        assert_std_error::<CopyError>();
        let cancelled = CopyError::Cancelled;
        assert_eq!(format!("{cancelled}"), "cancelado");
        let failed = CopyError::VerifyFailed(PathBuf::from("/x/y"));
        assert!(format!("{failed}").contains("/x/y"));
        let io_err = CopyError::Io {
            path: PathBuf::from("/z"),
            source: std::io::Error::other("boom"),
        };
        let as_dyn: &dyn std::error::Error = &io_err;
        assert!(as_dyn.source().is_some());
    }

    async fn run_copy_with_durability(policy: DurabilityPolicy) -> (u64, Vec<u8>) {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("a.txt");
        std::fs::write(&src, b"hello-durability").unwrap();
        let dst = tmp.path().join("out.txt");
        let opts = CopyOptions {
            durability: policy,
            ..CopyOptions::default()
        };
        let plan = crate::scanner::build_plan(&src, &dst, &opts).unwrap();
        let (tx, _rx) = mpsc::channel(64);
        let engine = CopyEngine::new(opts, tx);
        let report = engine.run(plan).await;
        let content = std::fs::read(&dst).unwrap();
        // tmp vive hasta aquí: la lectura ocurre antes de limpiar el dir.
        (report.files_copied, content)
    }

    #[tokio::test]
    async fn durability_none_copies_content() {
        let (copied, content) = run_copy_with_durability(DurabilityPolicy::None).await;
        assert_eq!(copied, 1);
        assert_eq!(content, b"hello-durability");
    }

    #[tokio::test]
    async fn durability_file_copies_content() {
        let (copied, content) = run_copy_with_durability(DurabilityPolicy::File).await;
        assert_eq!(copied, 1);
        assert_eq!(content, b"hello-durability");
    }

    async fn run_failing_plan(max_errors: usize) -> Report {
        let opts = CopyOptions {
            max_errors,
            ..CopyOptions::default()
        };
        let tasks: Vec<CopyTask> = (0..5)
            .map(|i| CopyTask {
                src: PathBuf::from(format!("/nonexistent-cf-src-{i}")),
                dst: PathBuf::from(format!("/tmp/nonexistent-cf-dst-{i}")),
                size: 0,
                kind: TaskKind::File,
            })
            .collect();
        let plan = CopyPlan {
            tasks,
            options: opts.clone(),
            total_bytes: 0,
        };
        let (tx, _rx) = mpsc::channel(64);
        CopyEngine::new(opts, tx).run(plan).await
    }

    #[tokio::test]
    async fn errors_capped_at_max_errors() {
        let report = run_failing_plan(2).await;
        assert_eq!(report.files_failed, 5);
        assert_eq!(report.errors.len(), 2);
        assert_eq!(report.errors_truncated, 3);
    }

    #[tokio::test]
    async fn errors_not_capped_below_limit() {
        let report = run_failing_plan(1000).await;
        assert_eq!(report.files_failed, 5);
        assert_eq!(report.errors.len(), 5);
        assert_eq!(report.errors_truncated, 0);
    }
}