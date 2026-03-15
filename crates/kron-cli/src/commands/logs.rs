use anyhow::{bail, Context, Result};
use kron_core::config;
use kron_store::Store;

pub fn execute(job_name: &str, run_number: usize) -> Result<()> {
    // Verify job exists via TOML config (single source of truth)
    let jobs = config::load_all_jobs().context("failed to load jobs")?;
    if !jobs.iter().any(|j| j.job.name == job_name) {
        bail!("job '{job_name}' not found");
    }

    let store = Store::open(&config::db_path()).context("failed to open database")?;
    // Fetch up to run_number runs (DESC order). Index run_number-1 gives the requested run.
    // TODO: could be optimized with OFFSET for large run counts
    let runs = store.list_runs(job_name, run_number)?;

    if runs.is_empty() {
        bail!("no runs recorded for '{job_name}'");
    }

    // runs are in DESC order (most recent first). run_number=1 means most recent = index 0
    let run = runs.get(run_number - 1).with_context(|| {
        format!(
            "run #{run_number} not found (only {} runs recorded)",
            runs.len()
        )
    })?;

    let exit_code = run
        .exit_code
        .map_or_else(|| "-".to_string(), |c| c.to_string());

    println!(
        "=== Job: {job_name} | Run #{run_number} | {} ===",
        run.started_at.format("%Y-%m-%d %H:%M:%S")
    );
    println!("Status: {} | Exit code: {exit_code}", run.status.as_str());

    if let Some(finished) = run.finished_at {
        let duration = finished - run.started_at;
        println!("Duration: {}s", duration.num_seconds());
    }

    println!();

    if !run.stdout.is_empty() {
        println!("--- stdout ---");
        print!("{}", run.stdout);
        if !run.stdout.ends_with('\n') {
            println!();
        }
    }

    if !run.stderr.is_empty() {
        println!("--- stderr ---");
        print!("{}", run.stderr);
        if !run.stderr.ends_with('\n') {
            println!();
        }
    }

    if run.stdout.is_empty() && run.stderr.is_empty() {
        println!("(no output)");
    }

    Ok(())
}
