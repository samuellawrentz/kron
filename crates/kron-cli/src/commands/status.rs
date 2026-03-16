use anyhow::{Context, Result};
use kron_core::config;
use kron_store::Store;

pub fn execute() -> Result<()> {
    let jobs = config::load_all_jobs().context("failed to load jobs")?;

    if jobs.is_empty() {
        println!("No jobs configured.");
        return Ok(());
    }

    let store = Store::open(&config::db_path()).context("failed to open database")?;
    let last_runs = store.get_last_run_all_jobs().unwrap_or_default();

    println!(
        "{:<10} {:<20} {:<10} {:<12} {:<20}",
        "ID", "NAME", "STATUS", "EXIT CODE", "LAST RUN"
    );
    println!("{}", "-".repeat(75));

    for job_config in &jobs {
        let job = &job_config.job;
        let name_display = job.name.as_deref().unwrap_or("-");
        let last_run = last_runs.get(&job.id);
        match last_run {
            Some(run) => {
                let duration = run.finished_at.map_or_else(
                    || "running".to_string(),
                    |f| format!("{}s", (f - run.started_at).num_seconds()),
                );
                let exit_code = run
                    .exit_code
                    .map_or_else(|| "-".to_string(), |c| c.to_string());

                println!(
                    "{:<10} {:<20} {:<10} {:<12} {} ({duration})",
                    job.id,
                    name_display,
                    run.status.as_str(),
                    exit_code,
                    run.started_at
                        .with_timezone(&chrono::Local)
                        .format("%Y-%m-%d %H:%M:%S"),
                );
            }
            None => {
                println!(
                    "{:<10} {:<20} {:<10} {:<12} no runs yet",
                    job.id, name_display, "never", "-"
                );
            }
        }
    }

    Ok(())
}
