use std::time::Duration;

use chrono::Utc;
use tokio::process::Command;
use tracing::{info, info_span, Instrument};

use crate::error::CoreError;

pub struct JobOutput {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub started_at: chrono::DateTime<Utc>,
    pub finished_at: chrono::DateTime<Utc>,
    pub success: bool,
}

/// Execute a command and capture its output.
///
/// # Errors
/// Returns `CoreError::Execution` if the process could not be spawned.
/// Returns `CoreError::Timeout` if the command exceeds the given timeout.
pub async fn execute_command(
    command: &str,
    working_dir: Option<&str>,
    timeout: Option<Duration>,
) -> Result<JobOutput, CoreError> {
    let started_at = Utc::now();
    let span = info_span!("execute_command", command = %command);

    async {
        info!("executing command");

        let mut cmd = Command::new("sh");
        cmd.args(["-c", command]);

        if let Some(dir) = working_dir {
            cmd.current_dir(dir);
        }

        let output = if let Some(dur) = timeout {
            match tokio::time::timeout(dur, cmd.output()).await {
                Ok(Ok(out)) => out,
                Ok(Err(e)) => return Err(CoreError::Execution(e.to_string())),
                Err(_) => return Err(CoreError::Timeout(dur)),
            }
        } else {
            cmd.output()
                .await
                .map_err(|e| CoreError::Execution(e.to_string()))?
        };

        let finished_at = Utc::now();

        Ok(JobOutput {
            exit_code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            started_at,
            finished_at,
            success: output.status.success(),
        })
    }
    .instrument(span)
    .await
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_execute_simple_command() {
        let output = execute_command("echo hello", None, None).await.unwrap();
        assert!(output.success);
        assert_eq!(output.exit_code, Some(0));
        assert_eq!(output.stdout.trim(), "hello");
        assert!(output.stderr.is_empty());
    }

    #[tokio::test]
    async fn test_execute_failing_command() {
        let output = execute_command("exit 1", None, None).await.unwrap();
        assert!(!output.success);
        assert_eq!(output.exit_code, Some(1));
    }

    #[tokio::test]
    async fn test_execute_captures_stderr() {
        let output = execute_command("echo err >&2", None, None).await.unwrap();
        assert!(output.success);
        assert_eq!(output.stderr.trim(), "err");
    }

    #[tokio::test]
    async fn test_execute_with_working_dir() {
        let output = execute_command("pwd", Some("/tmp"), None).await.unwrap();
        assert!(output.success);
        assert!(output.stdout.trim().starts_with("/tmp"));
    }

    #[tokio::test]
    async fn test_execute_timeout() {
        let result = execute_command("sleep 10", None, Some(Duration::from_millis(100))).await;
        assert!(matches!(result, Err(CoreError::Timeout(_))));
    }
}
