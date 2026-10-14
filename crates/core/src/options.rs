#[derive(Debug, Clone)]
pub struct CopyOptions {
    pub overwrite: OverwritePolicy,
    pub verify: VerifyPolicy,
    /// P1.3 (antes `preserve_permissions`): preserva metadatos, no solo permisos.
    ///
    /// Se preserva: mode Unix (bits rwx), mtime + atime (ambas plataformas
    /// vía `filetime`), flag readonly en Windows (si el origen lo era).
    /// NO se preserva en v1 (limitación conocida): ownership (uid/gid),
    /// ACLs, xattrs ni tiempos de creación (btime).
    pub preserve_metadata: bool,
    pub follow_symlinks: bool,
    pub buffer_size: usize,        // bytes por lectura (default 256 KiB)
    pub parallel_files: usize,     // archivos en paralelo
    pub continue_on_error: bool,
    pub exclusion: Vec<String>,    // globs relativos a la raíz
    /// P0.9: permite que symlinks seguidos apunten fuera de `src`.
    /// Default `false` (seguro): se aborta con `SymlinkEscapesSource`.
    pub allow_symlink_escape: bool,
    /// P0.9: cap opcional (bytes) del árbol externo cuando
    /// `allow_symlink_escape=true`. Defensa en profundidad; default `None`.
    pub max_escape_bytes: Option<u64>,
    /// P0.6a: trade-off explícito rendimiento ↔ durabilidad.
    /// Default `File` (cambio respecto al comportamiento anterior,
    /// que equivalía a `None`).
    pub durability: DurabilityPolicy,
    /// P0.7: cap de `Report.errors`. Se preservan los primeros `max_errors`
    /// para diagnóstico; `errors_truncated` indica cuántos se omitieron.
    /// Aplica al Vec completo, no por origen. Default 1000.
    pub max_errors: usize,
}

impl Default for CopyOptions {
    fn default() -> Self {
        Self {
            overwrite: OverwritePolicy::Always,
            verify: VerifyPolicy::Size,
            preserve_metadata: true,
            follow_symlinks: false,
            buffer_size: 256 * 1024,
            parallel_files: 4,
            continue_on_error: true,
            exclusion: Vec::new(),
            allow_symlink_escape: false,
            max_escape_bytes: None,
            durability: DurabilityPolicy::default(),
            max_errors: 1000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverwritePolicy { Always, Never, IfNewer, IfDifferent }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyPolicy { None, Size, Hash }

/// Política de durabilidad: trade-off explícito rendimiento ↔ durabilidad.
///
/// | Variante | fsync fichero (pre-rename) | fsync dir padre (post-rename) | Ante caída de SOP / corte de luz |
/// |---|---|---|---|
/// | `None` | no | no | puede perderse el fichero (comportamiento anterior) |
/// | `File` (default) | sí (`sync_all` del temporal) | no | contenido durable; la entrada del directorio puede perderse → fichero ausente, nunca corrupto |
/// | `Full` | sí | sí en Unix (`sync_dir` del padre); en Windows equivale a `File` (NTFS journaliza los metadatos, no necesita fsync del directorio) | ni el dato ni el nombre se pierden (semántica TeraCopy en Unix; en Windows, garantía de NTFS) |
///
/// Coste: ~1 `fsync` por fichero en `File`; +1 por directorio padre en `Full`
/// (más lento en HDD/NFS).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DurabilityPolicy { None, #[default] File, Full }