//! Backend Tauri de CopyForge (P4.2 + P4.3).
//!
//! Expone al frontend:
//! - `get_options` / `set_options`: [`GuiOptions`] actúa de DTO en la frontera con
//!   `copyforge_core::CopyOptions` (los tipos del motor no tienen serde y
//!   `crates/core` no se toca).
//! - `adjust_window_size`: fallback del redimensionado desde Rust.
//! - `start_copy` / `cancel_copy`: arranque y cancelación de una copia real con
//!   `copyforge_core::CopyEngine`, retransmitiendo sus `CopyEvent` al frontend
//!   como eventos `copy-event` ([`CopyEventDto`]).
//!
//! Diseño: la GUI monitoriza una cola de tareas (la pestaña «Lista de copia»),
//! no una ventana por copia; por eso el estado de la copia en curso vive en
//! [`CopyState`] en vez de crear una ventana nueva por operación.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use copyforge_core::{
    build_plan, CopyEngine, CopyEvent, CopyOptions, DurabilityPolicy, OverwritePolicy,
    VerifyPolicy,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tauri::{Emitter, State};

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

/// Ruta en texto (lossy: las rutas de Windows/Unix que no son UTF-8 no son
/// representables; la UI no las necesita ver en crudo).
fn path_str(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Versión serializable de `copyforge_core::CopyEvent` para la frontera con React.
///
/// `CopyEvent` (y su `Report`) no implementan `Serialize` y `crates/core` no se
/// toca, así que se mapea aquí, igual que `GuiOptions` con `CopyOptions`. El
/// discriminador va en la clave `type` (`planned`, `fileStarted`, `progress`, …)
/// para que el frontend haga `switch` sobre él.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum CopyEventDto {
    Planned { total_files: usize, total_bytes: u64 },
    FileStarted { path: String, size: u64 },
    Progress { path: String, bytes_copied: u64, total: u64 },
    FileCompleted { path: String, elapsed_ms: u64 },
    FileSkipped { path: String, reason: String },
    FileFailed { path: String, error: String },
    Symlinked { src: String, dst: String },
    SymlinkFallback { path: String, target: String, reason: String },
    Finished {
        files_copied: u64,
        files_skipped: u64,
        files_failed: u64,
        bytes_copied_total: u64,
        dirs_created: u64,
        symlinks_created: u64,
        duration_ms: u64,
        errors: Vec<String>,
        errors_truncated: usize,
        symlinks_fallback: Vec<(String, String)>,
    },
}

impl From<CopyEvent> for CopyEventDto {
    fn from(event: CopyEvent) -> Self {
        match event {
            CopyEvent::Planned { total_files, total_bytes } => {
                Self::Planned { total_files, total_bytes }
            }
            CopyEvent::FileStarted { path, size } => {
                Self::FileStarted { path: path_str(&path), size }
            }
            CopyEvent::Progress { path, bytes_copied, total } => Self::Progress {
                path: path_str(&path),
                bytes_copied,
                total,
            },
            CopyEvent::FileCompleted { path, elapsed } => Self::FileCompleted {
                path: path_str(&path),
                elapsed_ms: elapsed.as_millis() as u64,
            },
            CopyEvent::FileSkipped { path, reason } => Self::FileSkipped {
                path: path_str(&path),
                reason: format!("{reason:?}"),
            },
            CopyEvent::FileFailed { path, error } => {
                Self::FileFailed { path: path_str(&path), error }
            }
            CopyEvent::Symlinked { src, dst } => {
                Self::Symlinked { src: path_str(&src), dst: path_str(&dst) }
            }
            CopyEvent::SymlinkFallback { path, target, reason } => Self::SymlinkFallback {
                path: path_str(&path),
                target: path_str(&target),
                reason,
            },
            CopyEvent::Finished { report } => Self::Finished {
                files_copied: report.files_copied,
                files_skipped: report.files_skipped,
                files_failed: report.files_failed,
                bytes_copied_total: report.bytes_copied,
                dirs_created: report.dirs_created,
                symlinks_created: report.symlinks_created,
                duration_ms: report.duration.as_millis() as u64,
                errors: report
                    .errors
                    .iter()
                    .map(|(path, error)| format!("{}: {error}", path_str(path)))
                    .collect(),
                errors_truncated: report.errors_truncated,
                symlinks_fallback: report
                    .symlinks_fallback
                    .iter()
                    .map(|(src, dst)| (path_str(src), path_str(dst)))
                    .collect(),
            },
        }
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

/// Estado de la copia en curso (P4.3).
///
/// El motor crea su propio `CancellationToken` al instanciarse; aquí se guarda
/// un clon para que `cancel_copy` pueda frenar la ejecución. `cancel_copy` es
/// síncrono, de ahí el `Mutex`: se toma y se suelta sin cruzar ningún `await`.
///
/// `cancel` es un `Arc` para que la tarea del motor tenga su propio propietario
/// del token: el guard [`ReleaseToken`] vive dentro de un `tokio::spawn`, que exige
/// `'static`, y `State<'_, T>` presta por el comando y no es `Clone` en Tauri 2.
#[derive(Default)]
struct CopyState {
    cancel: Arc<Mutex<Option<CancellationToken>>>,
    /// P4.4: flag de pausa/reanudar compartido con el engine (`with_pause`).
    paused: Mutex<Arc<AtomicBool>>,
}

// `Mutex` no implementa `Clone`, así que el estado se reconstruye a mano con el
// mismo `Arc` (sin derivar).
impl Clone for CopyState {
    fn clone(&self) -> Self {
        Self {
            cancel: self.cancel.clone(),
            paused: Mutex::new(
                self.paused
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone(),
            ),
        }
    }
}

/// Libera el slot de cancelación al salir de su ámbito.
///
/// Vive dentro de la tarea del motor: si `engine.run` paniquea, el unwinding
/// corre `Drop` y el slot queda libre. Sin él, un panic dejaría `Some` para
/// siempre y `cancel_copy` y un segundo `start_copy` quedarían bloqueados el resto
/// de la sesión.
///
/// Nota: el perfil release usa `panic = "abort"`, así que el guard solo protege
/// en debug (y en builds sin abort). Ver PLAN.md.
struct ReleaseToken {
    cancel: Arc<Mutex<Option<CancellationToken>>>,
}

impl Drop for ReleaseToken {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.cancel.lock() {
            *slot = None;
        }
    }
}

/// Lanza una copia real con el motor de `copyforge_core`.
///
/// Se valida y planifica antes de arrancar, así que un error de rutas (origen
/// inexistente, destino dentro del origen, …) vuelve como error del comando en
/// vez de perderse en los eventos. El motor corre en una tarea aparte y sus
/// `CopyEvent` se reenvían al frontend por el canal `copy-event`.
#[tauri::command]
async fn start_copy(
    app: tauri::AppHandle,
    state: State<'_, CopyState>,
    src: String,
    dst: String,
    options: GuiOptions,
) -> Result<(), String> {
    let src_path = std::path::PathBuf::from(&src);
    let dst_path = std::path::PathBuf::from(&dst);
    let core_options: CopyOptions = options.into();

    let plan = build_plan(&src_path, &dst_path, &core_options).map_err(|e| e.to_string())?;
    tracing::info!(
        src = %src,
        dst = %dst,
        tasks = plan.tasks.len(),
        total_bytes = plan.total_bytes,
        "start_copy"
    );

    let (tx, mut rx) = mpsc::channel::<CopyEvent>(256);

    // (1) Rechazo si ya hay copia en curso: si no, un segundo arranque sustituye
    //     el token y la primera queda huérfana e in-cancellable.
    {
        let slot = state.cancel.lock().map_err(|e| e.to_string())?;
        if slot.is_some() {
            return Err("ya hay una copia en curso".to_string());
        }
    }

    // (2) Flag de pausa (P4.4): se crea y se guarda aquí, en la rama de aceptación,
    //     y se comparte con el engine. `pause_copy` lo consulta desde `CopyState`.
    let paused = {
        let mut slot = state.paused.lock().map_err(|e| e.to_string())?;
        let flag = Arc::new(AtomicBool::new(false));
        *slot = flag.clone();
        flag
    };

    let engine = CopyEngine::with_pause(core_options, tx, paused);

    {
        let mut slot = state.cancel.lock().map_err(|e| e.to_string())?;
        *slot = Some(engine.cancel_token());
    }

    // (3) El reenviador avisa cuando ha drenado el canal (incluido `Finished`).
    let (drained_tx, drained_rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let Err(e) = app.emit("copy-event", CopyEventDto::from(event)) {
                tracing::warn!("no se pudo emitir `copy-event`: {e}");
                break;
            }
        }
        let _ = drained_tx.send(());
    });

    // (4) El token se libera SOLO cuando el último evento ha sido emitido. El
    //     guard vive aquí dentro y libera el slot al terminar el bloque; el
    //     `Arc` propio satisface el `'static` del spawn.
    let cancel = state.cancel.clone();
    tokio::spawn(async move {
        let _release = ReleaseToken { cancel };

        let report = engine.run(plan).await; // panic ⇒ el guard limpia en el unwinding
        tracing::info!(?report, "start_copy terminado");
        // Soltar `events_tx` ANTES de esperar el drenaje: si no, el reenviador
        // nunca ve el canal cerrado y el spawn se queda esperando un drenaje que
        // no puede llegar (espera circular).
        drop(engine);

        // Si el reenviador rompió por error de emit, el oneshot llega cerrado.
        let _ = drained_rx.await;
    });

    Ok(())
}

/// Pausa/reanuda la copia en curso (P4.4).
///
/// El flag vive en `CopyState.paused` y es el mismo `Arc<AtomicBool>` que consulta
/// el motor (vía `CopyEngine::with_pause`). Sobre una copia inexistente devuelve
/// `Err`; comprobar `cancel_copy` en `pause_copy` queda como mejora futura.
#[tauri::command]
fn pause_copy(state: State<'_, CopyState>, paused: bool) -> Result<(), String> {
    let slot = state.paused.lock().map_err(|e| e.to_string())?;
    slot.store(paused, Ordering::Relaxed);
    tracing::info!("pause_copy paused={paused}");
    Ok(())
}

/// Cancela la copia en curso (P4.4).
///
/// Emite `copy-cancelled` para que el frontend sepa que el **próximo** `finished`
/// corresponde a esta cancelación (no al anterior). El motor después envía su
/// `finished` habitual, con los ficheros cancelados contados en `files_failed`.
#[tauri::command]
fn cancel_copy(app: tauri::AppHandle, state: State<'_, CopyState>) -> Result<(), String> {
    // Clonar dentro del lock y operar fuera: no se emite con el guard del Mutex
    // cogido (emit es lento y otro comando podría quedar bloqueado).
    let token = {
        let guard = state.cancel.lock().map_err(|e| e.to_string())?;
        guard.as_ref().cloned()
    };
    match token {
        Some(token) => {
            token.cancel();
            tracing::info!("cancel_copy: token de cancelación disparado");
            if let Err(e) = app.emit("copy-cancelled", ()) {
                tracing::warn!("no se pudo emitir `copy-cancelled`: {e}");
            }
            Ok(())
        }
        None => Err("no hay ninguna copia en curso".to_string()),
    }
}

/// Altura mínima lógica de la ventana (coincide con la base de `tauri.conf.json`
/// y con `BASE_WINDOW_HEIGHT` del frontend): al cerrar el panel de detalles no
/// debe quedar colapsada.
const MIN_WINDOW_HEIGHT: f64 = 250.0;

/// Altura destino al abrir (`expanded = true`) o cerrar el panel de detalles.
///
/// `delta_height` es la altura que ocupa el panel en px lógicos; se usa su valor
/// absoluto porque el signo lo decide `expanded`.
fn next_window_height(current: f64, expanded: bool, delta_height: f64) -> f64 {
    let delta = delta_height.abs();
    let target = if expanded { current + delta } else { current - delta };
    target.max(MIN_WINDOW_HEIGHT)
}

/// Suma (o resta) la altura del panel de detalles a la ventana.
///
/// Es el *fallback* de `Window.setSize()` del frontend: hay versiones de Tauri 2
/// en las que el `setSize` lanzado desde el webview no surte efecto cuando la
/// ventana va sin decoración del sistema (`decorations: false`). La UI intenta
/// primero el camino JS y, si falla, invoca este comando.
#[tauri::command]
fn adjust_window_size(
    window: tauri::Window,
    expanded: bool,
    delta_height: f64,
) -> Result<(), String> {
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let current = window
        .inner_size()
        .map_err(|e| e.to_string())?
        .to_logical::<f64>(scale);
    let height = next_window_height(current.height, expanded, delta_height);

    window
        .set_size(tauri::LogicalSize::new(current.width, height))
        .map_err(|e| e.to_string())?;

    tracing::info!(
        expanded,
        delta_height,
        logical_height = height,
        "adjust_window_size"
    );
    Ok(())
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
        .invoke_handler(tauri::generate_handler![
            get_options,
            set_options,
            adjust_window_size,
            start_copy,
            cancel_copy,
            pause_copy
        ])
        .manage(CopyState::default())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;
    use copyforge_core::Report;
    use std::path::PathBuf;
    use std::time::Duration;

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
    fn finished_dto_keeps_every_report_field() {
        // Un Report con todos los campos no-default: el DTO no debe perder ninguno.
        let mut report = Report::default();
        report.files_copied = 3;
        report.files_skipped = 1;
        report.files_failed = 2;
        report.bytes_copied = 7;
        report.dirs_created = 4;
        report.symlinks_created = 5;
        report.duration = Duration::from_millis(1234);
        report.errors.push((PathBuf::from("/x/boom.txt"), "e".into()));
        report.errors_truncated = 998;
        report.symlinks_fallback.push((PathBuf::from("/a"), PathBuf::from("/b")));

        let CopyEventDto::Finished {
            errors_truncated,
            symlinks_fallback,
            files_failed,
            duration_ms,
            ..
        } = CopyEventDto::from(CopyEvent::Finished { report })
        else {
            panic!("esperaba Finished");
        };

        assert_eq!(errors_truncated, 998);
        assert_eq!(symlinks_fallback, vec![("/a".to_string(), "/b".to_string())]);
        assert_eq!(files_failed, 2);
        assert_eq!(duration_ms, 1234);
    }

    #[test]
    fn finished_dto_round_trips_through_json() {
        // El frontend discrimina por `type` y lee campos en snake_case.
        let mut report = Report::default();
        report.files_copied = 1;
        report.errors_truncated = 4;
        report.symlinks_fallback.push((PathBuf::from("/s"), PathBuf::from("/d")));

        let dto = CopyEventDto::from(CopyEvent::Finished { report });
        let json = serde_json::to_string(&dto).expect("serializa");
        assert!(json.contains("\"type\":\"finished\""), "json: {json}");
        assert!(json.contains("\"errors_truncated\":4"), "json: {json}");
        let back: CopyEventDto = serde_json::from_str(&json).expect("deserializa");
        let CopyEventDto::Finished { errors_truncated, .. } = back else {
            panic!("esperaba Finished");
        };
        assert_eq!(errors_truncated, 4);
    }

    #[test]
    fn window_height_follows_the_panel() {
        assert_eq!(next_window_height(600.0, true, 121.0), 721.0);
        assert_eq!(next_window_height(721.0, false, 121.0), 600.0);
    }

    #[test]
    fn window_height_never_collapses_below_the_minimum() {
        // El signo lo decide `expanded`, no el signo de `delta_height`.
        assert_eq!(next_window_height(300.0, false, 500.0), MIN_WINDOW_HEIGHT);
        assert_eq!(next_window_height(300.0, true, -50.0), 350.0);
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
