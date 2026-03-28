//! Integration test: full job lifecycle through the public API.
//!
//! Uses an in-memory `SQLite` store and a temporary directory for job files so
//! no real user config is touched.

#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use chrono::Utc;
use kron_core::config::{JobConfig, JobDefinition, generate_short_id};
use kron_store::{RunRecord, RunStatus, Store};

/// Build a test `JobConfig` with sensible defaults for lifecycle testing.
fn make_job_config(id: &str) -> JobConfig {
    JobConfig {
        job: JobDefinition {
            id: id.to_string(),
            name: Some(format!("test-job-{id}")),
            command: "echo lifecycle-ok".to_string(),
            schedule: "0 2 * * *".to_string(),
            working_dir: None,
            enabled: true,
            timeout: None,
            env: None,
            alert: None,
            once: false,
        },
    }
}

fn make_run_record(job_id: &str, job_name: &str) -> RunRecord {
    RunRecord {
        id: generate_short_id(),
        job_id: job_id.to_string(),
        job_name: job_name.to_string(),
        started_at: Utc::now(),
        finished_at: None,
        exit_code: None,
        stdout: String::new(),
        stderr: String::new(),
        status: RunStatus::Running,
    }
}

// ---------------------------------------------------------------------------
// Config round-trip
// ---------------------------------------------------------------------------

#[test]
fn config_save_and_load_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let id = generate_short_id();
    let config = make_job_config(&id);

    // Write to a temp path directly (bypasses jobs_dir())
    let toml_path: PathBuf = dir.path().join(format!("{id}.toml"));
    let contents = toml::to_string(&config).unwrap();
    std::fs::write(&toml_path, &contents).unwrap();

    let loaded = kron_core::config::load_job(&toml_path).unwrap();
    assert_eq!(loaded.job.id, id);
    assert_eq!(loaded.job.command, "echo lifecycle-ok");
    assert_eq!(loaded.job.schedule, "0 2 * * *");
    assert!(loaded.job.enabled);
    assert!(!loaded.job.once);
}

#[test]
fn config_name_survives_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let id = generate_short_id();
    let config = make_job_config(&id);

    let toml_path = dir.path().join(format!("{id}.toml"));
    std::fs::write(&toml_path, toml::to_string(&config).unwrap()).unwrap();

    let loaded = kron_core::config::load_job(&toml_path).unwrap();
    assert_eq!(
        loaded.job.name.as_deref(),
        Some(format!("test-job-{id}").as_str())
    );
}

// ---------------------------------------------------------------------------
// Store: insert + update + query
// ---------------------------------------------------------------------------

#[test]
fn store_insert_and_list_runs() {
    let store = Store::open_in_memory().unwrap();
    let job_id = generate_short_id();
    let job_name = format!("test-job-{job_id}");

    let mut run = make_run_record(&job_id, &job_name);
    store.insert_run(&run).unwrap();

    let runs = store.list_runs(&job_id, 10).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].job_id, job_id);
    assert_eq!(runs[0].status, RunStatus::Running);

    // Update the run with results
    run.finished_at = Some(Utc::now());
    run.exit_code = Some(0);
    run.stdout = "lifecycle-ok".to_string();
    run.status = RunStatus::Success;
    store.update_run(&run).unwrap();

    let updated = store.list_runs(&job_id, 10).unwrap();
    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0].status, RunStatus::Success);
    assert_eq!(updated[0].exit_code, Some(0));
    assert_eq!(updated[0].stdout, "lifecycle-ok");
    assert!(updated[0].finished_at.is_some());
}

#[test]
fn store_list_runs_returns_most_recent_first() {
    let store = Store::open_in_memory().unwrap();
    let job_id = generate_short_id();
    let job_name = format!("job-{job_id}");

    for i in 0..3u8 {
        let mut run = make_run_record(&job_id, &job_name);
        run.started_at = Utc::now() + chrono::Duration::seconds(i64::from(i));
        run.status = RunStatus::Success;
        run.exit_code = Some(0);
        store.insert_run(&run).unwrap();
    }

    let runs = store.list_runs(&job_id, 10).unwrap();
    assert_eq!(runs.len(), 3);
    // Most recent first
    assert!(runs[0].started_at >= runs[1].started_at);
    assert!(runs[1].started_at >= runs[2].started_at);
}

#[test]
fn store_list_runs_respects_limit() {
    let store = Store::open_in_memory().unwrap();
    let job_id = generate_short_id();
    let job_name = format!("job-{job_id}");

    for _ in 0..5 {
        let run = make_run_record(&job_id, &job_name);
        store.insert_run(&run).unwrap();
    }

    let runs = store.list_runs(&job_id, 2).unwrap();
    assert_eq!(runs.len(), 2);
}

#[test]
fn store_empty_list_when_no_runs() {
    let store = Store::open_in_memory().unwrap();
    let runs = store.list_runs("nonexistent-job-id", 10).unwrap();
    assert!(runs.is_empty());
}

// ---------------------------------------------------------------------------
// Full lifecycle: config + runner + store
// ---------------------------------------------------------------------------

#[tokio::test]
async fn full_lifecycle_execute_and_record() {
    // 1. Build config
    let job_id = generate_short_id();
    let job_name = format!("lifecycle-{job_id}");

    // 2. Execute the command via the runner
    let output = kron_core::runner::execute_command("echo lifecycle-ok", None, None, None)
        .await
        .unwrap();

    assert!(output.success);
    assert_eq!(output.exit_code, Some(0));
    assert_eq!(output.stdout.trim(), "lifecycle-ok");

    // 3. Open in-memory store and record the run
    let store = Store::open_in_memory().unwrap();
    let mut run = make_run_record(&job_id, &job_name);
    store.insert_run(&run).unwrap();

    // 4. Update with real runner results
    run.finished_at = Some(output.finished_at);
    run.exit_code = output.exit_code;
    run.stdout = output.stdout.clone();
    run.stderr = output.stderr.clone();
    run.status = if output.success {
        RunStatus::Success
    } else {
        RunStatus::Failed
    };
    store.update_run(&run).unwrap();

    // 5. Query and verify
    let runs = store.list_runs(&job_id, 10).unwrap();
    assert_eq!(runs.len(), 1);
    let recorded = &runs[0];
    assert_eq!(recorded.status, RunStatus::Success);
    assert_eq!(recorded.exit_code, Some(0));
    assert_eq!(recorded.stdout.trim(), "lifecycle-ok");
    assert!(recorded.finished_at.is_some());
}

#[tokio::test]
async fn lifecycle_failing_command_records_failure() {
    let job_id = generate_short_id();
    let job_name = format!("fail-job-{job_id}");

    let output = kron_core::runner::execute_command("exit 42", None, None, None)
        .await
        .unwrap();

    assert!(!output.success);
    assert_eq!(output.exit_code, Some(42));

    let store = Store::open_in_memory().unwrap();
    let mut run = make_run_record(&job_id, &job_name);
    store.insert_run(&run).unwrap();

    run.finished_at = Some(output.finished_at);
    run.exit_code = output.exit_code;
    run.stdout = output.stdout;
    run.stderr = output.stderr;
    run.status = if output.success {
        RunStatus::Success
    } else {
        RunStatus::Failed
    };
    store.update_run(&run).unwrap();

    let runs = store.list_runs(&job_id, 10).unwrap();
    assert_eq!(runs[0].status, RunStatus::Failed);
    assert_eq!(runs[0].exit_code, Some(42));
}
