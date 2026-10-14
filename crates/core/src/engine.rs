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

        // P1.4: limpieza best-effort de temporales huérfanos de ejecuciones
        // anteriores/interrumpidas, antes de copiar.
        cleanup_orphan_tmps(&plan.tasks).await;

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
                FileResult::Fallback { src, dst, bytes, is_dir, reason } => {
                    let _ = self.events_tx.send(CopyEvent::SymlinkFallback { path: src.clone(), target: dst.clone(), reason: reason.clone() }).await;
                    report.symlinks_fallback.push((src, dst));
                    if is_dir {
                        report.dirs_created += 1;
                    } else {
                        report.files_copied += 1;
                        report.bytes_copied += bytes;
                    }
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
    /// P1.6: enlace recreado como contenido por falta de privilegio.
    Fallback { src: PathBuf, dst: PathBuf, bytes: u64, is_dir: bool, reason: String },
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
        return copy_symlink_followed(task, opts, events, cancel).await;
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
        // TODO(P1.6): symlink roto apuntando a directorio se recrea como file
        // en Windows (CreateSymbolicLink no valida el target) porque aquí no
        // hay forma fiable de saber el tipo original: metadata() falla y el
        // enlace no lo almacena. Limitación conocida, ver docs/PLAN.md P1.6.
        let res = if is_dir {
            platform::symlink_dir(&link_target, &task.dst)
        } else {
            platform::symlink_file(&link_target, &task.dst)
        };
        if let Err(e) = res {
            // P1.6: sin privilegio para crear el enlace (Windows sin
            // Developer Mode) no se falla: se copia el contenido como si se
            // siguiera el enlace, y se reporta el fallback.
            if platform::is_privilege_error(&e) {
                let reason = e.to_string();
                debug!("symlink sin privilegio, fallback a contenido: {:?} ({reason})", task.src);
                return match copy_symlink_followed(task, opts, events, cancel).await {
                    FileResult::Copied { bytes } => FileResult::Fallback {
                        src: task.src.clone(), dst: task.dst.clone(), bytes, is_dir: false, reason,
                    },
                    FileResult::DirCreated => FileResult::Fallback {
                        src: task.src.clone(), dst: task.dst.clone(), bytes: 0, is_dir: true, reason,
                    },
                    other => other,
                };
            }
            return FileResult::Failed { path: task.dst.clone(), error: CopyError::Io { path: task.dst.clone(), source: e } };
        }

        debug!("symlink recreado: {:?} -> {:?} (target {:?})", task.src, task.dst, link_target);
        FileResult::Symlinked { src: task.src.clone(), dst: task.dst.clone() }
    }
}

/// Copiar el contenido resuelto de un symlink (modo follow).
///
/// El caso directorio recursivo ya viene expandido por el scanner
/// (WalkDir con follow_links=true); aquí basta con asegurar el dir
/// destino o delegar a copy_file si es un fichero.
async fn copy_symlink_followed(
    task: &CopyTask,
    opts: &CopyOptions,
    events: &mpsc::Sender<CopyEvent>,
    cancel: &CancellationToken,
) -> FileResult {
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
            // Fix 3: lo avisa run(). Never = ya existía; el resto de
            // políticas que coinciden equivalen a idéntico (P1.3).
            let reason = match opts.overwrite {
                OverwritePolicy::Never => SkipReason::AlreadyExists,
                _ => SkipReason::Identical,
            };
            return FileResult::Skipped { path: task.dst.clone(), reason };
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

    // --- temporal único en el mismo FS que dst (P1.4) ---
    // NamedTempFile reserva un nombre único (O_EXCL) con prefijo reconocible
    // y borra el archivo al hacer Drop: ningún retorno de error deja huérfano.
    let named = match create_temp_in(&task.dst) {
        Ok(n) => n,
        // Fix 7: reporta dst, no tmp
        Err(e) => return FileResult::Failed { path: task.dst.clone(), error: CopyError::Io { path: task.dst.clone(), source: e } },
    };
    // Handle async sobre el mismo inode (reopen duplica el fd, cursor en 0).
    // `named` se mantiene vivo para retener el path y el auto-borrado.
    let mut dst = match named.reopen() {
        Ok(f) => tokio::fs::File::from_std(f),
        Err(e) => return FileResult::Failed { path: task.dst.clone(), error: CopyError::Io { path: task.dst.clone(), source: e } },
    };

    let mut buf = vec![0u8; opts.buffer_size.max(64 * 1024)];
    let mut copied: u64 = 0;
    let mut last_emit = Instant::now();

    loop {
        if cancel.is_cancelled() {
            drop(dst);
            // `named` se dropea aquí y limpia el temporal.
            return FileResult::Failed { path: task.src.clone(), error: CopyError::Cancelled };
        }

        let n = match src.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                drop(dst);
                return FileResult::Failed { path: task.src.clone(), error: CopyError::Io { path: task.src.clone(), source: e } };
            }
        };

        if let Err(e) = dst.write_all(&buf[..n]).await {
            drop(dst);
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
        return FileResult::Failed { path: task.dst.clone(), error: CopyError::Io { path: task.dst.clone(), source: e } };
    }

    // --- durabilidad pre-rename (P0.6a): fsync del temporal según política ---
    if let Err(e) = apply_durability(opts.durability, &dst, &task.dst).await {
        drop(dst);
        return FileResult::Failed { path: task.dst.clone(), error: e };
    }
    drop(dst);

    // A partir de aquí se opera por path sobre el temporal retenido por `named`.
    let tmp_path = named.path().to_path_buf();

    // --- verificación ---
    match opts.verify {
        VerifyPolicy::None => {}
        VerifyPolicy::Size => {
            if let Ok(m) = fs::metadata(&tmp_path).await {
                if m.len() != task.size {
                    return FileResult::Failed {
                        path: task.dst.clone(),
                        error: CopyError::VerifyFailed(task.dst.clone()),
                    };
                }
            }
        }
        VerifyPolicy::Hash => {
            if let Err(e) = verify_hash(&task.src, &tmp_path, &task.dst).await {
                return FileResult::Failed { path: task.dst.clone(), error: e };
            }
        }
    }

    // --- permisos (mode Unix; el resto de metadatos va tras el rename) ---
    if opts.preserve_metadata {
        if let Ok(meta) = fs::metadata(&task.src).await {
            let _ = fs::set_permissions(&tmp_path, meta.permissions()).await;
        }
    }

    // --- rename atómico (P1.1b: vía platform::atomic_replace) ---
    // Se convierte a TempPath para desarmar el auto-borrado: tras el rename
    // el path es el destino final y el Drop NO debe eliminarlo (opción B:
    // no se usa persist() para no perder el ReplaceFileW de Windows).
    let temp_path = named.into_temp_path();
    if let Err(e) = platform::atomic_replace(&tmp_path, &task.dst) {
        // `temp_path` se dropea aquí y limpia el temporal.
        return FileResult::Failed { path: task.dst.clone(), error: CopyError::Io { path: task.dst.clone(), source: e } };
    }
    // IMPORTANTE: NO llamar a temp_path.keep() tras atomic_replace.
    // En Unix keep() es no-op; en Windows ejecuta SetFileAttributesW sobre el
    // path viejo del temporal, que ya no existe tras el rename → ERROR_FILE_NOT_FOUND
    // → se contaría FileResult::Failed aunque la copia fue correcta.
    // El Drop del TempPath es un no-op inofensivo sobre un path ya movido.
    // Ver CI run 2026-10-14T05:37 para evidencia.
    drop(temp_path);

    // --- metadatos post-rename (P1.3): mtime/atime + readonly Windows ---
    // Best-effort: si falla, la copia ya es correcta; se registra en debug
    // y se sigue (fallar aquí provocaría recopiados, no corrupción).
    if opts.preserve_metadata {
        if let Ok(src_meta) = fs::metadata(&task.src).await {
            if let Ok(mtime) = src_meta.modified() {
                let atime = src_meta.accessed().unwrap_or(mtime);
                if let Err(e) = filetime::set_file_times(
                    &task.dst,
                    filetime::FileTime::from_system_time(atime),
                    filetime::FileTime::from_system_time(mtime),
                ) {
                    debug!("no se pudo preservar mtime de {:?}: {e}", task.dst);
                }
            }
            #[cfg(windows)]
            {
                if src_meta.permissions().readonly() {
                    let _ = platform::set_readonly(&task.dst, true);
                }
            }
        }
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

/// Prefijo reconocible de los temporales de copia (P1.4). Una sola fuente
/// para creación (`create_temp_in`) y limpieza (`cleanup_orphan_tmps`).
const TEMP_PREFIX: &str = ".copyforge-tmp-";

/// Antigüedad mínima para considerar un temporal huérfano (P1.4): evita
/// borrar el temporal vivo de otra copia concurrente en el mismo directorio.
/// Riesgo residual documentado: una copia que retenga su temporal más de
/// una hora mientras otro run limpia podría ver su rename fallar (el
/// reintento lo recupera; el origen jamás se toca).
const ORPHAN_MAX_AGE_SECS: u64 = 3600;

/// Crea un temporal único en el mismo directorio que `dst` (P1.4).
///
/// Mismo directorio = mismo filesystem = el rename posterior es atómico
/// (entre FS distintos no lo sería). Sin `unwrap`/`expect`.
fn create_temp_in(dst: &Path) -> std::io::Result<tempfile::NamedTempFile> {
    let parent = dst.parent().filter(|p| !p.as_os_str().is_empty());
    let dir = parent.unwrap_or(Path::new("."));
    tempfile::Builder::new()
        .prefix(TEMP_PREFIX)
        .tempfile_in(dir)
}

/// Limpieza best-effort de temporales huérfanos (P1.4).
///
/// Recorre los directorios padre de las tareas (una sola profundidad por
/// directorio, sin recursión) y elimina los ficheros `.copyforge-tmp-*`
/// con antigüedad mayor a `ORPHAN_MAX_AGE_SECS`. Los errores se ignoran.
/// Sin `unwrap`/`expect`.
async fn cleanup_orphan_tmps(tasks: &[CopyTask]) {
    use std::collections::HashSet;
    let mut seen: HashSet<&Path> = HashSet::new();
    for task in tasks {
        let Some(parent) = task.dst.parent() else { continue };
        if !seen.insert(parent) {
            continue;
        }
        let Ok(mut rd) = fs::read_dir(parent).await else { continue };
        while let Ok(Some(ent)) = rd.next_entry().await {
            if !ent.file_name().to_string_lossy().starts_with(TEMP_PREFIX) {
                continue;
            }
            let old_enough = ent
                .metadata()
                .await
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| std::time::SystemTime::now().duration_since(t).ok())
                .map(|age| age.as_secs() >= ORPHAN_MAX_AGE_SECS)
                .unwrap_or(false);
            if old_enough {
                let _ = fs::remove_file(ent.path()).await;
            }
        }
    }
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

    #[cfg(unix)]
    #[tokio::test]
    async fn preserve_metadata_keeps_mtime() {
        use std::time::{Duration, UNIX_EPOCH};
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("a.txt");
        std::fs::write(&src, b"data").unwrap();
        let mtime = UNIX_EPOCH + Duration::from_secs(1_600_000_000);
        filetime::set_file_mtime(&src, filetime::FileTime::from_system_time(mtime)).unwrap();
        let dst = tmp.path().join("b.txt");
        let opts = CopyOptions::default();
        let plan = crate::scanner::build_plan(&src, &dst, &opts).unwrap();
        let (tx, _rx) = mpsc::channel(64);
        let report = CopyEngine::new(opts, tx).run(plan).await;
        assert_eq!(report.files_copied, 1);
        let got = std::fs::metadata(&dst).unwrap().modified().unwrap();
        let diff = got
            .duration_since(mtime)
            .unwrap_or_else(|e| e.duration());
        assert!(diff <= Duration::from_secs(1), "mtime diff too big: {diff:?}");
    }

    #[tokio::test]
    async fn if_different_second_pass_skips_all() {
        // OJO: con dir+dst-existente el efectivo anida (dst/src) por diseño
        // P0.2, así que la idempotencia estable se prueba con un fichero:
        // src=file, dst=file es estable entre pasadas.
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("a.txt");
        std::fs::write(&src, b"aaa").unwrap();
        let dst = tmp.path().join("b.txt");
        let opts = CopyOptions {
            overwrite: OverwritePolicy::IfDifferent,
            ..CopyOptions::default()
        };
        let (tx, _rx) = mpsc::channel(1024);
        let plan = crate::scanner::build_plan(&src, &dst, &opts).unwrap();
        let first = CopyEngine::new(opts.clone(), tx).run(plan).await;
        assert_eq!(first.files_copied, 1);
        let (tx2, mut rx2) = mpsc::channel(1024);
        let plan2 = crate::scanner::build_plan(&src, &dst, &opts).unwrap();
        let second = CopyEngine::new(opts, tx2).run(plan2).await;
        assert_eq!(second.files_copied, 0);
        assert_eq!(second.files_skipped, 1);
        let mut identical = 0;
        while let Ok(ev) = rx2.try_recv() {
            if let CopyEvent::FileSkipped {
                reason: SkipReason::Identical,
                ..
            } = ev
            {
                identical += 1;
            }
        }
        assert_eq!(identical, 1);
    }

    // P1.4: un fallo a mitad de copia (aquí, size declarado != real, que
    // falla en la verificación) no debe dejar `.copyforge-tmp-*` atrás:
    // el Drop del NamedTempFile limpia solo.
    #[cfg(unix)]
    #[tokio::test]
    async fn failed_copy_leaves_no_temp() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("a.txt");
        std::fs::write(&src, b"12345").unwrap();
        let dst = tmp.path().join("b.txt");
        let opts = CopyOptions::default();
        let plan = CopyPlan {
            tasks: vec![CopyTask {
                src: src.clone(),
                dst: dst.clone(),
                size: 5000,
                kind: TaskKind::File,
            }],
            options: opts.clone(),
            total_bytes: 5000,
        };
        let (tx, _rx) = mpsc::channel(64);
        let report = CopyEngine::new(opts, tx).run(plan).await;
        assert_eq!(report.files_failed, 1);
        let leftovers: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(".copyforge-tmp-")
            })
            .collect();
        assert!(leftovers.is_empty());
    }

    // P1.4: un huérfano antiguo se elimina al inicio de run().
    #[cfg(unix)]
    #[tokio::test]
    async fn run_cleans_stale_temp_orphans() {
        let tmp = tempfile::tempdir().unwrap();
        let orphan = tmp.path().join(".copyforge-tmp-manual");
        std::fs::write(&orphan, b"stale").unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
        filetime::set_file_mtime(&orphan, filetime::FileTime::from_system_time(old)).unwrap();
        let src = tmp.path().join("a.txt");
        std::fs::write(&src, b"data").unwrap();
        let dst = tmp.path().join("b.txt");
        let opts = CopyOptions::default();
        let plan = crate::scanner::build_plan(&src, &dst, &opts).unwrap();
        let (tx, _rx) = mpsc::channel(64);
        let report = CopyEngine::new(opts, tx).run(plan).await;
        assert_eq!(report.files_copied, 1);
        assert!(!orphan.exists());
    }

    // P1.4: el gate de antigüedad protege el temporal vivo de otra copia
    // concurrente: un huérfano RECIENTE no se toca.
    #[cfg(unix)]
    #[tokio::test]
    async fn run_keeps_fresh_temp_files() {
        let tmp = tempfile::tempdir().unwrap();
        let fresh = tmp.path().join(".copyforge-tmp-live");
        std::fs::write(&fresh, b"live").unwrap();
        let src = tmp.path().join("a.txt");
        std::fs::write(&src, b"data").unwrap();
        let dst = tmp.path().join("b.txt");
        let opts = CopyOptions::default();
        let plan = crate::scanner::build_plan(&src, &dst, &opts).unwrap();
        let (tx, _rx) = mpsc::channel(64);
        let report = CopyEngine::new(opts, tx).run(plan).await;
        assert_eq!(report.files_copied, 1);
        assert!(fresh.exists());
    }

    // P1.6: regresión — recrear symlinks sigue funcionando tras el refactor
    // (el fallback solo salta con error de privilegio).
    #[cfg(unix)]
    #[tokio::test]
    async fn symlink_recreate_still_works() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("real.txt");
        std::fs::write(&target, b"contenido").unwrap();
        let link = tmp.path().join("link.txt");
        platform::symlink_file(&target, &link).unwrap();
        let dst = tmp.path().join("out-link.txt");
        let opts = CopyOptions::default();
        let plan = crate::scanner::build_plan(&link, &dst, &opts).unwrap();
        let (tx, _rx) = mpsc::channel(64);
        let report = CopyEngine::new(opts, tx).run(plan).await;
        assert_eq!(report.files_failed, 0);
        assert_eq!(report.symlinks_created, 1);
        let meta = std::fs::symlink_metadata(&dst).unwrap();
        assert!(meta.file_type().is_symlink());
        assert_eq!(std::fs::read_link(&dst).unwrap(), target);
    }

    // P1.6: con Developer Mode recrea el enlace; sin él (EACCES) el fallback
    // copia el contenido. Sin privilegio ni el fixture se puede crear: en ese
    // caso se pasa en vacío (documentado). Ambos caminos son éxito.
    #[cfg(windows)]
    #[tokio::test]
    async fn symlink_recreate_or_fallback_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("real.txt");
        std::fs::write(&target, b"contenido").unwrap();
        let link = tmp.path().join("link.txt");
        let Ok(()) = platform::symlink_file(&target, &link) else {
            return;
        };
        let dst = tmp.path().join("out-link.txt");
        let opts = CopyOptions::default();
        let plan = crate::scanner::build_plan(&link, &dst, &opts).unwrap();
        let (tx, _rx) = mpsc::channel(64);
        let report = CopyEngine::new(opts, tx).run(plan).await;
        assert_eq!(report.files_failed, 0);
        assert_eq!(std::fs::read(&dst).unwrap(), b"contenido");
        if std::fs::symlink_metadata(&dst).unwrap().file_type().is_symlink() {
            assert_eq!(report.symlinks_created, 1);
        } else {
            assert_eq!(report.files_copied, 1);
            assert_eq!(report.symlinks_fallback.len(), 1);
        }
    }
}