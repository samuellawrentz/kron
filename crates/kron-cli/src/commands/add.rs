use anyhow::{Context, Result};
use croner::Cron;

use kron_core::config::{self, JobConfig, JobDefinition, validate_job_name};

#[allow(clippy::needless_pass_by_value)]
pub fn execute(
    schedule: String,
    command: Vec<String>,
    name: Option<String>,
    working_dir: Option<String>,
) -> Result<()> {
    // Validate cron expression
    Cron::new(&schedule)
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid schedule '{schedule}': {e}"))?;

    let command_str = command.join(" ");
    let job_name = name.unwrap_or_else(|| {
        // Derive name from command: take basename of first word, sanitize
        command_str
            .split_whitespace()
            .next()
            .unwrap_or("job")
            .rsplit('/')
            .next()
            .unwrap_or("job")
            .replace('.', "-")
    });

    // Validate name before saving (prevents path traversal)
    validate_job_name(&job_name)?;

    // Save TOML config file — TOML is the single source of truth for job definitions
    let job_config = JobConfig {
        job: JobDefinition {
            name: job_name.clone(),
            command: command_str.clone(),
            schedule: schedule.clone(),
            working_dir: working_dir.clone(),
            enabled: true,
            timeout: None,
        },
    };
    let path = config::save_job(&job_config).context("failed to save job config")?;

    println!("Added job '{job_name}'");
    println!("  Config: {}", path.display());
    println!("  Schedule: {schedule}");
    println!("  Command: {command_str}");

    Ok(())
}
