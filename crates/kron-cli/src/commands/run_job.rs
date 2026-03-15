use anyhow::{Context, Result, bail};
use chrono::Utc;
use uuid::Uuid;

use kron_core::{config, runner};
use kron_store::{RunRecord, RunStatus, Store};

pub async fn execute(job_name: &str) -> Result<()> {
    // Load job definition from TOML (single source of truth)
    let jobs = config::load_all_jobs().context("failed to load jobs")?;
    let job_config = jobs
        .into_iter()
        .find(|j| j.job.name == job_name)
        .map(|j| j.job);
    let Some(job) = job_config else {
        bail!("job '{job_name}' not found")
    };

    println!("Running '{}': {}", job.name, job.command);

    let store = Store::open(&config::db_path()).context("failed to open database")?;
    let run_id = Uuid::new_v4().to_string();
    let started_at = Utc::now();

    // Record run start — job_id is the job name since TOML jobs have no separate UUID
    let run = RunRecord {
        id: run_id.clone(),
        job_id: job.name.clone(),
        job_name: job.name.clone(),
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

    // TODO: parse job.timeout string into Duration when a duration-parsing crate is added
    let timeout = None;

    // Execute
    let output = runner::execute_command(&job.command, job.working_dir.as_deref(), timeout).await?;

    // Record result
    let status = if output.success {
        RunStatus::Success
    } else {
        RunStatus::Failed
    };
    let completed_run = RunRecord {
        id: run_id,
        job_id: job.name.clone(),
        job_name: job.name.clone(),
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
