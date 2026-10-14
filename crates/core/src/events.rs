use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum CopyEvent {
    Planned { total_files: usize, total_bytes: u64 },
    FileStarted { path: PathBuf, size: u64 },
    Progress { path: PathBuf, bytes_copied: u64, total: u64 },
    FileCompleted { path: PathBuf, elapsed: Duration },
    FileSkipped { path: PathBuf, reason: SkipReason },
    FileFailed { path: PathBuf, error: String },
    Symlinked { src: PathBuf, dst: PathBuf },
    /// Un enlace no se pudo recrear (típ. falta de privilegio en Windows)
    /// y se copió su contenido en su lugar. `path` = enlace origen,
    /// `target` = destino donde quedó el contenido, `reason` = causa.
    SymlinkFallback { path: PathBuf, target: PathBuf, reason: String },
    Finished { report: Report },
}

#[derive(Debug, Clone, Copy)]
pub enum SkipReason { AlreadyExists, Identical, Excluded }

#[derive(Debug, Clone, Default)]
pub struct Report {
    pub files_copied: u64,
    pub files_skipped: u64,
    pub files_failed: u64,
    pub bytes_copied: u64,
    pub dirs_created: u64, // Fix 2: dirs aparte, no cuentan como files
    pub symlinks_created: u64,
    pub symlinked: Vec<(PathBuf, PathBuf)>,
    /// Enlaces recreados como contenido por falta de privilegio (P1.6):
    /// pares (origen, destino). El conteo es `len()`.
    pub symlinks_fallback: Vec<(PathBuf, PathBuf)>,
    pub errors: Vec<(PathBuf, String)>,
    /// Errores omitidos de `errors` por el cap `max_errors` (P0.7).
    pub errors_truncated: usize,
    pub duration: Duration,
}