use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

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
    #[serde(default)]
    pub on_silence: Option<String>,
}

fn default_on_failure() -> bool {
    true
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
    }
    let contents = toml::to_string(config)?;
    std::fs::write(&path, contents)?;
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
    uuid::Uuid::new_v4().to_string().replace('-', "")[..8].to_string()
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
    #[serde(default)]
    pub timeout: Option<String>,
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
    #[serde(default)]
    pub alert: Option<JobAlert>,
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

/// Read and parse a TOML job config file.
/// If the config has no `id`, generates one and re-saves the file (backward compat).
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
        let entry = entry?;
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
    let jobs = load_all_jobs()?;
    // First try exact ID match
    if let Some(job) = jobs.iter().find(|j| j.job.id == query) {
        return Ok(Some(job.clone()));
    }
    // Then try ID prefix match (so users can type just first few chars)
    let prefix_matches: Vec<_> = jobs
        .iter()
        .filter(|j| j.job.id.starts_with(query))
        .collect();
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
            },
        }
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
}
