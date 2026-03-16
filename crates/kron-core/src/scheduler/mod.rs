use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::Utc;
use croner::Cron;
use kron_store::{RunRecord, RunStatus, Store};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, error, info, info_span, warn};
use uuid::Uuid;

use crate::config::{self, JobDefinition};
use crate::error::CoreError;
use crate::runner;

/// Parsed and cached job state to avoid re-reading filesystem and re-parsing cron every tick.
struct CachedJob {
    def: JobDefinition,
    cron: Cron,
}

pub struct Scheduler {
    store: Arc<Mutex<Store>>,
    cancel: CancellationToken,
    running_jobs: Arc<Mutex<HashSet<String>>>,
}

impl Scheduler {
    pub fn new(store: Store, cancel: CancellationToken) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
            cancel,
            running_jobs: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Run the scheduler loop. Checks every second for jobs that need to run.
    ///
    /// # Errors
    /// Returns `CoreError` if a fatal scheduling error occurs.
    pub async fn run(&self) -> Result<(), CoreError> {
        info!("scheduler started");

        let mut last_check = HashMap::<String, chrono::DateTime<Utc>>::new();
        let mut last_reload = Instant::now();

        // Load jobs initially
        let mut cached_jobs: Vec<CachedJob> = tokio::task::spawn_blocking(reload_jobs)
            .await
            .ok()
            .flatten()
            .unwrap_or_default();

        loop {
            tokio::select! {
                () = self.cancel.cancelled() => {
                    info!("scheduler shutting down");
                    return Ok(());
                }
                () = tokio::time::sleep(Duration::from_secs(1)) => {
                    // Reload job configs every 10 seconds instead of every tick
                    if last_reload.elapsed() >= Duration::from_secs(10) {
                        let new_jobs = tokio::task::spawn_blocking(reload_jobs)
                            .await
                            .ok()
                            .flatten();
                        // Reload failure returns None — keep the old cache.
                        if let Some(jobs) = new_jobs {
                            cached_jobs = jobs;
                        }
                        last_reload = Instant::now();
                    }
                    self.tick(&cached_jobs, &mut last_check);
                }
            }
        }
    }

    fn tick(&self, jobs: &[CachedJob], last_check: &mut HashMap<String, chrono::DateTime<Utc>>) {
        let now = Utc::now();
        let now_minute = now.format("%Y-%m-%d %H:%M").to_string();

        for cached in jobs {
            let def = &cached.def;
            if !def.enabled {
                continue;
            }

            // Use job ID as the key for last_check and running_jobs tracking
            let last = last_check
                .get(&def.id)
                .copied()
                .unwrap_or(chrono::DateTime::<Utc>::UNIX_EPOCH);
            let last_minute = last.format("%Y-%m-%d %H:%M").to_string();

            if now_minute != last_minute {
                // Skip if job is already running
                {
                    let running = self.running_jobs.lock().unwrap_or_else(|e| {
                        warn!("running_jobs mutex was poisoned, recovering");
                        e.into_inner()
                    });
                    if running.contains(&def.id) {
                        let job_label = def.name.as_deref().unwrap_or(&def.id);
                        info!(job = %job_label, "job already running, skipping tick");
                        last_check.insert(def.id.clone(), now);
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

            last_check.insert(def.id.clone(), now);
        }
    }

    #[allow(clippy::too_many_lines)]
    fn spawn_job(&self, def: &JobDefinition) {
        let store = Arc::clone(&self.store);
        let running_jobs = Arc::clone(&self.running_jobs);
        let job_id = def.id.clone();
        // Use name for display; fall back to id
        let job_display_name = def.name.clone().unwrap_or_else(|| def.id.clone());
        let command = def.command.clone();
        let working_dir = def.working_dir.clone();
        let timeout = def.timeout.as_deref().and_then(parse_duration);
        let env_vars = def.env.clone();
        let job_alert = def.alert.clone();

        let job_id_clone = job_id.clone();
        let span_name = job_display_name.clone();
        tokio::spawn(
            async move {
                // Mark job as running by ID
                {
                    let mut running = running_jobs.lock().unwrap_or_else(|e| {
                        warn!("running_jobs mutex was poisoned, recovering");
                        e.into_inner()
                    });
                    running.insert(job_id.clone());
                }

                // Ensure we remove the job from running_jobs when done
                let _guard = RunningGuard {
                    running_jobs: Arc::clone(&running_jobs),
                    name: job_id.clone(),
                };

                let run_id = Uuid::new_v4().to_string();
                let started_at = Utc::now();

                // Insert run record — job_id is the short ID; job_name is the display name
                {
                    let s = Arc::clone(&store);
                    let run = RunRecord {
                        id: run_id.clone(),
                        job_id: job_id_clone.clone(),
                        job_name: job_display_name.clone(),
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

                // Execute with optional timeout
                let result = runner::execute_command(
                    &command,
                    working_dir.as_deref(),
                    timeout,
                    env_vars.as_ref(),
                )
                .await;

                // Record result
                let (finished_at, exit_code, stdout, stderr, status) = match result {
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
                        (
                            output.finished_at,
                            output.exit_code,
                            output.stdout,
                            output.stderr,
                            status,
                        )
                    }
                    Err(CoreError::Timeout(dur)) => {
                        warn!(job = %job_display_name, timeout = ?dur, "job timed out");
                        (
                            Utc::now(),
                            None,
                            String::new(),
                            format!("timed out after {dur:?}"),
                            RunStatus::Failed,
                        )
                    }
                    Err(e) => {
                        error!("job execution failed: {e}");
                        (
                            Utc::now(),
                            None,
                            String::new(),
                            e.to_string(),
                            RunStatus::Failed,
                        )
                    }
                };

                // Retain copies needed for alert dispatch before values are moved into RunRecord
                let status_for_alert = status.clone();
                let stderr_for_alert = stderr.clone();

                // Update run record
                {
                    let s = Arc::clone(&store);
                    let run = RunRecord {
                        id: run_id,
                        job_id: job_id_clone,
                        job_name: job_display_name.clone(),
                        started_at,
                        finished_at: Some(finished_at),
                        exit_code,
                        stdout,
                        stderr,
                        status,
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

                // Fire alerts based on per-job alert settings
                if let Some(ref alert) = job_alert {
                    let should_alert = match status_for_alert {
                        RunStatus::Failed => alert.on_failure,
                        RunStatus::Success => alert.on_success,
                        RunStatus::Running => false,
                    };
                    if should_alert {
                        match crate::config::load_alerts() {
                            Ok(alert_config) if !alert_config.provider.is_empty() => {
                                let display_name = job_display_name.clone();
                                let cmd = command.clone();
                                let stderr_snippet = if stderr_for_alert.len() > 500 {
                                    stderr_for_alert[..500].to_string()
                                } else {
                                    stderr_for_alert.clone()
                                };
                                let subject = match status_for_alert {
                                    RunStatus::Failed => {
                                        format!("kron job failed: {display_name}")
                                    }
                                    RunStatus::Success | RunStatus::Running => {
                                        format!("kron job succeeded: {display_name}")
                                    }
                                };
                                let body = format!(
                                    "Command: {cmd}\nExit code: {}\nStderr: {stderr_snippet}",
                                    exit_code
                                        .map_or_else(|| "unknown".to_string(), |c| c.to_string())
                                );
                                tokio::spawn(async move {
                                    crate::notify::notify_all(
                                        &alert_config.provider,
                                        &subject,
                                        &body,
                                    )
                                    .await;
                                });
                            }
                            Ok(_) => {}
                            Err(e) => warn!("failed to load alert config: {e}"),
                        }
                    }
                }
            }
            .instrument(info_span!("job_run", job = %span_name)),
        );
    }
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

        // Allow a tick or two
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
}
