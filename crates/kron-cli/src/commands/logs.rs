use anyhow::{Context, Result, bail};
use kron_core::config;
use kron_store::Store;

fn validate_run_number(run_number: usize) -> Result<()> {
    if run_number == 0 {
        bail!("run number must be >= 1")
    }
    Ok(())
}

pub fn execute(query: Option<&str>, run_number: usize) -> Result<()> {
    validate_run_number(run_number)?;
    let store = Store::open(&config::db_path()).context("failed to open database")?;

    let run = if let Some(query) = query {
        // Resolve by config, or fall back to querying the store directly
        // (handles --once jobs whose config was auto-removed).
        let (job_id, display) = match config::find_job(query)? {
            Some(cfg) => {
                let d = cfg.job.name.clone().unwrap_or_else(|| cfg.job.id.clone());
                (cfg.job.id, d)
            }
            None => (query.to_string(), query.to_string()),
        };

        let runs = store.list_runs(&job_id, run_number)?;

        if runs.is_empty() {
            bail!("no runs recorded for '{display}'");
        }

        runs.into_iter()
            .nth(run_number - 1)
            .with_context(|| format!("run #{run_number} not found for '{display}'"))?
    } else {
        // No job specified — show the Nth most recent run across all jobs
        store
            .get_nth_latest_run(run_number)?
            .context("no runs recorded yet")?
    };

    let display = run.display_name();

    let exit_code = run
        .exit_code
        .map_or_else(|| "-".to_string(), |c| c.to_string());

    println!(
        "=== Job: {display} | Run #{run_number} | {} ===",
        run.started_at
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M:%S")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_run_number_rejects_zero() {
        assert!(validate_run_number(0).is_err());
    }

    #[test]
    fn test_validate_run_number_accepts_positive() {
        assert!(validate_run_number(1).is_ok());
    }
}
