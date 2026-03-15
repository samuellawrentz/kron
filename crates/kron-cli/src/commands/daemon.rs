use anyhow::{Context, Result};
use tokio_util::sync::CancellationToken;
use tracing::info;

use kron_core::{config, scheduler::Scheduler};
use kron_store::Store;

pub async fn execute() -> Result<()> {
    // Store is opened for run history only — job definitions come from TOML
    let store = Store::open(&config::db_path()).context("failed to open database")?;
    let cancel = CancellationToken::new();
    let scheduler = Scheduler::new(store, cancel.clone());

    // Handle SIGINT (Ctrl+C) and SIGTERM
    let cancel_clone = cancel.clone();
    tokio::spawn(async move {
        #[allow(clippy::expect_used)]
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to register SIGTERM handler");

        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = sigterm.recv() => {},
        }
        info!("received shutdown signal");
        cancel_clone.cancel();
    });

    println!("kron daemon started. Press Ctrl+C to stop.");
    scheduler.run().await.context("scheduler error")?;
    println!("kron daemon stopped.");

    Ok(())
}
