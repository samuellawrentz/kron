use anyhow::{Context, Result};
use kron_core::config;
use kron_store::Store;

pub fn execute() -> Result<()> {
    let jobs = config::load_all_jobs().context("failed to load jobs")?;

    if jobs.is_empty() {
        println!("No jobs configured. Use 'kron add' to create one.");
        return Ok(());
    }

    let store = Store::open(&config::db_path()).context("failed to open database")?;

    println!(
        "{:<10} {:<20} {:<8} {:<8} {:<25} COMMAND",
        "ID", "NAME", "ENABLED", "RUNS", "SCHEDULE"
    );
    println!("{}", "-".repeat(95));
    for job_config in &jobs {
        let job = &job_config.job;
        let name_display = job.name.as_deref().unwrap_or("-");
        let runs_display = match store.count_runs(&job.id) {
            Ok((success, total)) => format!("{success}/{total}"),
            Err(_) => "-".to_string(),
        };
        println!(
            "{:<10} {:<20} {:<8} {:<8} {:<25} {}",
            job.id,
            name_display,
            if job.enabled { "yes" } else { "no" },
            runs_display,
            job.schedule,
            job.command,
        );
    }

    Ok(())
}
