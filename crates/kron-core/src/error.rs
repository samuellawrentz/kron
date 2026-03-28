#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CoreError {
    #[error("invalid schedule: {0}")]
    InvalidSchedule(String),

    #[error("job not found: {name}")]
    JobNotFound { name: String },

    #[error("invalid job name '{name}': {reason}")]
    InvalidJobName { name: String, reason: String },

    #[error("config error: {0}")]
    Config(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("store error: {0}")]
    Store(#[from] kron_store::StoreError),

    #[error("toml parse error: {0}")]
    TomlParse(#[from] toml::de::Error),

    #[error("toml serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("execution failed: {0}")]
    Execution(String),

    #[error("job timed out after {0:?}")]
    Timeout(std::time::Duration),

    #[error("notification error: {0}")]
    Notification(String),
}
