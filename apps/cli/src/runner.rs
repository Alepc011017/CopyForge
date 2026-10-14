use std::sync::Arc;

use anyhow::{Context, Result};
use copyforge_core::{build_plan, validate_paths, CopyEngine, CopyEvent, Report};
use tokio::sync::mpsc;

use crate::{args::Args, ui::ProgressUi};

pub async fn run(args: Args) -> Result<()> {
    let opts = args.to_options();

    // 0) Validación temprana (P0.5): src==dst y dst⊂src, antes de cualquier IO.
    validate_paths(&args.source, &args.dest)
        .with_context(|| format!("rutas inválidas: {} -> {}", args.source.display(), args.dest.display()))?;

    // 1) Plan
    let plan = build_plan(&args.source, &args.dest, &opts)
        .with_context(|| format!("construyendo plan desde {}", args.source.display()))?;

    if args.dry_run {
        println!("Dry-run: {} tareas, {} bytes totales",
                 plan.tasks.len(), plan.total_bytes);
        for t in plan.tasks.iter().take(50) {
            println!("  {:?}  {} -> {}",
                     t.kind, t.src.display(), t.dst.display());
        }
        if plan.tasks.len() > 50 {
            println!("  … y {} más", plan.tasks.len() - 50);
        }
        return Ok(());
    }

    // 2) Canal de eventos
    let (tx, mut rx) = mpsc::channel::<CopyEvent>(2048);

    // 3) UI en un hilo bloqueante (indicatif no es async-friendly)
    let ui = Arc::new(std::sync::Mutex::new(ProgressUi::new(args.quiet, opts.parallel_files)));

    // Como indicatif es sync y tokio es async, hacemos el puente con
    // un canal std::sync::mpsc alimentado por un forwarder async.
    let (sync_tx, sync_rx) = std::sync::mpsc::channel::<CopyEvent>();

    // Forwarder: async rx -> sync tx
    let fwd = tokio::spawn(async move {
        while let Some(ev) = rx.recv().await {
            if sync_tx.send(ev).is_err() { break; }
        }
        // Fix 4: sin DEBUG, cierra canal para terminar UI
        drop(sync_tx);
    });

    // Hilo de UI
    let ui_thread = std::thread::spawn(move || {
        let mut ui = ui.lock().unwrap();
        let mut per_file_last: std::collections::HashMap<std::path::PathBuf, u64> = Default::default();

        for ev in sync_rx {
            // Actualizamos el overall con el delta de cada Progress
            if let CopyEvent::Progress { ref path, bytes_copied, .. } = ev {
                let prev = per_file_last.insert(path.clone(), bytes_copied).unwrap_or(0);
                let delta = bytes_copied.saturating_sub(prev);
                if delta > 0 {
                    ui.inc_overall(delta);
                }
            }
            ui.on_event(ev);
        }
        ui.finish();
    });

    // 4) Engine
    let engine = CopyEngine::new(opts, tx);
    // P2.1: Ctrl-C → cancelación cooperativa del engine (responde en el
    // gate de spawn, al inicio de cada tarea y en cada iteración del bucle).
    // Los temporales se limpian solos por Drop (P1.4): al abortar, cada tarea
    // dropea su NamedTempFile y no quedan .copyforge-tmp-* pendientes.
    let cancel_token = engine.cancel_token();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            cancel_token.cancel();
        }
    });
    let report = engine.run(plan).await;
    // Fix: libera tx para cerrar forwarder
    drop(engine);

    // 5) Cierre ordenado
    // Fix 4: sin eprintln DEBUG
    fwd.await.ok();
    ui_thread.join().ok();

    if report.files_failed > 0 {
        std::process::exit(exit_code_for(&report));
    }
    Ok(())
}

/// Código de salida del proceso según el reporte (P2.1).
/// Las cancelaciones cuentan como fallo: el engine las reporta en
/// `files_failed` (variante `CopyError::Cancelled`).
///
/// Semántica exacta de exit codes:
/// - `0`: éxito (o pre-cancelado sin trabajo: el engine cortocircuita el
///   spawn y el reporte queda vacío, indistinguible del éxito).
/// - `1`: fallos en la copia, incluida cancelación a mitad de una tarea.
///
/// Candidato (sin implementar): código `130` para SIGINT puro, estándar
/// Unix que algunos scripts detectan. Requeriría distinguir en `Report`
/// "cancelado antes de empezar" de "éxito vacío" (hoy indistinguibles),
/// lo que implica cambio en `crates/core`. Ver PLAN.md.
pub fn exit_code_for(report: &Report) -> i32 {
    if report.files_failed > 0 { 1 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use copyforge_core::{CopyEngine, CopyOptions};

    fn test_dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "copyforge-cli-test-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    // P2.1: token cancelado ANTES de run (sin sleeps) → no se copia nada.
    // Nota honesta: el pre-cancelado cortocircuita el spawn y el reporte
    // queda vacío (exit 0); la cancelación a mitad de copia sí genera
    // Failed(Cancelled) → exit 1 (verificado manualmente con Ctrl-C).
    #[cfg(unix)]
    #[tokio::test]
    async fn cancelled_token_copies_nothing() {
        let dir = test_dir("cancel");
        let src = dir.join("a.txt");
        std::fs::write(&src, b"data").unwrap();
        let dst = dir.join("b.txt");
        let opts = CopyOptions::default();
        let plan = copyforge_core::build_plan(&src, &dst, &opts).unwrap();
        let (tx, _rx) = tokio::sync::mpsc::channel(64);
        let engine = CopyEngine::new(opts, tx);
        engine.cancel_token().cancel();
        let report = engine.run(plan).await;
        assert_eq!(report.files_copied, 0);
        assert!(!dst.exists());
    }

    #[test]
    fn exit_code_maps_failures() {
        let ok = Report::default();
        assert_eq!(exit_code_for(&ok), 0);
        let failed = Report { files_failed: 2, ..Report::default() };
        assert_eq!(exit_code_for(&failed), 1);
    }
}