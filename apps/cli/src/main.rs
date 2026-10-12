use clap::Parser;
use tracing_subscriber::{EnvFilter, fmt};
use anyhow::Result;

mod args;
mod runner;
mod ui;

use args::Args;
use runner::run;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let args = Args::parse();
    run(args).await
}