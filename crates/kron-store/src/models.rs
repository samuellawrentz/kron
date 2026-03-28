use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq)]
pub struct JobRecord {
    pub id: String,
    pub name: Option<String>,
    pub command: String,
    pub schedule: String,
    pub working_dir: Option<String>,
    pub created_at: DateTime<Utc>,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunRecord {
    pub id: String,
    pub job_id: String,
    pub job_name: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub status: RunStatus,
}

impl RunRecord {
    /// Display name for this run's job: prefers `job_name`, falls back to `job_id`.
    #[must_use]
    pub fn display_name(&self) -> &str {
        display_name_from(&self.job_name, &self.job_id)
    }
}

/// Metadata-only view of a run — no stdout/stderr.
/// Use for list/history/status displays to avoid loading large output blobs.
#[derive(Debug, Clone, PartialEq)]
pub struct RunSummary {
    pub id: String,
    pub job_id: String,
    pub job_name: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub exit_code: Option<i32>,
    pub status: RunStatus,
}

impl RunSummary {
    /// Display name for this run's job: prefers `job_name`, falls back to `job_id`.
    #[must_use]
    pub fn display_name(&self) -> &str {
        display_name_from(&self.job_name, &self.job_id)
    }
}

fn display_name_from<'a>(job_name: &'a str, job_id: &'a str) -> &'a str {
    if job_name.is_empty() {
        job_id
    } else {
        job_name
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RunStatus {
    Running,
    Success,
    Failed,
}

impl RunStatus {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Success => "success",
            Self::Failed => "failed",
        }
    }
}

impl fmt::Display for RunStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for RunStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "running" => Ok(Self::Running),
            "success" => Ok(Self::Success),
            "failed" => Ok(Self::Failed),
            _ => Err(format!("unknown run status: {s:?}")),
        }
    }
}
