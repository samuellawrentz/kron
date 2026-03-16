use anyhow::{Context, Result, bail};

use kron_core::{config, runner};

pub async fn execute(job_query: &str) -> Result<()> {
    // Resolve job by ID or name
    let job_config = config::find_job(job_query)
        .context("failed to load jobs")?
        .ok_or_else(|| anyhow::anyhow!("job '{job_query}' not found"))?;
    let job = &job_config.job;

    let display_name = job.name.as_deref().unwrap_or(&job.id);

    println!("=== DRY RUN: {display_name} ({}) ===", job.id);
    println!("  Command: {}", job.command);
    if let Some(ref dir) = job.working_dir {
        println!("  Working dir: {dir}");
    }
    println!("  Schedule: {}", job.schedule);
    println!();
    println!("Running...");
    println!();

    // Execute the command but DON'T record in database
    let output = runner::execute_command(
        &job.command,
        job.working_dir.as_deref(),
        None, // no timeout for dry-run
        job.env.as_ref(),
    )
    .await?;

    // Print output
    if !output.stdout.is_empty() {
        println!("--- stdout ---");
        print!("{}", output.stdout);
        if !output.stdout.ends_with('\n') {
            println!();
        }
    }

    if !output.stderr.is_empty() {
        println!("--- stderr ---");
        print!("{}", output.stderr);
        if !output.stderr.ends_with('\n') {
            println!();
        }
    }

    if output.stdout.is_empty() && output.stderr.is_empty() {
        println!("(no output)");
    }

    let duration = output.finished_at - output.started_at;
    let exit_code = output
        .exit_code
        .map_or_else(|| "unknown".to_string(), |c| c.to_string());

    println!();
    if output.success {
        println!("Exit code: {exit_code} (success)");
    } else {
        println!("Exit code: {exit_code} (FAILED)");
    }
    println!("Duration: {}s", duration.num_seconds());
    println!("(not recorded in history)");

    if !output.success {
        bail!("job exited with failure status");
    }

    Ok(())
}
