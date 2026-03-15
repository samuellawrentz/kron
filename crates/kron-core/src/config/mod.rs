use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobConfig {
    pub job: JobDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobDefinition {
    pub name: String,
    pub command: String,
    pub schedule: String,
    #[serde(default)]
    pub working_dir: Option<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub timeout: Option<String>,
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
///
/// # Errors
/// Returns `CoreError` if the file cannot be read or is not valid TOML.
pub fn load_job(path: &Path) -> Result<JobConfig, CoreError> {
    let contents = std::fs::read_to_string(path)?;
    let config = toml::from_str(&contents)?;
    Ok(config)
}

/// Write a job config as TOML to `jobs_dir()/<name>.toml`.
/// Creates parent directories if needed.
/// Returns the path written.
///
/// # Errors
/// Returns `CoreError` if the file cannot be written or serialized.
pub fn save_job(config: &JobConfig) -> Result<PathBuf, CoreError> {
    let dir = jobs_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.toml", config.job.name));
    let contents = toml::to_string(config)?;
    std::fs::write(&path, contents)?;
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

/// Remove the TOML config file for the named job.
///
/// # Errors
/// Returns `CoreError::JobNotFound` if the file does not exist, or `CoreError::Io` on failure.
pub fn delete_job_file(name: &str) -> Result<(), CoreError> {
    let path = jobs_dir().join(format!("{name}.toml"));
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(CoreError::JobNotFound {
            name: name.to_string(),
        }),
        Err(e) => Err(CoreError::Io(e)),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::fs;

    use super::*;

    fn sample_config(name: &str) -> JobConfig {
        JobConfig {
            job: JobDefinition {
                name: name.to_string(),
                command: "echo hello".to_string(),
                schedule: "0 2 * * *".to_string(),
                working_dir: None,
                enabled: true,
                timeout: None,
            },
        }
    }

    #[test]
    fn test_serialize_deserialize_job_config() {
        let original = sample_config("backup");
        let serialized = toml::to_string(&original).unwrap();
        let parsed: JobConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(parsed.job.name, original.job.name);
        assert_eq!(parsed.job.command, original.job.command);
        assert_eq!(parsed.job.schedule, original.job.schedule);
        assert_eq!(parsed.job.working_dir, original.job.working_dir);
        assert_eq!(parsed.job.enabled, original.job.enabled);
    }

    #[test]
    fn test_load_all_jobs_with_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        let job_path = dir.path().join("test-job.toml");

        let config = sample_config("test-job");
        let contents = toml::to_string(&config).unwrap();
        fs::write(&job_path, contents).unwrap();

        let loaded = load_job(&job_path).unwrap();
        assert_eq!(loaded.job.name, "test-job");
        assert_eq!(loaded.job.command, "echo hello");
        assert!(loaded.job.enabled);
    }

    #[test]
    fn test_default_enabled_is_true() {
        let toml_str = r#"
[job]
name = "myjob"
command = "echo hi"
schedule = "* * * * *"
"#;
        let config: JobConfig = toml::from_str(toml_str).unwrap();
        assert!(config.job.enabled);
    }
}
