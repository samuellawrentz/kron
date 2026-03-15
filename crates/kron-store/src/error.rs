use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("migration error: {0}")]
    Migration(#[from] rusqlite_migration::Error),

    #[error("job not found: {name}")]
    JobNotFound { name: String },

    #[error("job already exists: {name}")]
    JobAlreadyExists { name: String },

    #[error("run not found: {id}")]
    RunNotFound { id: String },
}
