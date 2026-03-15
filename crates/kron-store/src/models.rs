use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct JobRecord {
    pub id: String,
    pub name: Option<String>,
    pub command: String,
    pub schedule: String,
    pub working_dir: Option<String>,
    pub created_at: DateTime<Utc>,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
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

    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s {
            "running" => Self::Running,
            "success" => Self::Success,
            "failed" => Self::Failed,
            _ => {
                tracing::warn!(
                    status = s,
                    "unrecognized RunStatus value, defaulting to Failed"
                );
                Self::Failed
            }
        }
    }
}

impl fmt::Display for RunStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParseRunStatusError(String);

impl fmt::Display for ParseRunStatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown run status: {:?}", self.0)
    }
}

impl std::error::Error for ParseRunStatusError {}

impl FromStr for RunStatus {
    type Err = ParseRunStatusError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "running" => Ok(Self::Running),
            "success" => Ok(Self::Success),
            "failed" => Ok(Self::Failed),
            _ => Err(ParseRunStatusError(s.to_string())),
        }
    }
}
