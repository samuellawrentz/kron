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
                        cached_jobs = tokio::task::spawn_blocking(reload_jobs)
                            .await
                            .unwrap_or_default();
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

            // Issue 2: use UNIX_EPOCH as default so the first tick always evaluates
            let last = last_check
                .get(&def.name)
                .copied()
                .unwrap_or(chrono::DateTime::<Utc>::UNIX_EPOCH);
            let last_minute = last.format("%Y-%m-%d %H:%M").to_string();

            if now_minute != last_minute {
                // Issue 12: skip if job is already running
                {
                    let running = self.running_jobs.lock().unwrap_or_else(|e| {
                        warn!("running_jobs mutex was poisoned, recovering");
                        e.into_inner()
                    });
                    if running.contains(&def.name) {
                        info!(job = %def.name, "job already running, skipping tick");
                        last_check.insert(def.name.clone(), now);
                        continue;
                    }
                }

                match cached.cron.is_time_matching(&now) {
                    Ok(true) => {
                        info!(job = %def.name, "triggering job");
                        self.spawn_job(def);
                    }
                    Ok(false) => {}
                    Err(e) => {
                        warn!(job = %def.name, "schedule match error: {e}");
                    }
                }
            }

            last_check.insert(def.name.clone(), now);
        }
    }

    #[allow(clippy::too_many_lines)]
    fn spawn_job(&self, def: &JobDefinition) {
        let store = Arc::clone(&self.store);
        let running_jobs = Arc::clone(&self.running_jobs);
        let name = def.name.clone();
        let command = def.command.clone();
        let working_dir = def.working_dir.clone();
        let timeout = def.timeout.as_deref().and_then(parse_duration);

        let name_clone = name.clone();
        let span_name = name.clone();
        tokio::spawn(
            async move {
                // Mark job as running (Issue 12)
                {
                    let mut running = running_jobs.lock().unwrap_or_else(|e| {
                        warn!("running_jobs mutex was poisoned, recovering");
                        e.into_inner()
                    });
                    running.insert(name.clone());
                }

                // Ensure we remove the job from running_jobs when done
                let _guard = RunningGuard {
                    running_jobs: Arc::clone(&running_jobs),
                    name: name.clone(),
                };

                // job_id is the job name (TOML is source of truth, no jobs table)
                let job_id = name_clone.clone();
                let run_id = Uuid::new_v4().to_string();
                let started_at = Utc::now();

                // Issue 1 + 6: insert_run via spawn_blocking with poison warning
                {
                    let s = Arc::clone(&store);
                    let run = RunRecord {
                        id: run_id.clone(),
                        job_id: job_id.clone(),
                        job_name: name.clone(),
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

                // Execute with optional timeout (Issue 10)
                let result =
                    runner::execute_command(&command, working_dir.as_deref(), timeout).await;

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
                        warn!(job = %name, timeout = ?dur, "job timed out");
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

                // Issue 1 + 6: update_run via spawn_blocking with poison warning
                {
                    let s = Arc::clone(&store);
                    let run = RunRecord {
                        id: run_id,
                        job_id,
                        job_name: name.clone(),
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
            }
            .instrument(info_span!("job_run", job = %span_name)),
        );
    }
}

/// RAII guard that removes a job name from `running_jobs` when dropped.
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
fn parse_duration(s: &str) -> Option<Duration> {
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

fn reload_jobs() -> Vec<CachedJob> {
    let mut cache = Vec::new();
    match config::load_all_jobs() {
        Ok(configs) => {
            for config in configs {
                let def = config.job;
                match Cron::new(&def.schedule).parse() {
                    Ok(cron) => cache.push(CachedJob { def, cron }),
                    Err(e) => warn!(job = %def.name, "invalid schedule, skipping: {e}"),
                }
            }
        }
        Err(e) => {
            warn!("failed to load job configs: {e}");
        }
    }
    cache
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
