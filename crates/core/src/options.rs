#[derive(Debug, Clone)]
pub struct CopyOptions {
    pub overwrite: OverwritePolicy,
    pub verify: VerifyPolicy,
    pub preserve_permissions: bool,
    pub follow_symlinks: bool,
    pub buffer_size: usize,        // bytes por lectura (default 256 KiB)
    pub parallel_files: usize,     // archivos en paralelo
    pub continue_on_error: bool,
    pub exclusion: Vec<String>,    // globs relativos a la raíz
}

impl Default for CopyOptions {
    fn default() -> Self {
        Self {
            overwrite: OverwritePolicy::Always,
            verify: VerifyPolicy::Size,
            preserve_permissions: true,
            follow_symlinks: false,
            buffer_size: 256 * 1024,
            parallel_files: 4,
            continue_on_error: true,
            exclusion: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverwritePolicy { Always, Never, IfNewer, IfDifferent }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyPolicy { None, Size, Hash }