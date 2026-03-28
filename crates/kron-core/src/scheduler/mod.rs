//! Next-fire scheduler that computes sleep intervals and dispatches due jobs.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{Local, Timelike, Utc};
use croner::Cron;
use kron_store::{RunRecord, RunStatus, Store};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, error, info, info_span, warn};
use uuid::Uuid;

use crate::config::{self, JobDefinition};
use crate::error::CoreError;
use crate::runner;
use crate::systemd;

/// Maximum time to sleep between checking for due jobs.
/// Acts as a fallback reload interval for manually edited TOML files.
const MAX_SLEEP_SECS: u64 = 60;

/// Parsed and cached job state to avoid re-reading filesystem and re-parsing cron every tick.
struct CachedJob {
    def: JobDefinition,
    cron: Cron,
}

pub struct Scheduler {
    store: Arc<Mutex<Store>>,
    cancel: CancellationToken,
    running_jobs: Arc<Mutex<HashSet<String>>>,
    reload_signal: Arc<Notify>,
    silence_alerted: Arc<Mutex<HashSet<String>>>,
}

impl Scheduler {
    pub fn new(store: Store, cancel: CancellationToken) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
            cancel,
            running_jobs: Arc::new(Mutex::new(HashSet::new())),
            reload_signal: Arc::new(Notify::new()),
            silence_alerted: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Get a handle to signal a config reload (e.g., from a SIGHUP handler).
    #[must_use]
    pub fn reload_handle(&self) -> Arc<Notify> {
        Arc::clone(&self.reload_signal)
    }

    /// Run the scheduler loop using a next-fire model.
    ///
    /// Instead of polling every second, computes the next job due time and sleeps
    /// until then. Wakes early on: cancellation, SIGHUP reload signal, or a
    /// fallback interval (60s) for picking up manually edited config files.
    ///
    /// # Errors
    /// Returns `CoreError` if a fatal scheduling error occurs.
    pub async fn run(&self) -> Result<(), CoreError> {
        info!("scheduler started (next-fire model)");

        let mut last_prune = Instant::now();

        // Load jobs initially
        let mut cached_jobs: Vec<CachedJob> = tokio::task::spawn_blocking(reload_jobs)
            .await
            .ok()
            .flatten()
            .unwrap_or_default();

        info!(jobs = cached_jobs.len(), "loaded initial job configs");

        // Watchdog interval: ping systemd every 15s so it knows we're alive.
        // No-op when not running under systemd (e.g. macOS, manual start).
        let mut watchdog = tokio::time::interval(Duration::from_secs(15));
        watchdog.tick().await; // consume the immediate first tick

        loop {
            // Compute next fire time across all enabled jobs
            let now = Local::now();
            let sleep_duration = compute_next_sleep(&cached_jobs, &now);

            info!(
                sleep_secs = sleep_duration.as_secs(),
                "sleeping until next job or reload"
            );

            tokio::select! {
                () = self.cancel.cancelled() => {
                    info!("scheduler shutting down");
                    return Ok(());
                }
                () = self.reload_signal.notified() => {
                    info!("reload signal received, reloading configs");
                    if let Some(jobs) = reload_jobs_async().await {
                        cached_jobs = jobs;
                        info!(jobs = cached_jobs.len(), "reloaded job configs");
                    }
                }
                () = tokio::time::sleep(sleep_duration) => {
                    // Fire due jobs
                    self.fire_due_jobs(&cached_jobs);

                    // Periodic config reload (every MAX_SLEEP_SECS as fallback)
                    // This catches manually edited TOML files
                    if sleep_duration.as_secs() >= MAX_SLEEP_SECS
                        && let Some(jobs) = reload_jobs_async().await
                    {
                        cached_jobs = jobs;
                    }

                    // Prune old runs and check silence every 60 seconds
                    if last_prune.elapsed() >= Duration::from_secs(60) {
                        self.prune_old_runs().await;
                        self.check_silence(&cached_jobs).await;
                        last_prune = Instant::now();
                    }
                }
                _ = watchdog.tick() => {
                    systemd::sd_notify("WATCHDOG=1");
                }
            }
        }
    }

    /// Check all jobs and fire those whose schedule matches the current minute.
    ///
    /// croner's `is_time_matching` uses second-level granularity — it only
    /// returns `true` when the seconds component is exactly 0.  We truncate
    /// the current time to the start of the minute so that the check succeeds
    /// regardless of which second within the minute we happen to wake up.
    fn fire_due_jobs(&self, jobs: &[CachedJob]) {
        let now = Local::now()
            .with_second(0)
            .and_then(|t| t.with_nanosecond(0))
            .unwrap_or_else(Local::now);

        for cached in jobs {
            let def = &cached.def;
            if !def.enabled {
                continue;
            }

            // Skip if job is already running
            {
                let running = self.running_jobs.lock().unwrap_or_else(|e| {
                    warn!("running_jobs mutex was poisoned, recovering");
                    e.into_inner()
                });
                if running.contains(&def.id) {
                    let job_label = def.name.as_deref().unwrap_or(&def.id);
                    info!(job = %job_label, "job already running, skipping");
                    continue;
                }
            }

            match cached.cron.is_time_matching(&now) {
                Ok(true) => {
                    let job_label = def.name.as_deref().unwrap_or(&def.id);
                    info!(job = %job_label, "triggering job");
                    self.spawn_job(def);
                }
                Ok(false) => {}
                Err(e) => {
                    let job_label = def.name.as_deref().unwrap_or(&def.id);
                    warn!(job = %job_label, "schedule match error: {e}");
                }
            }
        }
    }

    /// Check all jobs with `on_silence` set and fire an alert if they've been silent too long.
    async fn check_silence(&self, jobs: &[CachedJob]) {
        for cached in jobs {
            let def = &cached.def;
            let Some(ref alert) = def.alert else { continue };
            let Some(ref silence_str) = alert.on_silence else {
                continue;
            };
            let Some(silence_dur) = parse_duration(silence_str) else {
                continue;
            };

            // Check if already alerted for this job
            {
                let alerted = self
                    .silence_alerted
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if alerted.contains(&def.id) {
                    continue;
                }
            }

            let s = Arc::clone(&self.store);
            let job_id = def.id.clone();
            let last_run = tokio::task::spawn_blocking(move || {
                let store = s.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                store.get_last_run(&job_id)
            })
            .await;

            let elapsed = match last_run {
                Ok(Ok(Some(run))) => Utc::now().signed_duration_since(run.started_at),
                Ok(Ok(None)) => {
                    // Never run — treat as infinitely silent
                    chrono::Duration::MAX
                }
                Ok(Err(e)) => {
                    warn!(job = %def.id, "failed to query last run for silence check: {e}");
                    continue;
                }
                Err(e) => {
                    error!("spawn_blocking panicked in silence check: {e}");
                    continue;
                }
            };

            let silence_chrono =
                chrono::Duration::from_std(silence_dur).unwrap_or(chrono::Duration::MAX);
            if elapsed <= silence_chrono {
                continue;
            }

            // Mark as alerted before firing to avoid races
            {
                let mut alerted = self
                    .silence_alerted
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                alerted.insert(def.id.clone());
            }

            let alert_config = match tokio::task::spawn_blocking(config::load_alerts).await {
                Ok(Ok(cfg)) if !cfg.provider.is_empty() => cfg,
                Ok(Err(e)) => {
                    warn!("failed to load alert config for silence check: {e}");
                    continue;
                }
                _ => continue,
            };

            let job_name = def.name.clone().unwrap_or_else(|| def.id.clone());
            let subject = format!("kron job silent: {job_name}");
            let hours = elapsed.num_hours();
            let body = if elapsed == chrono::Duration::MAX {
                format!("Job '{job_name}' has never run and on_silence is set.")
            } else {
                format!("Job '{job_name}' has not run in {hours}h (threshold: {silence_str}).")
            };

            info!(job = %job_name, elapsed_hours = hours, "firing silence alert");
            tokio::spawn(async move {
                crate::notify::notify_all(&alert_config.provider, &subject, &body).await;
            });
        }
    }

    /// Prune old runs based on retention policy.
    async fn prune_old_runs(&self) {
        let s = Arc::clone(&self.store);
        let _ = tokio::task::spawn_blocking(move || {
            let Ok(global_config) = config::load_global_config() else {
                return;
            };
            let retention = &global_config.retention;
            let store = s.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            match store.prune_runs(retention.max_runs_per_job, retention.max_age_days) {
                Ok(0) => {}
                Ok(n) => tracing::info!(deleted = n, "pruned old run records"),
                Err(e) => tracing::warn!("failed to prune runs: {e}"),
            }
        })
        .await;
    }

    fn spawn_job(&self, def: &JobDefinition) {
        let store = Arc::clone(&self.store);
        let running_jobs = Arc::clone(&self.running_jobs);
        let silence_alerted = Arc::clone(&self.silence_alerted);
        let job_id = def.id.clone();
        let job_display_name = def.name.clone().unwrap_or_else(|| def.id.clone());
        let command = def.command.clone();
        let working_dir = def.working_dir.clone();
        let timeout = def.timeout.as_deref().and_then(parse_duration);
        let env_vars = def.env.clone();
        let job_alert = def.alert.clone();
        let once = def.once;
        let script = config::script_path(&def.id);

        let span_name = job_display_name.clone();
        tokio::spawn(
            async move {
                mark_running(&running_jobs, &job_id);
                let _guard = RunningGuard {
                    running_jobs: Arc::clone(&running_jobs),
                    name: job_id.clone(),
                };

                let run_id = Uuid::new_v4().to_string();
                let started_at = Utc::now();

                record_run_start(
                    &store,
                    run_id.clone(),
                    job_id.clone(),
                    job_display_name.clone(),
                    started_at,
                )
                .await;

                let result = runner::execute_command_or_script(
                    &command,
                    Some(script.as_path()),
                    working_dir.as_deref(),
                    timeout,
                    env_vars.as_ref(),
                )
                .await;

                let outcome = resolve_outcome(result, &job_display_name);

                record_run_result(
                    &store,
                    run_id,
                    &job_id,
                    &job_display_name,
                    started_at,
                    &outcome,
                )
                .await;

                // Reset silence alert tracking when job runs successfully
                if outcome.status == RunStatus::Success {
                    let mut alerted = silence_alerted
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    alerted.remove(&job_id);
                }

                if once {
                    handle_once_removal(&job_id, &job_display_name).await;
                }

                if let Some(ref alert) = job_alert {
                    dispatch_alerts(alert, &outcome, &job_display_name, &command).await;
                }
            }
            .instrument(info_span!("job_run", job = %span_name)),
        );
    }
}

/// Outcome of a job execution, extracted from the runner result.
struct JobOutcome {
    finished_at: chrono::DateTime<Utc>,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    status: RunStatus,
}

fn mark_running(running_jobs: &Arc<Mutex<HashSet<String>>>, job_id: &str) {
    let mut running = running_jobs.lock().unwrap_or_else(|e| {
        warn!("running_jobs mutex was poisoned, recovering");
        e.into_inner()
    });
    running.insert(job_id.to_string());
}

async fn record_run_start(
    store: &Arc<Mutex<Store>>,
    run_id: String,
    job_id: String,
    job_display_name: String,
    started_at: chrono::DateTime<Utc>,
) {
    let s = Arc::clone(store);
    let run = RunRecord {
        id: run_id,
        job_id,
        job_name: job_display_name,
        started_at,
        finished_at: None,
        exit_code: None,
        stdout: String::new(),
        stderr: String::new(),
        status: RunStatus::Running,
    };
    let result = tokio::task::spawn_blocking(move || {
        let store = s.lock().unwrap_or_else(|e| {
            warn!("store mutex was poisoned, recovering");
            e.into_inner()
        });
        store.insert_run(&run)
    })
    .await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => error!("failed to record run start: {e}"),
        Err(e) => error!("spawn_blocking panicked recording run start: {e}"),
    }
}

fn resolve_outcome(
    result: Result<crate::runner::JobOutput, CoreError>,
    job_display_name: &str,
) -> JobOutcome {
    match result {
        Ok(output) => {
            let status = if output.success {
                RunStatus::Success
            } else {
                RunStatus::Failed
            };
            info!(
                exit_code = ?output.exit_code,
                success = output.success,
                "job completed"
            );
            JobOutcome {
                finished_at: output.finished_at,
                exit_code: output.exit_code,
                stdout: output.stdout,
                stderr: output.stderr,
                status,
            }
        }
        Err(CoreError::Timeout(dur)) => {
            warn!(job = %job_display_name, timeout = ?dur, "job timed out");
            JobOutcome {
                finished_at: Utc::now(),
                exit_code: None,
                stdout: String::new(),
                stderr: format!("timed out after {dur:?}"),
                status: RunStatus::Failed,
            }
        }
        Err(e) => {
            error!("job execution failed: {e}");
            JobOutcome {
                finished_at: Utc::now(),
                exit_code: None,
                stdout: String::new(),
                stderr: e.to_string(),
                status: RunStatus::Failed,
            }
        }
    }
}

async fn record_run_result(
    store: &Arc<Mutex<Store>>,
    run_id: String,
    job_id: &str,
    job_display_name: &str,
    started_at: chrono::DateTime<Utc>,
    outcome: &JobOutcome,
) {
    let s = Arc::clone(store);
    let run = RunRecord {
        id: run_id,
        job_id: job_id.to_string(),
        job_name: job_display_name.to_string(),
        started_at,
        finished_at: Some(outcome.finished_at),
        exit_code: outcome.exit_code,
        stdout: outcome.stdout.clone(),
        stderr: outcome.stderr.clone(),
        status: outcome.status,
    };
    let result = tokio::task::spawn_blocking(move || {
        let store = s.lock().unwrap_or_else(|e| {
            warn!("store mutex was poisoned, recovering");
            e.into_inner()
        });
        store.update_run(&run)
    })
    .await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => error!("failed to record run result: {e}"),
        Err(e) => error!("spawn_blocking panicked recording run result: {e}"),
    }
}

async fn handle_once_removal(job_id: &str, job_display_name: &str) {
    info!(job = %job_display_name, "one-time job completed, removing job file");
    let jid = job_id.to_string();
    let _ = tokio::task::spawn_blocking(move || {
        if let Err(e) = config::delete_job_file(&jid) {
            warn!(job = %jid, "failed to remove one-time job file: {e}");
        }
    })
    .await;
    let _ = signal_daemon_reload();
}

const ALERT_STDERR_MAX: usize = 500;

async fn dispatch_alerts(
    alert: &config::JobAlert,
    outcome: &JobOutcome,
    job_display_name: &str,
    command: &str,
) {
    let should_alert = match outcome.status {
        RunStatus::Failed => alert.on_failure,
        RunStatus::Success => alert.on_success,
        _ => false,
    };
    if !should_alert {
        return;
    }

    let alert_config = match tokio::task::spawn_blocking(config::load_alerts).await {
        Ok(Ok(cfg)) if !cfg.provider.is_empty() => cfg,
        Ok(Err(e)) => {
            warn!("failed to load alert config: {e}");
            return;
        }
        _ => return,
    };

    let display_name = job_display_name.to_string();
    let cmd = command.to_string();
    let end = outcome.stderr.floor_char_boundary(ALERT_STDERR_MAX);
    let stderr_snippet = &outcome.stderr[..end];
    let subject = match outcome.status {
        RunStatus::Failed => format!("kron job failed: {display_name}"),
        _ => format!("kron job succeeded: {display_name}"),
    };
    let body = format!(
        "Command: {cmd}\nExit code: {}\nStderr: {stderr_snippet}",
        outcome
            .exit_code
            .map_or_else(|| "unknown".to_string(), |c| c.to_string())
    );
    tokio::spawn(async move {
        crate::notify::notify_all(&alert_config.provider, &subject, &body).await;
    });
}

/// Compute how long to sleep until the next job is due.
/// Returns at most `MAX_SLEEP_SECS` to ensure periodic config reload.
fn compute_next_sleep(jobs: &[CachedJob], now: &chrono::DateTime<Local>) -> Duration {
    let max_sleep = Duration::from_secs(MAX_SLEEP_SECS);
    let mut earliest = max_sleep;

    for cached in jobs {
        if !cached.def.enabled {
            continue;
        }
        match cached.cron.find_next_occurrence(now, false) {
            Ok(next) => {
                let until = (next - *now).to_std().unwrap_or(Duration::ZERO);
                if until < earliest {
                    earliest = until;
                }
            }
            Err(e) => {
                let job_label = cached.def.name.as_deref().unwrap_or(&cached.def.id);
                warn!(job = %job_label, "failed to compute next occurrence: {e}");
            }
        }
    }

    // Add a small buffer (1 second) to ensure we land inside the target minute
    // rather than waking up a fraction of a second early.
    if earliest > Duration::ZERO && earliest < max_sleep {
        earliest = earliest.saturating_add(Duration::from_secs(1));
    }

    // Never sleep less than 1 second to avoid busy-looping
    earliest.max(Duration::from_secs(1))
}

/// RAII guard that removes a job ID from `running_jobs` when dropped.
struct RunningGuard {
    running_jobs: Arc<Mutex<HashSet<String>>>,
    name: String,
}

impl Drop for RunningGuard {
    fn drop(&mut self) {
        let mut running = self.running_jobs.lock().unwrap_or_else(|e| {
            warn!("running_jobs mutex was poisoned, recovering");
            e.into_inner()
        });
        running.remove(&self.name);
    }
}

/// Parse a duration string. Supports bare seconds or suffixed "s", "m", "h".
/// Returns `None` if the string cannot be parsed, and logs a warning.
pub fn parse_duration(s: &str) -> Option<Duration> {
    if let Some(rest) = s.strip_suffix('h')
        && let Ok(n) = rest.parse::<u64>()
    {
        return Some(Duration::from_secs(n * 3600));
    }
    if let Some(rest) = s.strip_suffix('m')
        && let Ok(n) = rest.parse::<u64>()
    {
        return Some(Duration::from_secs(n * 60));
    }
    if let Some(rest) = s.strip_suffix('s')
        && let Ok(n) = rest.parse::<u64>()
    {
        return Some(Duration::from_secs(n));
    }
    if let Ok(n) = s.parse::<u64>() {
        return Some(Duration::from_secs(n));
    }
    warn!(value = %s, "invalid timeout value, ignoring (no timeout will be applied)");
    None
}

/// Reload all job configs from disk (async wrapper for `spawn_blocking`).
async fn reload_jobs_async() -> Option<Vec<CachedJob>> {
    tokio::task::spawn_blocking(reload_jobs)
        .await
        .ok()
        .flatten()
}

/// Reload all job configs from disk.
/// Returns `Some(jobs)` on success (even if the list is empty — that means zero job files).
/// Returns `None` on failure so the caller can keep the previous cache.
fn reload_jobs() -> Option<Vec<CachedJob>> {
    let mut cache = Vec::new();
    match config::load_all_jobs() {
        Ok(configs) => {
            for config in configs {
                let def = config.job;
                match Cron::new(&def.schedule).parse() {
                    Ok(cron) => cache.push(CachedJob { def, cron }),
                    Err(e) => {
                        let job_label = def.name.as_deref().unwrap_or(&def.id);
                        warn!(job = %job_label, "invalid schedule, skipping: {e}");
                    }
                }
            }
            Some(cache)
        }
        Err(e) => {
            warn!("failed to load job configs: {e}");
            None
        }
    }
}

/// Send SIGHUP to the running daemon to trigger a config reload.
/// Returns true if the signal was sent successfully.
#[must_use]
pub fn signal_daemon_reload() -> bool {
    let pid_path = config::data_dir().join("daemon.pid");
    let Ok(pid_str) = std::fs::read_to_string(&pid_path) else {
        return false;
    };
    let Ok(pid) = pid_str.trim().parse::<u32>() else {
        return false;
    };
    std::process::Command::new("kill")
        .args(["-HUP", &pid.to_string()])
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use kron_store::Store;
    use tokio_util::sync::CancellationToken;

    use super::*;

    #[tokio::test]
    async fn test_scheduler_starts_and_shuts_down() {
        let store = Store::open_in_memory().unwrap();
        let cancel = CancellationToken::new();
        let scheduler = Scheduler::new(store, cancel.clone());

        let handle = tokio::spawn(async move { scheduler.run().await });

        // Allow initial setup
        tokio::time::sleep(Duration::from_millis(100)).await;
        cancel.cancel();

        let result = handle.await.unwrap();
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_duration_seconds() {
        assert_eq!(parse_duration("30"), Some(Duration::from_secs(30)));
        assert_eq!(parse_duration("30s"), Some(Duration::from_secs(30)));
    }

    #[test]
    fn test_parse_duration_minutes() {
        assert_eq!(parse_duration("5m"), Some(Duration::from_secs(300)));
    }

    #[test]
    fn test_parse_duration_hours() {
        assert_eq!(parse_duration("1h"), Some(Duration::from_secs(3600)));
    }

    #[test]
    fn test_parse_duration_invalid() {
        assert_eq!(parse_duration("invalid"), None);
        assert_eq!(parse_duration(""), None);
    }

    #[test]
    fn test_compute_next_sleep_no_jobs() {
        let now = Local::now();
        let sleep = compute_next_sleep(&[], &now);
        assert_eq!(sleep.as_secs(), MAX_SLEEP_SECS);
    }

    #[test]
    fn test_compute_next_sleep_capped() {
        // A yearly job should still cap at MAX_SLEEP_SECS
        let cron = Cron::new("0 0 1 1 *").parse().unwrap();
        let jobs = vec![CachedJob {
            def: JobDefinition {
                id: "test".to_string(),
                name: None,
                command: "echo hi".to_string(),
                schedule: "0 0 1 1 *".to_string(),
                working_dir: None,
                enabled: true,
                timeout: None,
                env: None,
                alert: None,
                once: false,
            },
            cron,
        }];
        let now = Local::now();
        let sleep = compute_next_sleep(&jobs, &now);
        assert!(sleep.as_secs() <= MAX_SLEEP_SECS);
    }

    #[test]
    fn test_compute_next_sleep_disabled_job_ignored() {
        let cron = Cron::new("* * * * *").parse().unwrap();
        let jobs = vec![CachedJob {
            def: JobDefinition {
                id: "test".to_string(),
                name: None,
                command: "echo hi".to_string(),
                schedule: "* * * * *".to_string(),
                working_dir: None,
                enabled: false,
                timeout: None,
                env: None,
                alert: None,
                once: false,
            },
            cron,
        }];
        let now = Local::now();
        let sleep = compute_next_sleep(&jobs, &now);
        // Disabled job ignored, falls back to max sleep
        assert_eq!(sleep.as_secs(), MAX_SLEEP_SECS);
    }

    #[test]
    fn test_silence_alerted_starts_empty() {
        let store = Store::open_in_memory().unwrap();
        let cancel = CancellationToken::new();
        let scheduler = Scheduler::new(store, cancel);
        let alerted = scheduler.silence_alerted.lock().unwrap();
        assert!(alerted.is_empty());
    }

    #[tokio::test]
    async fn test_dispatch_alerts_skips_when_on_failure_false() {
        // on_failure=false with Failed outcome: returns immediately, no network access.
        let alert = crate::config::JobAlert {
            on_failure: false,
            on_success: false,
            on_silence: None,
        };
        let outcome = JobOutcome {
            finished_at: Utc::now(),
            exit_code: Some(1),
            stdout: String::new(),
            stderr: String::new(),
            status: RunStatus::Failed,
        };
        dispatch_alerts(&alert, &outcome, "test-job", "echo hi").await;
    }

    #[tokio::test]
    async fn test_dispatch_alerts_skips_when_on_success_false() {
        // on_success=false with Success outcome: returns immediately, no network access.
        let alert = crate::config::JobAlert {
            on_failure: false,
            on_success: false,
            on_silence: None,
        };
        let outcome = JobOutcome {
            finished_at: Utc::now(),
            exit_code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
            status: RunStatus::Success,
        };
        dispatch_alerts(&alert, &outcome, "test-job", "echo hi").await;
    }

    #[tokio::test]
    async fn test_dispatch_alerts_on_failure_true_no_providers_returns_cleanly() {
        // on_failure=true with Failed outcome: attempts to load alert config.
        // In test env, config load fails or returns no providers — returns without panic.
        let alert = crate::config::JobAlert {
            on_failure: true,
            on_success: false,
            on_silence: None,
        };
        let outcome = JobOutcome {
            finished_at: Utc::now(),
            exit_code: Some(1),
            stdout: String::new(),
            stderr: String::new(),
            status: RunStatus::Failed,
        };
        dispatch_alerts(&alert, &outcome, "test-job", "echo hi").await;
    }

    #[tokio::test]
    async fn test_dispatch_alerts_on_success_true_no_providers_returns_cleanly() {
        // on_success=true with Success outcome: attempts alert, returns cleanly without providers.
        let alert = crate::config::JobAlert {
            on_failure: false,
            on_success: true,
            on_silence: None,
        };
        let outcome = JobOutcome {
            finished_at: Utc::now(),
            exit_code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
            status: RunStatus::Success,
        };
        dispatch_alerts(&alert, &outcome, "test-job", "echo hi").await;
    }

    #[test]
    fn test_is_time_matching_any_second_in_minute() {
        // croner's is_time_matching only returns true at second 0.
        // Verify that our truncation strategy makes matching work at any second.
        use chrono::{NaiveDate, NaiveDateTime, NaiveTime, TimeZone};

        let cron = Cron::new("30 8 * * 1-5").parse().unwrap();
        let date = NaiveDate::from_ymd_opt(2026, 3, 23).unwrap(); // Monday

        for sec in [0, 1, 15, 30, 59] {
            let t = Local
                .from_local_datetime(&NaiveDateTime::new(
                    date,
                    NaiveTime::from_hms_opt(8, 30, sec).unwrap(),
                ))
                .unwrap();

            // Raw check only passes at second 0
            if sec == 0 {
                assert!(cron.is_time_matching(&t).unwrap());
            } else {
                assert!(!cron.is_time_matching(&t).unwrap());
            }

            // Truncated check passes for every second in the minute
            let truncated = t.with_second(0).unwrap().with_nanosecond(0).unwrap();
            assert!(
                cron.is_time_matching(&truncated).unwrap(),
                "truncated 08:30:{sec:02} should match"
            );
        }
    }
}
