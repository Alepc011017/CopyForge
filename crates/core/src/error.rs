use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CopyError {
    #[error("IO error en {path}: {source}")]
    Io { path: PathBuf, #[source] source: std::io::Error },

    #[error("origen no encontrado: {0}")]
    SourceNotFound(PathBuf),

    #[error("verificación fallida: {0}")]
    VerifyFailed(PathBuf),

    #[error("cancelado")]
    Cancelled,
}