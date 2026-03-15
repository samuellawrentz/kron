use anyhow::{Context, Result};
use kron_core::config;
use kron_store::Store;

pub fn execute(query: &str, count: usize) -> Result<()> {
    // Resolve job by ID or name
    let job_config = config::find_job(query)
        .context("failed to load jobs")?
        .with_context(|| format!("job '{query}' not found"))?;
    let job = &job_config.job;

    let store = Store::open(&config::db_path()).context("failed to open database")?;
    // Query by job ID (also matches old runs stored with name as job_id/job_name)
    let runs = store
        .list_runs(&job.id, count)
        .context("failed to list runs")?;

    let display = job.name.as_deref().unwrap_or(&job.id);

    if runs.is_empty() {
        println!("No runs recorded for '{display}'.");
        return Ok(());
    }

    println!("Run history for '{display}' (most recent first):\n");
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
            run.started_at
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S"),
        );
    }

    Ok(())
}
