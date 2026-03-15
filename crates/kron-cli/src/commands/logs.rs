use anyhow::{Context, Result, bail};
use kron_core::config;
use kron_store::Store;

pub fn execute(query: &str, run_number: usize) -> Result<()> {
    // Resolve job by ID or name
    let job_config = config::find_job(query)
        .context("failed to load jobs")?
        .with_context(|| format!("job '{query}' not found"))?;
    let job = &job_config.job;

    let store = Store::open(&config::db_path()).context("failed to open database")?;
    // Fetch up to run_number runs (DESC order). Index run_number-1 gives the requested run.
    let runs = store.list_runs(&job.id, run_number)?;

    let display = job.name.as_deref().unwrap_or(&job.id);

    if runs.is_empty() {
        bail!("no runs recorded for '{display}'");
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
        "=== Job: {display} | Run #{run_number} | {} ===",
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
