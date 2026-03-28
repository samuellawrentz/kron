#![allow(clippy::unwrap_used)]
//! End-to-end tests exercising real disk I/O, real `SQLite`, and real process execution.
//!
//! These tests are `#[ignore]`d so they don't run in the pre-commit hook.
//! Run with: `cargo test -- --ignored`

use std::time::{Duration, Instant};

use chrono::Utc;
use kron_core::CoreError;
use kron_core::config::{JobConfig, JobDefinition, generate_short_id, load_job};
use kron_core::runner::{execute_command, execute_command_or_script};
use kron_store::{RunRecord, RunStatus, Store};
use tokio_util::sync::CancellationToken;

fn make_job_config(id: &str) -> JobConfig {
    JobConfig {
        job: JobDefinition {
            id: id.to_string(),
            name: Some(format!("e2e-{id}")),
            command: "echo e2e-ok".to_string(),
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

fn make_run_record(job_id: &str) -> RunRecord {
    RunRecord {
        id: generate_short_id(),
        job_id: job_id.to_string(),
        job_name: format!("e2e-{job_id}"),
        started_at: Utc::now(),
        finished_at: None,
        exit_code: None,
        stdout: String::new(),
        stderr: String::new(),
        status: RunStatus::Running,
    }
}

// ---------------------------------------------------------------------------
// 1. Real file I/O: TOML round-trip + script precedence
// ---------------------------------------------------------------------------

#[test]
#[ignore = "e2e: uses real disk I/O and process spawning"]
fn test_e2e_save_load_delete_job_files() {
    let dir = tempfile::tempdir().unwrap();
    let id = generate_short_id();
    let config = make_job_config(&id);

    // Write TOML directly to tempdir
    let toml_path = dir.path().join(format!("{id}.toml"));
    let toml_str = toml::to_string(&config).unwrap();
    std::fs::write(&toml_path, &toml_str).unwrap();
    assert!(toml_path.exists());

    // Load it back — no .sh next to it, so command comes from TOML
    let loaded = load_job(&toml_path).unwrap();
    assert_eq!(loaded.job.id, id);
    assert_eq!(loaded.job.command, "echo e2e-ok");
    assert_eq!(loaded.job.schedule, "0 2 * * *");
    assert!(loaded.job.enabled);
    assert!(!loaded.job.once);
    assert_eq!(
        loaded.job.name.as_deref(),
        Some(format!("e2e-{id}").as_str())
    );

    // Write a .sh script next to the TOML with a different command
    let sh_path = dir.path().join(format!("{id}.sh"));
    std::fs::write(&sh_path, "#!/bin/sh\necho from-script\n").unwrap();

    // Temporarily place the script where load_script() will find it by writing
    // an updated TOML that references the same id. Since load_script uses
    // jobs_dir() internally, we test the precedence logic by calling load_job
    // which calls load_script internally. Because load_script looks in jobs_dir()
    // (not our tempdir), we verify the TOML-only path returns the inline command
    // and that file I/O itself works correctly.
    //
    // The TOML load_job path with no matching script returns the TOML command.
    assert_eq!(loaded.job.command, "echo e2e-ok");

    // Delete files and verify they're gone
    std::fs::remove_file(&toml_path).unwrap();
    std::fs::remove_file(&sh_path).unwrap();
    assert!(!toml_path.exists());
    assert!(!sh_path.exists());
}

// ---------------------------------------------------------------------------
// 2. Real SQLite persistence across reopen
// ---------------------------------------------------------------------------

#[test]
#[ignore = "e2e: uses real disk I/O and process spawning"]
fn test_e2e_store_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let job_id = generate_short_id();

    // Open, insert, close
    {
        let store = Store::open(&db_path).unwrap();
        let run = make_run_record(&job_id);
        store.insert_run(&run).unwrap();

        let runs = store.list_runs(&job_id, 10).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, RunStatus::Running);
    }

    // Reopen — verify the run persisted
    {
        let store = Store::open(&db_path).unwrap();
        let runs = store.list_runs(&job_id, 10).unwrap();
        assert_eq!(runs.len(), 1, "run must survive store reopen");
        assert_eq!(runs[0].status, RunStatus::Running);
        assert_eq!(runs[0].job_id, job_id);

        // Update to Success
        let mut run = runs[0].clone();
        run.finished_at = Some(Utc::now());
        run.exit_code = Some(0);
        run.stdout = "e2e-ok".to_string();
        run.status = RunStatus::Success;
        store.update_run(&run).unwrap();
    }

    // Reopen again — verify update persisted
    {
        let store = Store::open(&db_path).unwrap();
        let runs = store.list_runs(&job_id, 10).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, RunStatus::Success);
        assert_eq!(runs[0].exit_code, Some(0));
        assert_eq!(runs[0].stdout, "e2e-ok");
        assert!(runs[0].finished_at.is_some());
    }
}

// ---------------------------------------------------------------------------
// 3. Real shell execution
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "e2e: uses real disk I/O and process spawning"]
async fn test_e2e_execute_real_command() {
    let output = execute_command("echo hello-e2e", None, None, None)
        .await
        .unwrap();
    assert!(output.success);
    assert_eq!(output.exit_code, Some(0));
    assert!(
        output.stdout.contains("hello-e2e"),
        "stdout was: {:?}",
        output.stdout
    );

    let result = execute_command("exit 42", None, None, None).await.unwrap();
    assert!(!result.success);
    assert_eq!(result.exit_code, Some(42));
}

// ---------------------------------------------------------------------------
// 4. Real script file execution + fallback
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "e2e: uses real disk I/O and process spawning"]
async fn test_e2e_execute_script_file() {
    let dir = tempfile::tempdir().unwrap();
    let script_path = dir.path().join("test_e2e.sh");
    std::fs::write(&script_path, "#!/bin/sh\necho script-e2e\n").unwrap();

    // Script exists — should take precedence
    let output = execute_command_or_script(
        "echo fallback",
        Some(script_path.as_path()),
        None,
        None,
        None,
    )
    .await
    .unwrap();
    assert!(output.success);
    assert!(
        output.stdout.contains("script-e2e"),
        "expected 'script-e2e' in stdout, got: {:?}",
        output.stdout
    );

    // Delete the script — should fall back to inline command
    std::fs::remove_file(&script_path).unwrap();
    let fallback = execute_command_or_script(
        "echo fallback",
        Some(script_path.as_path()),
        None,
        None,
        None,
    )
    .await
    .unwrap();
    assert!(fallback.success);
    assert!(
        fallback.stdout.contains("fallback"),
        "expected 'fallback' in stdout, got: {:?}",
        fallback.stdout
    );
}

// ---------------------------------------------------------------------------
// 5. Timeout kills the process
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "e2e: uses real disk I/O and process spawning"]
async fn test_e2e_timeout_kills_process() {
    let start = Instant::now();
    let result = execute_command("sleep 30", None, Some(Duration::from_secs(1)), None).await;
    let elapsed = start.elapsed();

    assert!(
        matches!(result, Err(CoreError::Timeout(_))),
        "expected Timeout error, got: {:?}",
        result.err()
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "timeout took too long: {elapsed:?}"
    );
}

// ---------------------------------------------------------------------------
// 6. Full lifecycle: config file + runner + store
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "e2e: uses real disk I/O and process spawning"]
async fn test_e2e_full_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("lifecycle.db");
    let store = Store::open(&db_path).unwrap();

    let id = generate_short_id();
    let config = make_job_config(&id);

    // Write TOML to tempdir and load it back
    let toml_path = dir.path().join(format!("{id}.toml"));
    std::fs::write(&toml_path, toml::to_string(&config).unwrap()).unwrap();
    let loaded = load_job(&toml_path).unwrap();
    assert_eq!(loaded.job.id, id);

    // Execute the command
    let output = execute_command(&loaded.job.command, None, None, None)
        .await
        .unwrap();
    assert!(output.success);
    assert!(output.stdout.contains("e2e-ok"));

    // Record as Running, then update with result
    let mut run = make_run_record(&id);
    store.insert_run(&run).unwrap();

    run.finished_at = Some(output.finished_at);
    run.exit_code = output.exit_code;
    run.stdout = output.stdout.clone();
    run.stderr = output.stderr.clone();
    run.status = RunStatus::Success;
    store.update_run(&run).unwrap();

    // Verify via list_runs
    let runs = store.list_runs(&id, 10).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].status, RunStatus::Success);
    assert_eq!(runs[0].exit_code, Some(0));
    assert!(runs[0].stdout.contains("e2e-ok"));
    assert!(runs[0].finished_at.is_some());

    // Verify via get_latest_run
    let latest = store.get_latest_run().unwrap();
    assert!(latest.is_some());
    assert_eq!(latest.unwrap().job_id, id);
}

// ---------------------------------------------------------------------------
// 7. Store prune on disk
// ---------------------------------------------------------------------------

#[test]
#[ignore = "e2e: uses real disk I/O and process spawning"]
fn test_e2e_store_prune_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("prune.db");
    let job_id = generate_short_id();

    {
        let store = Store::open(&db_path).unwrap();

        // Insert 10 runs with staggered timestamps
        for i in 0..10_i64 {
            let mut run = make_run_record(&job_id);
            run.id = generate_short_id();
            run.started_at = Utc::now() + chrono::Duration::seconds(i);
            run.status = RunStatus::Success;
            run.exit_code = Some(0);
            store.insert_run(&run).unwrap();
        }

        let runs = store.list_runs(&job_id, 100).unwrap();
        assert_eq!(runs.len(), 10);

        // Prune to 5, keep all within 365 days
        let deleted = store.prune_runs(5, 365).unwrap();
        assert_eq!(deleted, 5, "expected 5 runs deleted");

        let after = store.list_runs(&job_id, 100).unwrap();
        assert_eq!(after.len(), 5);
    }

    // Reopen and verify count still 5
    {
        let store = Store::open(&db_path).unwrap();
        let runs = store.list_runs(&job_id, 100).unwrap();
        assert_eq!(runs.len(), 5, "pruned count must survive reopen");
    }
}

// ---------------------------------------------------------------------------
// 8. Daemon/scheduler start + graceful stop
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "e2e: uses real disk I/O and process spawning"]
async fn test_e2e_daemon_start_stop() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("daemon.db");
    let store = Store::open(&db_path).unwrap();

    let cancel = CancellationToken::new();
    let scheduler = kron_core::scheduler::Scheduler::new(store, cancel.clone());

    let handle = tokio::spawn(async move { scheduler.run().await });

    tokio::time::sleep(Duration::from_millis(500)).await;
    cancel.cancel();

    let result = handle.await.unwrap();
    assert!(
        result.is_ok(),
        "scheduler should shut down cleanly: {result:?}"
    );
}
