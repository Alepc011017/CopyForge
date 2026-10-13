use std::path::PathBuf;
use clap::{Parser, ValueEnum};
use copyforge_core::{CopyOptions, DurabilityPolicy, OverwritePolicy, VerifyPolicy};

#[derive(Parser, Debug)]
#[command(
    name = "copyforge",
    version,
    about = "Copia archivos y carpetas con verificación y progreso",
    long_about = None,
)]
pub struct Args {
    /// Origen (archivo o carpeta)
    pub source: PathBuf,

    /// Destino (archivo o carpeta)
    pub dest: PathBuf,

    /// Sobrescribir archivos existentes
    #[arg(short = 'f', long, value_enum, default_value_t = OverwriteArg::Always)]
    pub overwrite: OverwriteArg,

    /// Verificación post-copia
    #[arg(short = 'c', long, value_enum, default_value_t = VerifyArg::Size)]
    pub verify: VerifyArg,

    /// Durabilidad: trade-off rendimiento ↔ durabilidad
    #[arg(long, value_enum, default_value_t = DurabilityArg::File)]
    pub durability: DurabilityArg,

    /// Archivos copiados en paralelo
    #[arg(short = 'j', long, default_value_t = 4)]
    pub jobs: usize,

    /// Tamaño del buffer en KiB
    #[arg(short = 'b', long, default_value_t = 256)]
    pub buffer_kib: usize,

    /// Patrones glob a excluir (pueden repetirse). Ej: -x "*.log" -x "target/**"
    #[arg(short = 'x', long = "exclude")]
    pub exclude: Vec<String>,

    /// No preservar permisos
    #[arg(long)]
    pub no_preserve: bool,

    /// Seguir symlinks
    #[arg(long)]
    pub follow_symlinks: bool,

    /// Permitir que los symlinks seguidos apunten fuera de src (peligroso)
    #[arg(long)]
    pub allow_symlink_escape: bool,

    /// Detener al primer error
    #[arg(long)]
    pub fail_fast: bool,

    /// Mostrar qué se haría sin copiar
    #[arg(long)]
    pub dry_run: bool,

    /// Silenciar progreso (solo resultado final)
    #[arg(short = 'q', long)]
    pub quiet: bool,

    /// Aumentar verbosidad (-v, -vv)
    #[arg(short = 'v', long, action = clap::ArgAction::Count)]
    pub verbose: u8,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OverwriteArg { Always, Never, IfNewer, IfDifferent }

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum VerifyArg { None, Size, Hash }

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum DurabilityArg { None, File, Full }

impl From<OverwriteArg> for OverwritePolicy {
    fn from(a: OverwriteArg) -> Self {
        match a {
            OverwriteArg::Always => OverwritePolicy::Always,
            OverwriteArg::Never => OverwritePolicy::Never,
            OverwriteArg::IfNewer => OverwritePolicy::IfNewer,
            OverwriteArg::IfDifferent => OverwritePolicy::IfDifferent,
        }
    }
}

impl From<VerifyArg> for VerifyPolicy {
    fn from(a: VerifyArg) -> Self {
        match a {
            VerifyArg::None => VerifyPolicy::None,
            VerifyArg::Size => VerifyPolicy::Size,
            VerifyArg::Hash => VerifyPolicy::Hash,
        }
    }
}

impl From<DurabilityArg> for DurabilityPolicy {
    fn from(a: DurabilityArg) -> Self {
        match a {
            DurabilityArg::None => DurabilityPolicy::None,
            DurabilityArg::File => DurabilityPolicy::File,
            DurabilityArg::Full => DurabilityPolicy::Full,
        }
    }
}

impl Args {
    pub fn to_options(&self) -> CopyOptions {
        CopyOptions {
            overwrite: self.overwrite.into(),
            verify: self.verify.into(),
            preserve_permissions: !self.no_preserve,
            follow_symlinks: self.follow_symlinks,
            buffer_size: self.buffer_kib * 1024,
            parallel_files: self.jobs.max(1),
            continue_on_error: !self.fail_fast,
            exclusion: self.exclude.clone(),
            allow_symlink_escape: self.allow_symlink_escape,
            max_escape_bytes: None,
            durability: self.durability.into(),
        }
    }
}