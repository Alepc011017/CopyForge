use std::path::PathBuf;
use crate::options::CopyOptions;

#[derive(Debug, Clone)]
pub struct CopyPlan {
    pub tasks: Vec<CopyTask>,
    pub options: CopyOptions,
    pub total_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct CopyTask {
    pub src: PathBuf,
    pub dst: PathBuf,
    pub size: u64,
    pub kind: TaskKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind { File, Dir, Symlink }