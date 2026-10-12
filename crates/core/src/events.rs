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
    pub errors: Vec<(PathBuf, String)>,
    pub duration: Duration,
}