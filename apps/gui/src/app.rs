use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::thread::JoinHandle;

use copyforge_core::{build_plan, validate_paths, CopyEngine, CopyEvent, CopyOptions, Report};

// Estado de copia en curso
struct CopyState {
    rx: Receiver<CopyEvent>,
    handle: Option<JoinHandle<()>>,
    total_files: usize,
    total_bytes: u64,
    copied_bytes: u64,
    per_file: HashMap<PathBuf, u64>,
    current: String,
    skipped: u64,
    failed: u64,
    errors: Vec<String>,
    report: Option<Report>,
}

pub struct CopyForgeApp {
    source: String,
    dest: String,
    status: String,
    state: Option<CopyState>,
    // Fix dnd: diagnóstico visible
    dnd_debug: String,
    // P0.9: opt-in para seguir symlinks fuera de src.
    allow_escape: bool,
}

impl Default for CopyForgeApp {
    fn default() -> Self {
        Self {
            source: String::new(),
            dest: String::new(),
            status: String::from("Listo"),
            state: None,
            dnd_debug: String::from("DnD: sin eventos"),
            allow_escape: false,
        }
    }
}

impl CopyForgeApp {
    // Fix gui: limpia comillas y file:// al pegar
    fn clean_path(s: &str) -> String {
        let t = s.trim().trim_matches('"').trim_matches('\'').trim();
        let t = t.strip_prefix("file://").unwrap_or(t);
        t.trim().to_owned()
    }

    // Arranca copia en hilo aparte
    fn start_copy(&mut self) {
        self.source = Self::clean_path(&self.source.clone());
        self.dest = Self::clean_path(&self.dest.clone());
        let src = PathBuf::from(self.source.trim());
        let dst = PathBuf::from(self.dest.trim());
        // P0.5: validación temprana con mensaje claro, antes del engine.
        if let Err(e) = validate_paths(&src, &dst) {
            self.status = format!("Rutas inválidas: {e}");
            return;
        }
        let opts = CopyOptions {
            allow_symlink_escape: self.allow_escape,
            ..CopyOptions::default()
        };

        let plan = match build_plan(&src, &dst, &opts) {
            Ok(p) => p,
            Err(e) => {
                self.status = format!("Error plan: {e}");
                return;
            }
        };

        let (sync_tx, sync_rx) = std::sync::mpsc::channel::<CopyEvent>();
        let handle = std::thread::spawn(move || {
            // Runtime solo para el engine
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("rt");
            rt.block_on(async move {
                let (tx, mut rx) = tokio::sync::mpsc::channel::<CopyEvent>(2048);
                let engine = CopyEngine::new(opts, tx);
                let fwd_tx = sync_tx.clone();
                // Reenvía eventos async -> sync
                let fwd = tokio::spawn(async move {
                    while let Some(ev) = rx.recv().await {
                        if fwd_tx.send(ev).is_err() {
                            break;
                        }
                    }
                });
                let _report = engine.run(plan).await;
                drop(engine);
                let _ = fwd.await;
            });
        });

        self.state = Some(CopyState {
            rx: sync_rx,
            handle: Some(handle),
            total_files: 0,
            total_bytes: 1, // evita /0
            copied_bytes: 0,
            per_file: HashMap::new(),
            current: String::new(),
            skipped: 0,
            failed: 0,
            errors: Vec::new(),
            report: None,
        });
        self.status = String::from("Copiando...");
    }

    // Consume eventos sin bloquear
    fn poll_events(&mut self) {
        let done = if let Some(st) = self.state.as_mut() {
            loop {
                match st.rx.try_recv() {
                    Ok(ev) => match ev {
                        CopyEvent::Planned { total_files, total_bytes } => {
                            st.total_files = total_files;
                            st.total_bytes = total_bytes.max(1);
                        }
                        CopyEvent::FileStarted { path, .. } => {
                            st.current = path.to_string_lossy().into_owned();
                        }
                        CopyEvent::Progress { path, bytes_copied, .. } => {
                            // Suma solo el delta por archivo
                            let prev = st.per_file.insert(path, bytes_copied).unwrap_or(0);
                            st.copied_bytes += bytes_copied.saturating_sub(prev);
                        }
                        CopyEvent::FileSkipped { .. } => st.skipped += 1,
                        CopyEvent::FileFailed { path, error } => {
                            st.failed += 1;
                            st.errors.push(format!("{}: {}", path.display(), error));
                        }
                        CopyEvent::FileCompleted { .. } => {}
                        CopyEvent::Symlinked { .. } => {}
                        CopyEvent::Finished { report } => {
                            st.copied_bytes = st.total_bytes.min(
                                st.copied_bytes.max(report.bytes_copied),
                            );
                            // Usa totales del reporte
                            st.report = Some(report);
                        }
                    },
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break,
                }
            }
            // Terminó si hay reporte y el hilo acabó
            if st.report.is_some() {
                if let Some(h) = st.handle.take() {
                    let _ = h.join();
                }
                true
            } else {
                false
            }
        } else {
            false
        };

        if done {
            if let Some(st) = self.state.as_ref() {
                if let Some(r) = st.report.as_ref() {
                    self.status = format!(
                        "Fin: {} ok, {} omit, {} fallos",
                        r.files_copied, r.files_skipped, r.files_failed
                    );
                }
            }
        }
    }

    fn copying(&self) -> bool {
        self.state.as_ref().is_some_and(|s| s.report.is_none())
    }

    // Fix dnd: suelta archivos en ventana
    fn handle_drag_drop(&mut self, ctx: &egui::Context) {
        // Diagnóstico: cuenta hover + drop (con y sin ruta)
        let (n_hover, n_drop, n_path) = ctx.input(|i| {
            (
                i.raw.hovered_files.len(),
                i.raw.dropped_files.len(),
                i.raw.dropped_files.iter().filter(|f| f.path.is_some()).count(),
            )
        });
        if n_hover > 0 || n_drop > 0 {
            self.dnd_debug = format!("DnD: hover={n_hover} drop={n_drop} con_ruta={n_path}");
        }
        let paths: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if paths.is_empty() {
            // Fix dnd: avisa si llega sin ruta
            if n_drop > 0 && !self.copying() {
                self.status = format!("Suelta recibida pero sin ruta ({n_drop})");
            }
            return;
        }
        if self.copying() {
            return;
        }
        // Fix dnd: 1 -> origen, 2 -> origen+destino
        self.source = paths[0].to_string_lossy().into_owned();
        if paths.len() >= 2 {
            self.dest = paths[1].to_string_lossy().into_owned();
        } else if self.dest.trim().is_empty() {
            // Fix gui: autodestino si falta
            self.dest = format!("{}_copia", paths[0].to_string_lossy());
        }
        self.status = format!("Soltados {} archivo(s)", paths.len());
    }
}

impl eframe::App for CopyForgeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_drag_drop(ctx);
        self.poll_events();
        if self.copying() {
            // Repinta para avance fluido
            ctx.request_repaint();
        }

        // Fix dnd: overlay al arrastrar
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        if hovering {
            egui::Window::new("Soltar archivos")
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Suelta 1 = origen, 2 = origen + destino");
                });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("CopyForge");
            ui.small("Tip: arrastra archivos (X11/Windows) o pega la ruta con Ctrl+V");
            ui.small("En Wayland el arrastre no llega: usa Carpeta.../Archivo... o Ctrl+V");
            ui.small(&self.dnd_debug);
            ui.separator();

            // Origen / destino
            ui.horizontal(|ui| {
                ui.label("Origen:");
                ui.text_edit_singleline(&mut self.source);
                // Fix gui: carpeta o archivo
                if ui.button("Carpeta...").clicked() {
                    if let Some(p) = rfd::FileDialog::new().pick_folder() {
                        self.source = p.to_string_lossy().into_owned();
                    }
                }
                if ui.button("Archivo...").clicked() {
                    if let Some(p) = rfd::FileDialog::new().pick_file() {
                        self.source = p.to_string_lossy().into_owned();
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Destino:");
                ui.text_edit_singleline(&mut self.dest);
                if ui.button("Carpeta...").clicked() {
                    if let Some(p) = rfd::FileDialog::new().pick_folder() {
                        self.dest = p.to_string_lossy().into_owned();
                    }
                }
                if ui.button("Archivo...").clicked() {
                    if let Some(p) = rfd::FileDialog::new().pick_file() {
                        self.dest = p.to_string_lossy().into_owned();
                    }
                }
            });

            ui.add_space(8.0);
            let can_start =
                !self.copying() && !self.source.trim().is_empty() && !self.dest.trim().is_empty();
            // Fix gui: avisa por qué no arranca
            if !self.copying() && !can_start {
                ui.small("Falta origen y/o destino para copiar.");
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(can_start, egui::Button::new("Copiar"))
                    .clicked()
                {
                    self.start_copy();
                }
                // Fix gui: limpiar resetea campos
                if ui.add_enabled(!self.copying(), egui::Button::new("Limpiar")).clicked() {
                    self.source.clear();
                    self.dest.clear();
                    self.state = None;
                    self.status = String::from("Listo");
                }
            });
            ui.add_enabled_ui(!self.copying(), |ui| {
                ui.checkbox(&mut self.allow_escape, "Permitir symlinks fuera de src")
                    .on_hover_text("Si se sigue un enlace fuera del origen, copiar su contenido (peligroso)");
            });

            ui.add_space(8.0);
            ui.label(&self.status);

            // Progreso
            if let Some(st) = self.state.as_ref() {
                let frac = (st.copied_bytes as f32 / st.total_bytes as f32).clamp(0.0, 1.0);
                ui.add(
                    egui::ProgressBar::new(frac)
                        .show_percentage()
                        .text(format!("{}/{} archivos", st.copied_bytes, st.total_bytes)),
                );
                if !st.current.is_empty() {
                    ui.small(format!("Actual: {}", st.current));
                }
                ui.small(format!(
                    "Archivos: {}  Omitidos: {}  Fallidos: {}",
                    st.total_files, st.skipped, st.failed
                ));

                // Reporte final
                if let Some(r) = st.report.as_ref() {
                    ui.separator();
                    ui.label(format!(
                        "Copiados: {}  Dirs: {}  Bytes: {}",
                        r.files_copied, r.dirs_created, r.bytes_copied
                    ));
                    if !r.errors.is_empty() {
                        ui.collapsing("Errores", |ui| {
                            egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                                for (p, e) in r.errors.iter().take(50) {
                                    ui.monospace(format!("{}: {}", p.display(), e));
                                }
                            });
                        });
                    }
                    if ui.button("Nueva copia").clicked() {
                        self.state = None;
                        self.status = String::from("Listo");
                    }
                } else if !st.errors.is_empty() {
                    ui.collapsing("Fallos", |ui| {
                        for e in st.errors.iter().rev().take(10) {
                            ui.monospace(e);
                        }
                    });
                }
            }
        });
    }
}
