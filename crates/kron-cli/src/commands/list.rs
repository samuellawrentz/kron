use anyhow::{Context, Result};
use kron_core::config;

pub fn execute() -> Result<()> {
    let jobs = config::load_all_jobs().context("failed to load jobs")?;

    if jobs.is_empty() {
        println!("No jobs configured. Use 'kron add' to create one.");
        return Ok(());
    }

    println!("{:<20} {:<8} {:<25} COMMAND", "NAME", "ENABLED", "SCHEDULE");
    println!("{}", "-".repeat(75));
    for job_config in &jobs {
        let job = &job_config.job;
        println!(
            "{:<20} {:<8} {:<25} {}",
            job.name,
            if job.enabled { "yes" } else { "no" },
            job.schedule,
            job.command,
        );
    }

    Ok(())
}
