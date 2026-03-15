use anyhow::{Context, Result};
use kron_core::config;

pub fn execute(job_name: &str) -> Result<()> {
    // TOML is the single source of truth — remove only the config file
    config::delete_job_file(job_name)
        .with_context(|| format!("job '{job_name}' not found"))?;

    println!("Removed job '{job_name}'");
    Ok(())
}
