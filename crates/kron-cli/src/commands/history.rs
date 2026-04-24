use anyhow::{Context, Result};
use kron_core::config;
use kron_store::Store;

pub fn execute(query: Option<&str>, count: usize) -> Result<()> {
    let store = Store::open(&config::db_path()).context("failed to open database")?;

    if let Some(query) = query {
        // Single-job view: resolve by config, or fall back to querying the store
        // directly (handles --once jobs whose config was auto-removed).
        let (job_id, display) = match config::find_job(query).ok().flatten() {
            Some(cfg) => {
                let d = cfg.job.name.clone().unwrap_or_else(|| cfg.job.id.clone());
                (cfg.job.id, d)
            }
            None => (query.to_string(), query.to_string()),
        };

        let runs = store
            .list_runs_summary(&job_id, count)
            .context("failed to list runs")?;

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
            println!(
                "{:<4} {:<10} {:<12} {:<10} {}",
                i + 1,
                run.status.as_str(),
                format_exit_code(run.exit_code),
                format_duration(run.started_at, run.finished_at),
                run.started_at
                    .with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M:%S"),
            );
        }
    } else {
        // All-jobs view
        let runs = store
            .list_all_runs_summary(count)
            .context("failed to list runs")?;

        if runs.is_empty() {
            println!("No runs recorded yet.");
            return Ok(());
        }

        println!("Run history (all jobs, most recent first):\n");
        println!(
            "{:<4} {:<20} {:<10} {:<12} {:<10} STARTED",
            "#", "JOB", "STATUS", "EXIT CODE", "DURATION"
        );
        println!("{}", "-".repeat(85));

        for (i, run) in runs.iter().enumerate() {
            let job_display = truncate_display(run.display_name(), 20);
            println!(
                "{:<4} {:<20} {:<10} {:<12} {:<10} {}",
                i + 1,
                job_display,
                run.status.as_str(),
                format_exit_code(run.exit_code),
                format_duration(run.started_at, run.finished_at),
                run.started_at
                    .with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M:%S"),
            );
        }
    }

    Ok(())
}

fn format_exit_code(code: Option<i32>) -> String {
    code.map_or_else(|| "-".to_string(), |c| c.to_string())
}

fn format_duration(
    started: chrono::DateTime<chrono::Utc>,
    finished: Option<chrono::DateTime<chrono::Utc>>,
) -> String {
    finished.map_or_else(
        || "running".to_string(),
        |f| {
            let secs = (f - started).num_seconds();
            if secs < 1 {
                "<1s".to_string()
            } else {
                format!("{secs}s")
            }
        },
    )
}

fn truncate_display(s: &str, max: usize) -> String {
    super::truncate_display(s, max)
}
