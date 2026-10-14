use clap::Parser;
use tracing_subscriber::{EnvFilter, fmt};
use anyhow::Result;

mod args;
mod runner;
mod ui;

use args::Args;
use runner::run;

/// Punto de entrada del CLI.
///
/// Semántica de exit codes (detalle en `runner::exit_code_for`):
/// - `0`: éxito (o pre-cancelado sin trabajo).
/// - `1`: fallos en la copia, incluida cancelación a mitad de una tarea.
/// - Nota: `130` (SIGINT estándar Unix) es candidato a evaluar en P2 o
///   fuera de scope; por ahora un Ctrl-C efectivo sale con `1`.
#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // P2.1: el filtro se decide ANTES de iniciar tracing.
    let filter = args::log_filter_from(args.verbose, std::env::var("RUST_LOG").ok());
    fmt()
        .with_env_filter(EnvFilter::try_new(&filter).unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    run(args).await
}