//! Job execution engine: runs commands/scripts with timeout, env vars, and output capture.

use std::fmt::Write as _;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use chrono::Utc;
use tokio::process::Command;
use tracing::{Instrument, info, info_span};

#[cfg(unix)]
use nix::sys::signal::{self, Signal};
#[cfg(unix)]
use nix::unistd::Pid;

/// Determine the user's login shell for job execution.
///
/// Uses `$SHELL` so that shell-specific startup files (`.zshenv`,
/// `.bash_profile`, etc.) are sourced when running with `-l`.
/// Falls back to `"sh"` if `$SHELL` is unset.
fn user_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| {
        info!("$SHELL not set, falling back to 'sh' -- shell startup files may not load");
        "sh".into()
    })
}

use crate::error::CoreError;

/// Maximum number of bytes captured from stdout or stderr per run.
pub const MAX_OUTPUT_BYTES: usize = 1_048_576; // 1 MiB

/// Truncate `output` to at most `MAX_OUTPUT_BYTES`, appending a note if truncated.
fn truncate_output(output: String) -> String {
    if output.len() <= MAX_OUTPUT_BYTES {
        return output;
    }
    let total = output.len();
    let safe_boundary = output.floor_char_boundary(MAX_OUTPUT_BYTES);
    let mut truncated = output[..safe_boundary].to_owned();
    let _ = write!(truncated, "\n... [truncated, {total} bytes total]");
    truncated
}

pub struct JobOutput {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub started_at: chrono::DateTime<Utc>,
    pub finished_at: chrono::DateTime<Utc>,
    pub success: bool,
}

/// Apply working directory and environment variables to a `Command`.
fn configure_command(
    cmd: &mut Command,
    working_dir: Option<&str>,
    env_vars: Option<&std::collections::HashMap<String, String>>,
) {
    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }
    if let Some(vars) = env_vars {
        for (key, value) in vars {
            cmd.env(key, value);
        }
    }
}

#[cfg(unix)]
fn kill_process_group(pid: u32) {
    let Ok(raw_pid) = i32::try_from(pid) else {
        return;
    };
    if raw_pid > 0 {
        let _ = signal::kill(Pid::from_raw(-raw_pid), Signal::SIGKILL);
    }
}

/// Drive a fully-configured `Command` to completion, respecting an optional timeout,
/// and return a `JobOutput`.
///
/// The `started_at` timestamp is captured *before* this call so callers control
/// when timing begins.  `span` is the tracing span to instrument the future under.
///
/// # Errors
/// Returns `CoreError::Execution` if the process could not be spawned.
/// Returns `CoreError::Timeout` if the command exceeds the given timeout.
#[allow(clippy::implicit_hasher)]
async fn run_command(
    mut cmd: Command,
    working_dir: Option<&str>,
    timeout: Option<Duration>,
    env_vars: Option<&std::collections::HashMap<String, String>>,
    span: tracing::Span,
) -> Result<JobOutput, CoreError> {
    let started_at = Utc::now();
    configure_command(&mut cmd, working_dir, env_vars);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    #[cfg(unix)]
    cmd.process_group(0);

    async move {
        let child = cmd
            .spawn()
            .map_err(|e| CoreError::Execution(e.to_string()))?;
        let child_pid = child.id();

        let output = if let Some(dur) = timeout {
            let mut wait = Box::pin(child.wait_with_output());
            match tokio::time::timeout(dur, wait.as_mut()).await {
                Ok(Ok(out)) => out,
                Ok(Err(e)) => return Err(CoreError::Execution(e.to_string())),
                Err(_) => {
                    #[cfg(unix)]
                    if let Some(pid) = child_pid {
                        kill_process_group(pid);
                    }

                    let _ = wait.await;
                    return Err(CoreError::Timeout(dur));
                }
            }
        } else {
            child
                .wait_with_output()
                .await
                .map_err(|e| CoreError::Execution(e.to_string()))?
        };

        let finished_at = Utc::now();

        Ok(JobOutput {
            exit_code: output.status.code(),
            stdout: truncate_output(String::from_utf8_lossy(&output.stdout).into_owned()),
            stderr: truncate_output(String::from_utf8_lossy(&output.stderr).into_owned()),
            started_at,
            finished_at,
            success: output.status.success(),
        })
    }
    .instrument(span)
    .await
}

/// Execute a command and capture its output.
///
/// # Errors
/// Returns `CoreError::Execution` if the process could not be spawned.
/// Returns `CoreError::Timeout` if the command exceeds the given timeout.
#[allow(clippy::implicit_hasher)]
pub async fn execute_command(
    command: &str,
    working_dir: Option<&str>,
    timeout: Option<Duration>,
    env_vars: Option<&std::collections::HashMap<String, String>>,
) -> Result<JobOutput, CoreError> {
    let span = info_span!("execute_command", command = %command);
    info!(parent: &span, "executing command");

    let shell = user_shell();
    let mut cmd = Command::new(&shell);
    cmd.args(["-l", "-c", command]);
    cmd.kill_on_drop(true);

    run_command(cmd, working_dir, timeout, env_vars, span).await
}

/// Execute a job, preferring the `.sh` script file if it exists.
///
/// If `script_path` is `Some` and the file exists on disk at call time, runs
/// `bash <script_path>`.  Otherwise falls back to `sh -c <command>`.
///
/// # Design note
/// The `path.exists()` check is technically a TOCTOU race, but it is acceptable
/// here: the kron daemon never deletes a job's `.sh` file while the job is
/// running, and the `--once` deletion happens only *after* execution completes.
///
/// # Errors
/// Returns `CoreError::Execution` if the process could not be spawned.
/// Returns `CoreError::Timeout` if the command exceeds the given timeout.
#[allow(clippy::implicit_hasher)]
pub async fn execute_command_or_script(
    command: &str,
    script_path: Option<&Path>,
    working_dir: Option<&str>,
    timeout: Option<Duration>,
    env_vars: Option<&std::collections::HashMap<String, String>>,
) -> Result<JobOutput, CoreError> {
    if let Some(path) = script_path
        && path.exists()
    {
        let path_str = path.display().to_string();
        let span = info_span!("execute_script", script = %path_str);
        info!(parent: &span, "executing script file");

        let shell = user_shell();
        let mut cmd = Command::new(&shell);
        cmd.args([std::ffi::OsStr::new("-l"), path.as_os_str()]);
        cmd.kill_on_drop(true);

        return run_command(cmd, working_dir, timeout, env_vars, span).await;
    }

    // Fall back to sh -c for inline commands (no script file present)
    execute_command(command, working_dir, timeout, env_vars).await
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_execute_simple_command() {
        let output = execute_command("echo hello", None, None, None)
            .await
            .unwrap();
        assert!(output.success);
        assert_eq!(output.exit_code, Some(0));
        assert_eq!(output.stdout.trim(), "hello");
        assert!(output.stderr.is_empty());
    }

    #[tokio::test]
    async fn test_execute_failing_command() {
        let output = execute_command("exit 1", None, None, None).await.unwrap();
        assert!(!output.success);
        assert_eq!(output.exit_code, Some(1));
    }

    #[tokio::test]
    async fn test_execute_captures_stderr() {
        let output = execute_command("echo err >&2", None, None, None)
            .await
            .unwrap();
        assert!(output.success);
        assert_eq!(output.stderr.trim(), "err");
    }

    #[tokio::test]
    async fn test_execute_with_working_dir() {
        let output = execute_command("pwd", Some("/tmp"), None, None)
            .await
            .unwrap();
        assert!(output.success);
        assert!(output.stdout.trim().starts_with("/tmp"));
    }

    #[tokio::test]
    async fn test_execute_timeout() {
        let result =
            execute_command("sleep 10", None, Some(Duration::from_millis(100)), None).await;
        assert!(matches!(result, Err(CoreError::Timeout(_))));
    }

    #[tokio::test]
    async fn test_execute_with_env_vars() {
        let mut env = std::collections::HashMap::new();
        env.insert("KRON_TEST_VAR".to_string(), "hello123".to_string());
        let output = execute_command("echo $KRON_TEST_VAR", None, None, Some(&env))
            .await
            .unwrap();
        assert!(output.success);
        assert_eq!(output.stdout.trim(), "hello123");
    }

    #[tokio::test]
    async fn test_execute_env_vars_override() {
        let mut env = std::collections::HashMap::new();
        env.insert("HOME".to_string(), "/tmp/kron-test-home".to_string());
        let output = execute_command("echo $HOME", None, None, Some(&env))
            .await
            .unwrap();
        assert!(output.success);
        assert_eq!(output.stdout.trim(), "/tmp/kron-test-home");
    }

    #[tokio::test]
    async fn test_execute_multiple_env_vars() {
        let mut env = std::collections::HashMap::new();
        env.insert("KRON_A".to_string(), "alpha".to_string());
        env.insert("KRON_B".to_string(), "beta".to_string());
        let output = execute_command("echo $KRON_A-$KRON_B", None, None, Some(&env))
            .await
            .unwrap();
        assert!(output.success);
        assert_eq!(output.stdout.trim(), "alpha-beta");
    }

    #[tokio::test]
    async fn test_execute_script_file() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("test.sh");
        std::fs::write(&script, "#!/bin/bash\necho from_script\n").unwrap();

        let output =
            execute_command_or_script("echo fallback", Some(script.as_path()), None, None, None)
                .await
                .unwrap();
        assert!(output.success);
        assert_eq!(output.stdout.trim(), "from_script");
    }

    #[tokio::test]
    async fn test_execute_script_fallback_when_missing() {
        let missing = std::path::PathBuf::from("/tmp/kron_nonexistent_test.sh");
        let output =
            execute_command_or_script("echo fallback", Some(missing.as_path()), None, None, None)
                .await
                .unwrap();
        assert!(output.success);
        assert_eq!(output.stdout.trim(), "fallback");
    }

    #[tokio::test]
    async fn test_execute_script_with_working_dir() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("pwd.sh");
        std::fs::write(&script, "#!/bin/bash\npwd\n").unwrap();

        let output = execute_command_or_script(
            "echo fallback",
            Some(script.as_path()),
            Some("/tmp"),
            None,
            None,
        )
        .await
        .unwrap();
        assert!(output.success);
        assert!(output.stdout.trim().starts_with("/tmp"));
    }

    #[tokio::test]
    async fn test_execute_script_none_path_uses_inline() {
        let output = execute_command_or_script("echo inline", None, None, None, None)
            .await
            .unwrap();
        assert!(output.success);
        assert_eq!(output.stdout.trim(), "inline");
    }

    #[test]
    fn test_truncate_output_short() {
        let input = "hello world".to_string();
        let result = truncate_output(input.clone());
        assert_eq!(result, input);
    }

    #[test]
    fn test_truncate_output_long() {
        let total = MAX_OUTPUT_BYTES + 100;
        let input = "x".repeat(total);
        let result = truncate_output(input);
        assert!(result.len() > MAX_OUTPUT_BYTES);
        assert!(result.starts_with(&"x".repeat(MAX_OUTPUT_BYTES)));
        assert!(result.contains(&format!("[truncated, {total} bytes total]")));
    }

    #[test]
    fn test_truncate_output_multibyte_boundary() {
        // Place a 3-byte char (€ = U+20AC) right at the boundary so a naive
        // byte slice would land inside the character.
        let mut s = "a".repeat(MAX_OUTPUT_BYTES - 1);
        s.push('\u{20AC}'); // 3 bytes — straddles the boundary
        s.push_str("tail");
        let total = s.len();
        let result = truncate_output(s); // must not panic
        assert!(result.contains("[truncated,"));
        assert!(result.contains(&format!("{total} bytes total")));
    }

    #[test]
    fn test_truncate_output_exact_boundary() {
        // Exactly MAX_OUTPUT_BYTES should not be truncated
        let input = "z".repeat(MAX_OUTPUT_BYTES);
        let result = truncate_output(input.clone());
        assert_eq!(result, input);
    }
}
