use std::sync::Arc;

use anyhow::{Context, Result};
use copyforge_core::{build_plan, CopyEngine, CopyEvent};
use tokio::sync::mpsc;

use crate::{args::Args, ui::ProgressUi};

pub async fn run(args: Args) -> Result<()> {
    let opts = args.to_options();

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
    let report = engine.run(plan).await;
    // Fix: libera tx para cerrar forwarder
    drop(engine);

    // 5) Cierre ordenado
    // Fix 4: sin eprintln DEBUG
    fwd.await.ok();
    ui_thread.join().ok();

    if report.files_failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}