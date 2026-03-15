use anyhow::{Context, Result};
use std::process::Command as StdCommand;
use tokio_util::sync::CancellationToken;
use tracing::info;

use kron_core::{config, scheduler::Scheduler};
use kron_store::Store;

pub async fn execute(foreground: bool) -> Result<()> {
    if !foreground {
        // Re-launch as a background process
        let exe = std::env::current_exe().context("failed to determine kron binary path")?;

        // Ensure data directory exists
        let data_dir = kron_core::config::data_dir();
        std::fs::create_dir_all(&data_dir).context("failed to create data directory")?;

        // Open a log file for daemon output
        let log_path = data_dir.join("daemon.log");
        let log_file =
            std::fs::File::create(&log_path).context("failed to create daemon log file")?;
        let stderr_file = log_file
            .try_clone()
            .context("failed to clone log file handle")?;

        let child = StdCommand::new(exe)
            .args(["daemon", "--foreground"])
            .stdout(log_file)
            .stderr(stderr_file)
            .stdin(std::process::Stdio::null())
            .spawn()
            .context("failed to start daemon process")?;

        let pid = child.id();

        // Save PID file for later use
        let pid_path = data_dir.join("daemon.pid");
        std::fs::write(&pid_path, pid.to_string()).context("failed to write PID file")?;

        println!("kron daemon started (pid {pid})");
        println!("  Logs: {}", log_path.display());
        println!("  PID file: {}", pid_path.display());
        println!("  Stop with: kill {pid}");

        return Ok(());
    }

    // Foreground mode — existing behavior
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
