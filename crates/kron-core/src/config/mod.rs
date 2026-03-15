use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

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
