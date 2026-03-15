use anyhow::{Context, Result};
use kron_core::config;
use kron_store::Store;

pub fn execute(job_name: &str, count: usize) -> Result<()> {
    // Verify job exists via TOML config (single source of truth)
    let jobs = config::load_all_jobs().context("failed to load jobs")?;
    if !jobs.iter().any(|j| j.job.name == job_name) {
        anyhow::bail!("job '{job_name}' not found");
    }

    let store = Store::open(&config::db_path()).context("failed to open database")?;
    let runs = store
        .list_runs(job_name, count)
        .context("failed to list runs")?;

    if runs.is_empty() {
        println!("No runs recorded for '{job_name}'.");
        return Ok(());
    }

    println!("Run history for '{job_name}' (most recent first):\n");
    println!(
        "{:<4} {:<10} {:<12} {:<10} STARTED",
        "#", "STATUS", "EXIT CODE", "DURATION"
    );
    println!("{}", "-".repeat(65));

    for (i, run) in runs.iter().enumerate() {
        let duration = run.finished_at.map_or_else(
            || "running".to_string(),
            |f| {
                let secs = (f - run.started_at).num_seconds();
                if secs < 1 {
                    "<1s".to_string()
                } else {
                    format!("{secs}s")
                }
            },
        );
        let exit_code = run
            .exit_code
            .map_or_else(|| "-".to_string(), |c| c.to_string());

        println!(
            "{:<4} {:<10} {:<12} {:<10} {}",
            i + 1,
            run.status.as_str(),
            exit_code,
            duration,
            run.started_at.format("%Y-%m-%d %H:%M:%S"),
        );
    }

    Ok(())
}
