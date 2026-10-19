//! CLI de la GUI (P4.6a): arranque y re-lanzamiento con rutas.
//!
//! Permite `copyforge-gui-tauri copy <src...> --to <dst>` (y `move`), de modo que
//! una segunda invocación del binario puede encolar otra transferencia sin
//! levantar una segunda ventana (`tauri_plugin_single_instance`).

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::{GuiOptions, TransferMode, TransferRequest};

/// Argumentos de línea de comandos de la GUI.
#[derive(Debug, Parser)]
#[command(name = "copyforge-gui-tauri", version, about = "CopyForge (GUI)")]
pub struct LaunchArgs {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Copia uno o más orígenes a un destino.
    Copy(TransferArgs),
    /// Copia y, si todo va bien, borra los orígenes.
    Move(TransferArgs),
}

/// Opciones de una transferencia de la CLI.
#[derive(Debug, clap::Args, Clone)]
pub struct TransferArgs {
    /// Origenes (ficheros o carpetas).
    #[arg(required = true)]
    pub srcs: Vec<PathBuf>,

    /// Destino.
    #[arg(long, required = true)]
    pub to: PathBuf,

    /// Política de sobrescritura: never/if-newer/if-different/always.
    #[arg(long, default_value = "always")]
    pub overwrite: String,

    /// Verificación: none/size/hash.
    #[arg(long, default_value = "size")]
    pub verify: String,

    /// Durabilidad: none/file/full.
    #[arg(long, default_value = "file")]
    pub durability: String,

    /// Ficheros en paralelo.
    #[arg(long, default_value = "4")]
    pub jobs: usize,

    /// Exclusiones (globs separados por comas).
    #[arg(long, default_value = "")]
    pub exclude: String,
}

impl TransferArgs {
    /// Convierte los argumentos en una [`TransferRequest`] para el frontend.
    pub fn into_request(self, mode: TransferMode) -> TransferRequest {
        TransferRequest {
            srcs: self
                .srcs
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
            dst: self.to.to_string_lossy().into_owned(),
            options: self.into(),
            mode,
        }
    }
}

impl From<TransferArgs> for GuiOptions {
    fn from(t: TransferArgs) -> Self {
        Self {
            // La CLI no expone `preserve_metadata`: toma el default del motor.
            preserve_metadata: true,
            overwrite: parse_overwrite(&t.overwrite),
            verify: parse_verify(&t.verify),
            jobs: t.jobs,
            exclude: t.exclude,
            durability: parse_durability(&t.durability),
        }
    }
}

fn parse_overwrite(raw: &str) -> crate::OverwriteDto {
    match raw.to_ascii_lowercase().as_str() {
        "never" => crate::OverwriteDto::Never,
        "if-newer" => crate::OverwriteDto::IfNewer,
        "if-different" => crate::OverwriteDto::IfDifferent,
        _ => crate::OverwriteDto::Always,
    }
}

fn parse_verify(raw: &str) -> crate::VerifyDto {
    match raw.to_ascii_lowercase().as_str() {
        "none" => crate::VerifyDto::None,
        "hash" => crate::VerifyDto::Hash,
        _ => crate::VerifyDto::Size,
    }
}

fn parse_durability(raw: &str) -> crate::DurabilityDto {
    match raw.to_ascii_lowercase().as_str() {
        "none" => crate::DurabilityDto::None,
        "full" => crate::DurabilityDto::Full,
        _ => crate::DurabilityDto::File,
    }
}

impl LaunchArgs {
    /// Descomando la transferencia pendiente, si la hay.
    pub fn into_transfer_request(self) -> Option<TransferRequest> {
        match self.command {
            Some(Command::Copy(a)) => Some(a.into_request(TransferMode::Copy)),
            Some(Command::Move(a)) => Some(a.into_request(TransferMode::Move)),
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_parses_to_gui_options_with_defaults() {
        let args = LaunchArgs::try_parse_from([
            "copyforge-gui-tauri",
            "copy",
            "/tmp/a",
            "/tmp/b",
            "--to",
            "/tmp/dst",
        ])
        .expect("parsea copy")
        .into_transfer_request()
        .expect("hay transferencia");

        let gui = args.options;
        assert_eq!(gui.jobs, 4);
        assert_eq!(gui.overwrite, crate::OverwriteDto::Always);
        assert_eq!(gui.verify, crate::VerifyDto::Size);
        assert_eq!(gui.durability, crate::DurabilityDto::File);
        assert!(args.srcs.contains(&"/tmp/a".to_string()));
        assert_eq!(args.dst, "/tmp/dst");
        assert_eq!(args.mode, TransferMode::Copy);
    }

    #[test]
    fn move_flags_mode_and_parses_flags() {
        let parsed = LaunchArgs::try_parse_from([
            "copyforge-gui-tauri",
            "move",
            "/a",
            "/b",
            "--to",
            "/dst",
            "--jobs",
            "8",
            "--overwrite",
            "never",
            "--verify",
            "hash",
            "--durability",
            "full",
            "--exclude",
            "*.log, target/**",
        ])
        .expect("parsea move");

        let request = parsed.into_transfer_request().expect("hay transferencia");
        assert_eq!(request.mode, TransferMode::Move);
        assert_eq!(request.srcs.len(), 2);

        let gui = request.options;
        assert_eq!(gui.jobs, 8);
        assert_eq!(gui.overwrite, crate::OverwriteDto::Never);
        assert_eq!(gui.verify, crate::VerifyDto::Hash);
        assert_eq!(gui.durability, crate::DurabilityDto::Full);
        assert_eq!(gui.exclude, "*.log, target/**");
    }

    #[test]
    fn unknown_enums_fall_back_to_defaults() {
        let parsed = LaunchArgs::try_parse_from([
            "copyforge-gui-tauri",
            "copy",
            "/a",
            "--to",
            "/b",
            "--overwrite",
            "whatever",
            "--verify",
            "nope",
            "--durability",
            "zzz",
        ])
        .expect("parsea con valores desconocidos");

        let gui = parsed
            .into_transfer_request()
            .expect("hay transferencia")
            .options;
        assert_eq!(gui.overwrite, crate::OverwriteDto::Always);
        assert_eq!(gui.verify, crate::VerifyDto::Size);
        assert_eq!(gui.durability, crate::DurabilityDto::File);
    }

    #[test]
    fn no_subcommand_means_no_transfer() {
        let parsed = LaunchArgs::try_parse_from(["copyforge-gui-tauri"]).expect("parsea sin subcomando");
        assert!(parsed.into_transfer_request().is_none());
    }
}
