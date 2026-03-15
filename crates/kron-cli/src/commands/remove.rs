use anyhow::{Context, Result};
use kron_core::config;

pub fn execute(query: &str) -> Result<()> {
    let job = config::find_job(query)
        .context("failed to load jobs")?
        .with_context(|| format!("job '{query}' not found"))?;

    config::delete_job_file(&job.job.id)
        .with_context(|| format!("failed to remove job '{}'", job.job.id))?;

    if let Some(ref name) = job.job.name {
        println!("Removed job '{}' ({})", job.job.id, name);
    } else {
        println!("Removed job '{}'", job.job.id);
    }
    Ok(())
}
