use anyhow::{Context, Result};
use kron_core::config;

pub fn execute() -> Result<()> {
    let jobs = config::load_all_jobs().context("failed to load jobs")?;

    if jobs.is_empty() {
        println!("No jobs configured. Use 'kron add' to create one.");
        return Ok(());
    }

    println!(
        "{:<10} {:<20} {:<8} {:<25} COMMAND",
        "ID", "NAME", "ENABLED", "SCHEDULE"
    );
    println!("{}", "-".repeat(85));
    for job_config in &jobs {
        let job = &job_config.job;
        let name_display = job.name.as_deref().unwrap_or("-");
        println!(
            "{:<10} {:<20} {:<8} {:<25} {}",
            job.id,
            name_display,
            if job.enabled { "yes" } else { "no" },
            job.schedule,
            job.command,
        );
    }

    Ok(())
}
