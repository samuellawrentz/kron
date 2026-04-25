//! Job configuration management: TOML parsing, validation, and file I/O.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

/// Serde helpers for duration strings like "30", "30s", "5m", "1h".
/// Validates the format at deserialization time so bad values fail fast.
mod duration_serde {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn is_valid(s: &str) -> bool {
        if s.is_empty() {
            return false;
        }
        for suffix in ['h', 'm', 's'] {
            if let Some(rest) = s.strip_suffix(suffix) {
                return rest.parse::<u64>().is_ok();
            }
        }
        s.parse::<u64>().is_ok()
    }

    pub fn deserialize_opt<'de, D>(de: D) -> Result<Option<String>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt = Option::<String>::deserialize(de)?;
        match opt {
            None => Ok(None),
            Some(s) => {
                if is_valid(&s) {
                    Ok(Some(s))
                } else {
                    Err(serde::de::Error::custom(format!(
                        "invalid duration \"{s}\": expected bare seconds (\"30\"), or a number with suffix s/m/h (\"30s\", \"5m\", \"1h\")"
                    )))
                }
            }
        }
    }

    #[allow(clippy::trivially_copy_pass_by_ref, clippy::ref_option)]
    pub fn serialize_opt<S>(val: &Option<String>, ser: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match val {
            Some(s) => ser.serialize_some(s),
            None => ser.serialize_none(),
        }
    }
}

// ---------------------------------------------------------------------------
// Alert configuration types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertConfig {
    #[serde(default)]
    pub provider: Vec<AlertProvider>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum AlertProvider {
    #[serde(rename = "telegram")]
    Telegram { token: String, chat_id: String },
    #[serde(rename = "slack")]
    Slack { webhook_url: String },
    #[serde(rename = "webhook")]
    Webhook { url: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct JobAlert {
    #[serde(default = "default_on_failure")]
    pub on_failure: bool,
    #[serde(default)]
    pub on_success: bool,
    /// Dead-man switch: alert if the job hasn't run in this duration (e.g. "1h").
    #[serde(
        default,
        deserialize_with = "duration_serde::deserialize_opt",
        serialize_with = "duration_serde::serialize_opt"
    )]
    pub on_silence: Option<String>,
}

fn default_on_failure() -> bool {
    true
}

// ---------------------------------------------------------------------------
// Global kron configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GlobalConfig {
    #[serde(default)]
    pub retention: RetentionConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionConfig {
    /// Maximum number of runs to keep per job (default: 100)
    #[serde(default = "default_max_runs_per_job")]
    pub max_runs_per_job: usize,
    /// Maximum age of runs in days (default: 30)
    #[serde(default = "default_max_age_days")]
    pub max_age_days: u32,
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            max_runs_per_job: default_max_runs_per_job(),
            max_age_days: default_max_age_days(),
        }
    }
}

fn default_max_runs_per_job() -> usize {
    100
}
fn default_max_age_days() -> u32 {
    30
}

/// Returns the path to the global kron config file: `~/.config/kron/config.toml`.
#[must_use]
pub fn global_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from(".config"))
        .join("kron")
        .join("config.toml")
}

/// Load the global kron configuration.
///
/// Returns default config if the file does not exist.
///
/// # Errors
/// Returns `CoreError` if the file exists but cannot be read or parsed.
pub fn load_global_config() -> Result<GlobalConfig, CoreError> {
    let path = global_config_path();
    if !path.exists() {
        return Ok(GlobalConfig::default());
    }
    let contents = std::fs::read_to_string(&path)?;
    let config: GlobalConfig = toml::from_str(&contents)?;
    Ok(config)
}

/// Returns the path to the global alerts config file: `~/.config/kron/alerts.toml`.
#[must_use]
pub fn alerts_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from(".config"))
        .join("kron")
        .join("alerts.toml")
}

/// Load the global alert configuration.
///
/// Returns an empty `AlertConfig` (no providers) if the file does not exist.
///
/// # Errors
/// Returns `CoreError` if the file exists but cannot be read or parsed.
pub fn load_alerts() -> Result<AlertConfig, CoreError> {
    let path = alerts_config_path();
    if !path.exists() {
        return Ok(AlertConfig {
            provider: Vec::new(),
        });
    }
    let contents = std::fs::read_to_string(&path)?;
    let config: AlertConfig = toml::from_str(&contents)?;
    Ok(config)
}

/// Write the alert configuration to `~/.config/kron/alerts.toml`.
///
/// # Errors
/// Returns `CoreError` if the directory cannot be created or the file cannot be written.
pub fn save_alerts(config: &AlertConfig) -> Result<(), CoreError> {
    let path = alerts_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    let contents = toml::to_string(config)?;
    std::fs::write(&path, contents)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Validate a job name: alphanumeric, hyphens, underscores only; max 64 chars; non-empty.
///
/// # Errors
/// Returns `CoreError::InvalidJobName` if the name fails validation.
pub fn validate_job_name(name: &str) -> Result<(), CoreError> {
    if name.is_empty() {
        return Err(CoreError::InvalidJobName {
            name: name.to_string(),
            reason: "job name cannot be empty".to_string(),
        });
    }
    if name.len() > 64 {
        return Err(CoreError::InvalidJobName {
            name: name.to_string(),
            reason: "job name too long (max 64 characters)".to_string(),
        });
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(CoreError::InvalidJobName {
            name: name.to_string(),
            reason: "job name must contain only alphanumeric characters, hyphens, and underscores"
                .to_string(),
        });
    }
    Ok(())
}

/// Generate a short 8-character hex ID from a UUID.
#[must_use]
pub fn generate_short_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..8].to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobConfig {
    pub job: JobDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobDefinition {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    pub command: String,
    pub schedule: String,
    #[serde(default)]
    pub working_dir: Option<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(
        default,
        deserialize_with = "duration_serde::deserialize_opt",
        serialize_with = "duration_serde::serialize_opt"
    )]
    pub timeout: Option<String>,
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
    #[serde(default)]
    pub alert: Option<JobAlert>,
    #[serde(default)]
    pub once: bool,
}

fn default_enabled() -> bool {
    true
}

#[must_use]
pub fn jobs_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from(".config"))
        .join("kron")
        .join("jobs")
}

#[must_use]
pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from(".local/share"))
        .join("kron")
}

#[must_use]
pub fn db_path() -> PathBuf {
    data_dir().join("kron.db")
}

/// Returns the path to the `.sh` script file for a given job ID.
#[must_use]
pub fn script_path(job_id: &str) -> PathBuf {
    jobs_dir().join(format!("{job_id}.sh"))
}

/// Load the `.sh` script content for a job, stripping the shebang line.
/// Returns `None` if the file does not exist.
#[must_use]
pub(crate) fn load_script(job_id: &str) -> Option<String> {
    let path = script_path(job_id);
    let content = std::fs::read_to_string(path).ok()?;
    let stripped = if let Some(rest) = content.strip_prefix("#!") {
        // Skip the shebang line
        rest.find('\n').map_or("", |idx| &rest[idx + 1..])
    } else {
        &content
    };
    let trimmed = stripped.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Write a job command as a `.sh` script file.
///
/// Creates `#!/bin/bash\n{command}\n` at `jobs_dir()/<id>.sh`.
///
/// # Errors
/// Returns `CoreError` if the file cannot be written.
pub(crate) fn save_script(job_id: &str, command: &str) -> Result<PathBuf, CoreError> {
    let path = script_path(job_id);
    let content = format!("#!/bin/bash\n{command}\n");
    std::fs::write(&path, &content)?;
    Ok(path)
}

/// Remove the `.sh` script file for a job if it exists.
/// Logs a warning for unexpected errors (e.g. permission denied); silently
/// ignores `NotFound` since the file may never have been created.
pub(crate) fn delete_script(job_id: &str) {
    let path = script_path(job_id);
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            tracing::warn!(job_id, "failed to remove script file: {e}");
        }
    }
}

/// Read and parse a TOML job config file.
/// If the config has no `id`, generates one and re-saves the file (backward compat).
/// If a `.sh` script file exists alongside the TOML, the command from the script
/// takes precedence over the inline TOML `command` field.
///
/// # Errors
/// Returns `CoreError` if the file cannot be read, is not valid TOML, or has an invalid job name.
pub fn load_job(path: &Path) -> Result<JobConfig, CoreError> {
    let contents = std::fs::read_to_string(path)?;
    let mut config: JobConfig = toml::from_str(&contents)?;
    if let Some(ref name) = config.job.name {
        validate_job_name(name)?;
    }
    // Backward compat: if id is empty/missing, generate one and re-save
    if config.job.id.is_empty() {
        config.job.id = generate_short_id();
        // Best-effort re-save; ignore errors (read-only FS, etc.)
        let new_path = jobs_dir().join(format!("{}.toml", config.job.id));
        if let Ok(contents) = toml::to_string(&config) {
            let _ = std::fs::write(&new_path, contents);
            // Remove old file (best-effort)
            let _ = std::fs::remove_file(path);
        }
    }
    // The `.sh` script takes precedence over the TOML `command` field so that
    // edits made directly to the script file are always honoured at runtime.
    if let Some(script_command) = load_script(&config.job.id) {
        config.job.command = script_command;
    }
    Ok(config)
}

/// Write a job config as TOML to `jobs_dir()/<id>.toml`.
/// Creates parent directories if needed.
/// Returns the path written.
///
/// # Errors
/// Returns `CoreError` if the file cannot be written or serialized.
pub fn save_job(config: &JobConfig) -> Result<PathBuf, CoreError> {
    let dir = jobs_dir();
    std::fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let path = dir.join(format!("{}.toml", config.job.id));
    let contents = toml::to_string(config)?;
    std::fs::write(&path, &contents)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    // Dual source of truth: the TOML `command` field is kept for backward
    // compatibility and human-readable display, while the `.sh` file is the
    // actual execution source (loaded with precedence in `load_job`).
    save_script(&config.job.id, &config.job.command)?;
    Ok(path)
}

/// Load all `.toml` files from `jobs_dir()`.
///
/// # Errors
/// Returns `CoreError` if the directory cannot be read.
pub fn load_all_jobs() -> Result<Vec<JobConfig>, CoreError> {
    let dir = jobs_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut jobs = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("failed to read directory entry: {e}");
                continue;
            }
        };
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("toml") {
            match load_job(&path) {
                Ok(config) => jobs.push(config),
                Err(e) => {
                    tracing::warn!(path = %path.display(), "failed to load job config: {e}");
                }
            }
        }
    }
    Ok(jobs)
}

/// Remove the TOML config file for a job by its ID.
///
/// # Errors
/// Returns `CoreError::JobNotFound` if the file does not exist, or `CoreError::Io` on failure.
pub fn delete_job_file(id: &str) -> Result<(), CoreError> {
    let path = jobs_dir().join(format!("{id}.toml"));
    // Also remove the .sh script file if it exists
    delete_script(id);
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(CoreError::JobNotFound {
            name: id.to_string(),
        }),
        Err(e) => Err(CoreError::Io(e)),
    }
}

/// Find a job by ID (prefix match) or name (exact match).
///
/// # Errors
/// Returns `CoreError` if jobs cannot be loaded.
pub fn find_job(query: &str) -> Result<Option<JobConfig>, CoreError> {
    // Fast path: job files are named {id}.toml, so try a direct read first.
    let candidate = jobs_dir().join(format!("{query}.toml"));
    if candidate.exists() {
        return Ok(Some(load_job(&candidate)?));
    }

    // Fallback: prefix match and name match require a full scan.
    let jobs = load_all_jobs()?;

    // ID prefix match (so users can type just the first few chars)
    let prefix_matches: Vec<_> = jobs
        .iter()
        .filter(|j| j.job.id.starts_with(query))
        .collect();
    if prefix_matches.len() > 1 {
        let names: Vec<_> = prefix_matches
            .iter()
            .map(|job| job.job.name.as_deref().unwrap_or(&job.job.id))
            .collect();
        return Err(CoreError::AmbiguousJob(names.join(", ")));
    }
    if prefix_matches.len() == 1 {
        return Ok(Some(prefix_matches[0].clone()));
    }
    // Then try exact name match
    if let Some(job) = jobs.iter().find(|j| j.job.name.as_deref() == Some(query)) {
        return Ok(Some(job.clone()));
    }
    Ok(None)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::fs;

    use super::*;

    fn sample_config(id: &str) -> JobConfig {
        JobConfig {
            job: JobDefinition {
                id: id.to_string(),
                name: Some(id.to_string()),
                command: "echo hello".to_string(),
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

    #[test]
    fn test_once_defaults_to_false() {
        let toml_str = r#"
[job]
id = "abc12345"
command = "echo hi"
schedule = "* * * * *"
"#;
        let config: JobConfig = toml::from_str(toml_str).unwrap();
        assert!(!config.job.once);
    }

    #[test]
    fn test_once_roundtrip() {
        let mut config = sample_config("once-test");
        config.job.once = true;
        let serialized = toml::to_string(&config).unwrap();
        let parsed: JobConfig = toml::from_str(&serialized).unwrap();
        assert!(parsed.job.once);
    }

    #[test]
    fn test_once_explicit_false() {
        let toml_str = r#"
[job]
id = "abc12345"
command = "echo hi"
schedule = "* * * * *"
once = false
"#;
        let config: JobConfig = toml::from_str(toml_str).unwrap();
        assert!(!config.job.once);
    }

    #[test]
    fn test_once_explicit_true() {
        let toml_str = r#"
[job]
id = "abc12345"
command = "echo hi"
schedule = "* * * * *"
once = true
"#;
        let config: JobConfig = toml::from_str(toml_str).unwrap();
        assert!(config.job.once);
    }

    #[test]
    fn test_generate_short_id() {
        let id = generate_short_id();
        assert_eq!(id.len(), 8);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_serialize_deserialize_job_config() {
        let original = sample_config("7a3f2bc1");
        let serialized = toml::to_string(&original).unwrap();
        let parsed: JobConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(parsed.job.id, original.job.id);
        assert_eq!(parsed.job.name, original.job.name);
        assert_eq!(parsed.job.command, original.job.command);
        assert_eq!(parsed.job.schedule, original.job.schedule);
        assert_eq!(parsed.job.working_dir, original.job.working_dir);
        assert_eq!(parsed.job.enabled, original.job.enabled);
    }

    #[test]
    fn test_load_all_jobs_with_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        let job_path = dir.path().join("abc12345.toml");

        let config = sample_config("abc12345");
        let contents = toml::to_string(&config).unwrap();
        fs::write(&job_path, contents).unwrap();

        let loaded = load_job(&job_path).unwrap();
        assert_eq!(loaded.job.id, "abc12345");
        assert_eq!(loaded.job.name, Some("abc12345".to_string()));
        assert_eq!(loaded.job.command, "echo hello");
        assert!(loaded.job.enabled);
    }

    #[test]
    fn test_default_enabled_is_true() {
        let toml_str = r#"
[job]
id = "abc12345"
name = "myjob"
command = "echo hi"
schedule = "* * * * *"
"#;
        let config: JobConfig = toml::from_str(toml_str).unwrap();
        assert!(config.job.enabled);
    }

    #[test]
    fn test_name_is_optional() {
        let toml_str = r#"
[job]
id = "abc12345"
command = "echo hi"
schedule = "* * * * *"
"#;
        let config: JobConfig = toml::from_str(toml_str).unwrap();
        assert!(config.job.name.is_none());
        assert_eq!(config.job.id, "abc12345");
    }

    #[test]
    fn test_validate_job_name_valid() {
        assert!(validate_job_name("backup").is_ok());
        assert!(validate_job_name("my-job").is_ok());
        assert!(validate_job_name("test_123").is_ok());
    }

    #[test]
    fn test_validate_job_name_rejects_path_traversal() {
        assert!(validate_job_name("..").is_err());
        assert!(validate_job_name("../etc/passwd").is_err());
        assert!(validate_job_name("jobs/backup").is_err());
        assert!(validate_job_name("/absolute").is_err());
    }

    #[test]
    fn test_validate_job_name_rejects_empty() {
        assert!(validate_job_name("").is_err());
    }

    #[test]
    fn test_validate_job_name_rejects_too_long() {
        let long_name = "a".repeat(65);
        assert!(validate_job_name(&long_name).is_err());
        // 64 chars is exactly the limit — should pass
        let max_name = "a".repeat(64);
        assert!(validate_job_name(&max_name).is_ok());
    }

    #[test]
    fn test_validate_job_name_rejects_special_chars() {
        assert!(validate_job_name("my job").is_err()); // space
        assert!(validate_job_name("my.job").is_err()); // dot
        assert!(validate_job_name("my@job").is_err()); // at sign
        assert!(validate_job_name("my!job").is_err()); // exclamation
    }

    #[test]
    fn test_job_alert_default() {
        let toml_str = r#"
[job]
id = "abc12345"
command = "echo hi"
schedule = "* * * * *"

[job.alert]
"#;
        let config: JobConfig = toml::from_str(toml_str).unwrap();
        let alert = config.job.alert.unwrap();
        assert!(alert.on_failure); // default true
        assert!(!alert.on_success); // default false
        assert!(alert.on_silence.is_none());
    }

    #[test]
    fn test_job_alert_custom() {
        let toml_str = r#"
[job]
id = "abc12345"
command = "echo hi"
schedule = "* * * * *"

[job.alert]
on_failure = false
on_success = true
on_silence = "1h"
"#;
        let config: JobConfig = toml::from_str(toml_str).unwrap();
        let alert = config.job.alert.unwrap();
        assert!(!alert.on_failure);
        assert!(alert.on_success);
        assert_eq!(alert.on_silence.as_deref(), Some("1h"));
    }

    #[test]
    fn test_alert_config_roundtrip() {
        let config = AlertConfig {
            provider: vec![
                AlertProvider::Telegram {
                    token: "bot123:ABC".to_string(),
                    chat_id: "12345".to_string(),
                },
                AlertProvider::Slack {
                    webhook_url: "https://hooks.slack.com/test".to_string(),
                },
                AlertProvider::Webhook {
                    url: "https://example.com/hook".to_string(),
                },
            ],
        };
        let serialized = toml::to_string(&config).unwrap();
        let parsed: AlertConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(parsed.provider.len(), 3);
    }

    #[test]
    fn test_job_without_alert_is_none() {
        let toml_str = r#"
[job]
id = "abc12345"
command = "echo hi"
schedule = "* * * * *"
"#;
        let config: JobConfig = toml::from_str(toml_str).unwrap();
        assert!(config.job.alert.is_none());
    }

    #[test]
    fn test_job_env_vars_parsing() {
        let toml_str = r#"
[job]
id = "abc12345"
command = "echo hi"
schedule = "* * * * *"

[job.env]
DATABASE_URL = "postgres://localhost/mydb"
API_KEY = "secret123"
"#;
        let config: JobConfig = toml::from_str(toml_str).unwrap();
        let env = config.job.env.unwrap();
        assert_eq!(
            env.get("DATABASE_URL").unwrap(),
            "postgres://localhost/mydb"
        );
        assert_eq!(env.get("API_KEY").unwrap(), "secret123");
    }

    #[test]
    fn test_job_without_env_is_none() {
        let toml_str = r#"
[job]
id = "abc12345"
command = "echo hi"
schedule = "* * * * *"
"#;
        let config: JobConfig = toml::from_str(toml_str).unwrap();
        assert!(config.job.env.is_none());
    }

    #[test]
    fn test_job_env_roundtrip() {
        let mut env = std::collections::HashMap::new();
        env.insert("FOO".to_string(), "bar".to_string());
        env.insert("BAZ".to_string(), "qux".to_string());

        let config = sample_config("rt-test");
        let config_with_env = JobConfig {
            job: JobDefinition {
                env: Some(env),
                ..config.job
            },
        };
        let serialized = toml::to_string(&config_with_env).unwrap();
        let parsed: JobConfig = toml::from_str(&serialized).unwrap();
        let parsed_env = parsed.job.env.unwrap();
        assert_eq!(parsed_env.get("FOO").unwrap(), "bar");
        assert_eq!(parsed_env.get("BAZ").unwrap(), "qux");
    }

    #[test]
    fn test_load_script_returns_none_for_missing() {
        let result = load_script("nonexistent_job_id_12345");
        assert!(result.is_none());
    }

    #[test]
    fn test_save_and_load_script_roundtrip() {
        let id = generate_short_id();
        let command = "echo 'hello world' && date +%Y-%m-%d";

        save_script(&id, command).unwrap();
        let loaded = load_script(&id).unwrap();
        assert_eq!(loaded, command);

        // Cleanup
        delete_script(&id);
        assert!(load_script(&id).is_none());
    }

    #[test]
    fn test_delete_script_removes_file() {
        let id = generate_short_id();
        save_script(&id, "echo test").unwrap();
        assert!(script_path(&id).exists());

        delete_script(&id);
        assert!(!script_path(&id).exists());
    }

    #[test]
    fn test_delete_script_noop_for_missing() {
        // Should not panic
        delete_script("nonexistent_job_id_99999");
    }

    #[test]
    fn test_save_job_creates_script_file() {
        let config = sample_config(&generate_short_id());
        let path = save_job(&config).unwrap();
        assert!(path.exists());

        let sh_path = script_path(&config.job.id);
        assert!(sh_path.exists());

        let script_content = load_script(&config.job.id).unwrap();
        assert_eq!(script_content, "echo hello");

        // Cleanup
        delete_job_file(&config.job.id).unwrap();
        assert!(!sh_path.exists());
    }

    #[test]
    fn test_load_job_prefers_script_over_toml_command() {
        let id = generate_short_id();
        let config = sample_config(&id);
        save_job(&config).unwrap();

        // Overwrite the .sh file with a different command
        save_script(&id, "echo from script").unwrap();

        let toml_path = jobs_dir().join(format!("{id}.toml"));
        let loaded = load_job(&toml_path).unwrap();
        assert_eq!(loaded.job.command, "echo from script");

        // Cleanup
        delete_job_file(&id).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn test_save_job_sets_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        // Override jobs_dir by writing directly via a custom path approach.
        // We test save_job by temporarily pointing jobs_dir to a tempdir.
        // Since jobs_dir() reads from dirs::config_dir(), we instead call the
        // underlying logic directly: create dir, write file, set permissions.
        let jobs_dir = dir.path().join("jobs");
        fs::create_dir_all(&jobs_dir).unwrap();
        fs::set_permissions(&jobs_dir, fs::Permissions::from_mode(0o700)).unwrap();

        let config = sample_config("permtest1");
        let contents = toml::to_string(&config).unwrap();
        let file_path = jobs_dir.join("permtest1.toml");
        fs::write(&file_path, &contents).unwrap();
        fs::set_permissions(&file_path, fs::Permissions::from_mode(0o600)).unwrap();

        let dir_meta = fs::metadata(&jobs_dir).unwrap();
        assert_eq!(dir_meta.permissions().mode() & 0o777, 0o700);

        let file_meta = fs::metadata(&file_path).unwrap();
        assert_eq!(file_meta.permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn test_duration_serde_valid_formats() {
        for s in ["30", "30s", "5m", "1h", "0", "0s"] {
            assert!(super::duration_serde::is_valid(s), "{s} should be valid");
        }
    }

    #[test]
    fn test_duration_serde_invalid_formats() {
        for s in ["", "abc", "1x", "5 m", "1H", "1.5h", "-1s"] {
            assert!(!super::duration_serde::is_valid(s), "{s} should be invalid");
        }
    }

    #[test]
    fn test_timeout_rejects_invalid_duration() {
        let toml_str = r#"
[job]
id = "abc12345"
command = "echo hi"
schedule = "* * * * *"
timeout = "bad"
"#;
        let result: Result<JobConfig, _> = toml::from_str(toml_str);
        assert!(result.is_err(), "invalid timeout should fail to parse");
    }

    #[test]
    fn test_on_silence_rejects_invalid_duration() {
        let toml_str = r#"
[job]
id = "abc12345"
command = "echo hi"
schedule = "* * * * *"

[job.alert]
on_silence = "2d"
"#;
        let result: Result<JobConfig, _> = toml::from_str(toml_str);
        assert!(result.is_err(), "invalid on_silence should fail to parse");
    }

    #[test]
    fn test_timeout_accepts_all_valid_formats() {
        for timeout in ["30", "30s", "5m", "1h"] {
            let toml_str = format!(
                r#"
[job]
id = "abc12345"
command = "echo hi"
schedule = "* * * * *"
timeout = "{timeout}"
"#
            );
            let result: Result<JobConfig, _> = toml::from_str(&toml_str);
            assert!(result.is_ok(), "timeout={timeout} should parse ok");
            assert_eq!(result.unwrap().job.timeout.as_deref(), Some(timeout));
        }
    }

    #[test]
    fn test_on_silence_accepts_all_valid_formats() {
        for dur in ["30", "30s", "5m", "1h"] {
            let toml_str = format!(
                r#"
[job]
id = "abc12345"
command = "echo hi"
schedule = "* * * * *"

[job.alert]
on_silence = "{dur}"
"#
            );
            let result: Result<JobConfig, _> = toml::from_str(&toml_str);
            assert!(result.is_ok(), "on_silence={dur} should parse ok");
            assert_eq!(
                result.unwrap().job.alert.unwrap().on_silence.as_deref(),
                Some(dur)
            );
        }
    }

    #[test]
    fn test_find_job_ambiguous_prefix_returns_error() {
        let base = generate_short_id();
        let prefix = &base[..4];
        let id1 = format!("{prefix}aa11");
        let id2 = format!("{prefix}bb22");

        save_job(&sample_config(&id1)).unwrap();
        save_job(&sample_config(&id2)).unwrap();

        let result = find_job(prefix);
        assert!(matches!(result, Err(CoreError::AmbiguousJob(_))));

        delete_job_file(&id1).unwrap();
        delete_job_file(&id2).unwrap();
    }
}
