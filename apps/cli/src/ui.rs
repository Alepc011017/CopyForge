use std::path::Path;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use copyforge_core::{CopyEvent, Report};

pub struct ProgressUi {
    multi: MultiProgress,
    overall: ProgressBar,
    current: ProgressBar,
    quiet: bool,
    // Fix warn: quitados started/active sin uso
}

impl ProgressUi {
    pub fn new(quiet: bool, parallel: usize) -> Self {
        let multi = MultiProgress::new();

        let overall = multi.add(ProgressBar::new(0));
        overall.set_style(
            ProgressStyle::with_template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, ETA {eta}) {msg}",
            )
            .unwrap()
            .progress_chars("=>-"),
        );
        // overall.enable_steady_tick(Duration::from_millis(120));

        let current = if parallel == 1 && !quiet {
            multi.add(ProgressBar::new(0))
        } else {
            ProgressBar::hidden()
        };
        current.set_style(
            ProgressStyle::with_template("  {spinner:.dim} [{bar:30.green/black}] {bytes}/{total_bytes} {msg}")
                .unwrap()
                .progress_chars("=>-"),
        );

        Self {
            multi,
            overall,
            current,
            quiet,
        }
    }

    pub fn on_event(&mut self, ev: CopyEvent) {
        if self.quiet {
            // solo procesamos el Finished para imprimir resultado
            if let CopyEvent::Finished { report } = ev {
                print_report(&report);
            }
            return;
        }

        match ev {
            CopyEvent::Planned { total_files, total_bytes } => {
                self.overall.set_length(total_bytes);
                self.overall.set_message(format!("{total_files} archivos"));
            }
            CopyEvent::FileStarted { path, size } => {
                if self.current.is_hidden() {
                    // modo paralelo: no hay barra individual, solo actualizamos overall
                } else {
                    self.current.set_length(size);
                    self.current.set_position(0);
                    self.current.set_message(short_path(&path));
                }
            }
            CopyEvent::Progress { bytes_copied, .. } => {
                // Nota: este evento es por-archivo. En modo single-file lo usamos
                // para la barra individual y para avanzar overall incrementalmente.
                if !self.current.is_hidden() {
                    self.current.set_position(bytes_copied);
                }
            }
            CopyEvent::FileCompleted { .. } => {
                // El overall se actualiza con los bytes acumulados.
                // Necesitamos derivar el delta del último progreso. Como no
                // lo llevamos aquí, sumamos el tamaño desde Progress acumulando.
                // Simplificación: actualizamos overall con incremento por eventos
                // (ver runner.rs donde manejamos esto con contadores).
            }
            CopyEvent::FileSkipped { path, .. } => {
                // Fix 2: skip no avanza overall
                let _ = path;
            }
            CopyEvent::FileFailed { path, error } => {
                self.multi.suspend(|| {
                    eprintln!("✗ {}: {}", path.display(), error);
                });
            }
            CopyEvent::Symlinked { .. } => {
                // Los enlaces no pesan: no avanzan el overall.
            }
            CopyEvent::Finished { report } => {
                self.overall.finish_and_clear();
                if !self.current.is_hidden() {
                    self.current.finish_and_clear();
                }
                print_report(&report);
            }
        }
    }

    pub fn finish(&mut self) {
        self.overall.disable_steady_tick();
        self.overall.finish_and_clear();
        if !self.current.is_hidden() {
            self.current.finish_and_clear();
        }
    }

    pub fn inc_overall(&mut self, delta: u64) {
        self.overall.inc(delta);
    }
}

fn short_path(p: &Path) -> String {
    let s = p.to_string_lossy();
    if s.len() <= 50 { return s.into(); }
    let tail = &s[s.len() - 47..];
    format!("…{tail}")
}

fn print_report(r: &Report) {
    use humansize::{format_size, BINARY};
    use humantime::format_duration;

    println!();
    println!("── CopyForge ─────────────────────────────");
    println!("  Archivos copiados : {}", r.files_copied);
    println!("  Dirs creados      : {}", r.dirs_created); // Fix 2: muestra dirs
    println!("  Enlaces creados   : {}", r.symlinks_created);
    println!("  Archivos omitidos : {}", r.files_skipped);
    println!("  Archivos fallidos : {}", r.files_failed);
    println!("  Bytes copiados    : {}", format_size(r.bytes_copied, BINARY));
    println!("  Duración          : {}", format_duration(r.duration));
    if !r.errors.is_empty() {
        println!("  Errores:");
        for (p, e) in r.errors.iter().take(10) {
            println!("    - {}: {}", p.display(), e);
        }
        if r.errors.len() > 10 {
            println!("    … y {} más", r.errors.len() - 10);
        }
    }
    if r.errors_truncated > 0 {
        println!("  {} errores adicionales omitidos (total: {})",
                 r.errors_truncated, r.errors.len() + r.errors_truncated);
    }
    println!("──────────────────────────────────────────");
}