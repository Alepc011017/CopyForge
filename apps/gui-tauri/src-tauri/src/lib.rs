//! Backend Tauri de CopyForge (P4.2).
//!
//! Expone las opciones de copia al frontend mediante los comandos
//! `get_options` / `set_options`. Los tipos de `copyforge_core` no implementan
//! `Serialize`/`Deserialize` (y el motor no se toca en esta fase), así que
//! [`GuiOptions`] actúa de DTO en la frontera y se mapea a `CopyOptions`.
//!
//! En P4.3 `set_options` alimentará el arranque de la copia; de momento solo
//! registra lo recibido con `tracing::info!`.

use copyforge_core::{CopyOptions, DurabilityPolicy, OverwritePolicy, VerifyPolicy};
use serde::{Deserialize, Serialize};

/// DTO de frontera con el frontend (React).
///
/// Campos en `snake_case` para que coincidan 1:1 con la interfaz `GuiOptions`
/// de TypeScript. `exclude` es una lista de globs separada por comas (una sola
/// línea en la UI); se convierte a `Vec<String>` al mapear a `CopyOptions`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuiOptions {
    pub preserve_metadata: bool,
    pub overwrite: OverwriteDto,
    pub verify: VerifyDto,
    pub jobs: usize,
    pub exclude: String,
    pub durability: DurabilityDto,
}

/// Espejo serializable de `copyforge_core::OverwritePolicy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OverwriteDto {
    Never,
    IfNewer,
    IfDifferent,
    Always,
}

/// Espejo serializable de `copyforge_core::VerifyPolicy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerifyDto {
    None,
    Size,
    Hash,
}

/// Espejo serializable de `copyforge_core::DurabilityPolicy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DurabilityDto {
    None,
    File,
    Full,
}

impl From<OverwriteDto> for OverwritePolicy {
    fn from(o: OverwriteDto) -> Self {
        match o {
            OverwriteDto::Never => Self::Never,
            OverwriteDto::IfNewer => Self::IfNewer,
            OverwriteDto::IfDifferent => Self::IfDifferent,
            OverwriteDto::Always => Self::Always,
        }
    }
}

impl From<OverwritePolicy> for OverwriteDto {
    fn from(o: OverwritePolicy) -> Self {
        match o {
            OverwritePolicy::Never => Self::Never,
            OverwritePolicy::IfNewer => Self::IfNewer,
            OverwritePolicy::IfDifferent => Self::IfDifferent,
            OverwritePolicy::Always => Self::Always,
        }
    }
}

impl From<VerifyDto> for VerifyPolicy {
    fn from(v: VerifyDto) -> Self {
        match v {
            VerifyDto::None => Self::None,
            VerifyDto::Size => Self::Size,
            VerifyDto::Hash => Self::Hash,
        }
    }
}

impl From<VerifyPolicy> for VerifyDto {
    fn from(v: VerifyPolicy) -> Self {
        match v {
            VerifyPolicy::None => Self::None,
            VerifyPolicy::Size => Self::Size,
            VerifyPolicy::Hash => Self::Hash,
        }
    }
}

impl From<DurabilityDto> for DurabilityPolicy {
    fn from(d: DurabilityDto) -> Self {
        match d {
            DurabilityDto::None => Self::None,
            DurabilityDto::File => Self::File,
            DurabilityDto::Full => Self::Full,
        }
    }
}

impl From<DurabilityPolicy> for DurabilityDto {
    fn from(d: DurabilityPolicy) -> Self {
        match d {
            DurabilityPolicy::None => Self::None,
            DurabilityPolicy::File => Self::File,
            DurabilityPolicy::Full => Self::Full,
        }
    }
}

/// Convierte la línea de `exclude` (`"*.log, target/**"`) en globs sueltos.
fn parse_exclude(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

impl From<GuiOptions> for CopyOptions {
    fn from(g: GuiOptions) -> Self {
        Self {
            preserve_metadata: g.preserve_metadata,
            overwrite: g.overwrite.into(),
            verify: g.verify.into(),
            parallel_files: g.jobs,
            exclusion: parse_exclude(&g.exclude),
            durability: g.durability.into(),
            // El resto de `CopyOptions` (follow_symlinks, buffer_size, etc.) no
            // es editable en la UI todavía: se toman los defaults del motor.
            ..CopyOptions::default()
        }
    }
}

impl From<CopyOptions> for GuiOptions {
    fn from(c: CopyOptions) -> Self {
        Self {
            preserve_metadata: c.preserve_metadata,
            overwrite: c.overwrite.into(),
            verify: c.verify.into(),
            jobs: c.parallel_files,
            exclude: c.exclusion.join(", "),
            durability: c.durability.into(),
        }
    }
}

impl Default for GuiOptions {
    /// Defaults derivados de `CopyOptions::default()` para no duplicar valores.
    fn default() -> Self {
        CopyOptions::default().into()
    }
}

/// Devuelve las opciones actuales (defaults del motor en P4.2).
#[tauri::command]
fn get_options() -> GuiOptions {
    let opts = GuiOptions::default();
    tracing::info!(?opts, "get_options");
    opts
}

/// Recibe las opciones editadas en la UI. En P4.2 solo las registra.
#[tauri::command]
fn set_options(options: GuiOptions) {
    let mapped: CopyOptions = options.into();
    tracing::info!(?mapped, "set_options");
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Sin subscriber, `tracing::info!` sería un no-op. Esto hace visibles los
    // logs en la consola donde corre `tauri dev`.
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .try_init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![get_options, set_options])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_gui_options_maps_to_core_defaults() {
        let mapped: CopyOptions = GuiOptions::default().into();
        let core = CopyOptions::default();

        assert_eq!(mapped.preserve_metadata, core.preserve_metadata);
        assert_eq!(mapped.overwrite, core.overwrite);
        assert_eq!(mapped.verify, core.verify);
        assert_eq!(mapped.parallel_files, core.parallel_files);
        assert_eq!(mapped.exclusion, core.exclusion);
        assert_eq!(mapped.durability, core.durability);
    }

    #[test]
    fn exclude_line_splits_into_globs() {
        let mapped: CopyOptions = GuiOptions {
            exclude: String::from("*.log, target/**,  ,ok.txt "),
            ..GuiOptions::default()
        }
        .into();
        assert_eq!(mapped.exclusion, vec!["*.log", "target/**", "ok.txt"]);
    }

    #[test]
    fn gui_options_round_trip_through_core() {
        let gui = GuiOptions {
            preserve_metadata: false,
            overwrite: OverwriteDto::IfNewer,
            verify: VerifyDto::Hash,
            jobs: 7,
            exclude: String::from("*.tmp"),
            durability: DurabilityDto::Full,
        };
        let back = GuiOptions::from(CopyOptions::from(gui.clone()));

        assert_eq!(back.preserve_metadata, gui.preserve_metadata);
        assert_eq!(back.overwrite, gui.overwrite);
        assert_eq!(back.verify, gui.verify);
        assert_eq!(back.jobs, gui.jobs);
        assert_eq!(back.exclude, gui.exclude);
        assert_eq!(back.durability, gui.durability);
    }

    #[test]
    fn dto_serializes_to_plain_variant_names() {
        // El frontend compara contra literales "Always"/"Size"/"File".
        let json = serde_json::to_string(&GuiOptions::default()).expect("serializa");
        assert!(json.contains("\"Always\""), "json: {json}");
        assert!(json.contains("\"Size\""), "json: {json}");
        assert!(json.contains("\"File\""), "json: {json}");
    }

    #[test]
    fn deserializes_payload_sent_by_frontend() {
        // Mismo formato que envía `invoke("set_options", { options })`.
        let payload = r#"{"preserve_metadata":false,"overwrite":"IfDifferent",
            "verify":"Hash","jobs":8,"exclude":"*.log, target/**",
            "durability":"Full"}"#;
        let gui: GuiOptions =
            serde_json::from_str(payload).expect("deserializa el payload del frontend");
        let mapped: CopyOptions = gui.into();

        assert!(!mapped.preserve_metadata);
        assert_eq!(mapped.overwrite, OverwritePolicy::IfDifferent);
        assert_eq!(mapped.verify, VerifyPolicy::Hash);
        assert_eq!(mapped.parallel_files, 8);
        assert_eq!(mapped.exclusion, vec!["*.log", "target/**"]);
        assert_eq!(mapped.durability, DurabilityPolicy::Full);
    }
}
