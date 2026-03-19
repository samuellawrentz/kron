use anyhow::{Context, Result, bail};
use chrono::Utc;
use uuid::Uuid;

use kron_core::{config, runner};
use kron_store::{RunRecord, RunStatus, Store};

pub async fn execute(query: &str) -> Result<()> {
    // Resolve job by ID or name
    let job_config = config::find_job(query)
        .context("failed to load jobs")?
        .map(|c| c.job);
    let Some(job) = job_config else {
        bail!("job '{query}' not found")
    };

    let job_display = job.name.as_deref().unwrap_or(&job.id);
    println!("Running '{}': {}", job_display, job.command);

    let store = Store::open(&config::db_path()).context("failed to open database")?;
    let run_id = Uuid::new_v4().to_string();
    let started_at = Utc::now();

    // Record run start — job_id is the short ID; job_name is the display name
    let run = RunRecord {
        id: run_id.clone(),
        job_id: job.id.clone(),
        job_name: job.name.clone().unwrap_or_else(|| job.id.clone()),
        started_at,
        finished_at: None,
        exit_code: None,
        stdout: String::new(),
        stderr: String::new(),
        status: RunStatus::Running,
    };
    store
        .insert_run(&run)
        .context("failed to record run start")?;

    let timeout = job
        .timeout
        .as_deref()
        .and_then(kron_core::scheduler::parse_duration);

    // Execute, preferring .sh script if available
    let script = config::script_path(&job.id);
    let output = runner::execute_command_or_script(
        &job.command,
        Some(script.as_path()),
        job.working_dir.as_deref(),
        timeout,
        job.env.as_ref(),
    )
    .await?;

    // Record result
    let status = if output.success {
        RunStatus::Success
    } else {
        RunStatus::Failed
    };
    let completed_run = RunRecord {
        id: run_id,
        job_id: job.id.clone(),
        job_name: job.name.clone().unwrap_or_else(|| job.id.clone()),
        started_at,
        finished_at: Some(output.finished_at),
        exit_code: output.exit_code,
        stdout: output.stdout.clone(),
        stderr: output.stderr.clone(),
        status,
    };
    store
        .update_run(&completed_run)
        .context("failed to record run result")?;

    // Print result
    if output.success {
        println!(
            "Completed successfully (exit code {})",
            output.exit_code.unwrap_or(0)
        );
    } else {
        println!(
            "Failed (exit code {})",
            output
                .exit_code
                .map_or("unknown".to_string(), |c| c.to_string())
        );
    }

    if !output.stdout.is_empty() {
        print!("{}", output.stdout);
    }
    if !output.stderr.is_empty() {
        eprint!("{}", output.stderr);
    }

    Ok(())
}
