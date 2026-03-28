//! CLI entry point for kron — a modern cron replacement with built-in observability.

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::EnvFilter;

mod commands;

#[derive(Parser)]
#[command(
    name = "kron",
    version,
    about = "Cron, but it actually tells you what happened"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: commands::Command,

    /// Increase verbosity (-v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Respect RUST_LOG env var; fall back to verbosity flag
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(match cli.verbose {
            0 => "warn",
            1 => "info",
            2 => "debug",
            _ => "trace",
        })
    });
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    commands::run(cli.command).await
}
