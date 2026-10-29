use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CopyError {
    #[error("IO error en {path}: {source}")]
    Io { path: PathBuf, #[source] source: std::io::Error },

    #[error("origen no encontrado: {0}")]
    SourceNotFound(PathBuf),

    #[error("verificación fallida: {0}")]
    VerifyFailed(PathBuf),

    #[error("cancelado")]
    Cancelled,

    /// Destino inválido (P0.5, H-4): `src == dst` o contención mutua entre
    /// origen y destino, detectado por `scanner::validate_paths`.
    ///
    /// `reason` ya viene redactado para el usuario en español: "el origen y el
    /// destino son la misma ruta" u "origen y destino se contienen mutuamente".
    #[error("destino inválido: {reason}")]
    InvalidDestination { reason: String },

    #[error("symlink fuera de src: {path} -> {target}")]
    SymlinkEscapesSource { path: PathBuf, target: PathBuf },

    #[error("límite de escape superado: {current} > {limit} bytes")]
    EscapeCapExceeded { limit: u64, current: u64 },
}