use anyhow::{Context, Result};
use croner::Cron;

use kron_core::config::{self, JobConfig, JobDefinition, generate_short_id, validate_job_name};

/// Quote a single shell argument using POSIX single-quote wrapping.
///
/// Arguments that are safe (alphanumeric + common punctuation) pass through
/// unchanged. Everything else is wrapped in single quotes with internal
/// single quotes escaped as `'\''`.
fn shell_quote(arg: &str) -> String {
    if arg.is_empty() {
        return "''".to_string();
    }
    if arg
        .chars()
        .all(|c| c.is_alphanumeric() || "-_=/.:,@+%".contains(c))
    {
        return arg.to_string();
    }
    format!("'{}'", arg.replace('\'', "'\\''"))
}

/// Join command arguments into a single shell-safe string.
fn shell_quote_join(args: &[String]) -> String {
    args.iter()
        .map(|a| shell_quote(a))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Resolve a schedule string into a valid cron expression.
///
/// Tries parsing as a standard cron expression first. If that fails, tries
/// converting from a human-readable English description via `english-to-cron`.
/// Returns the resolved cron expression and an optional original English input
/// (present only when a conversion was performed).
pub(crate) fn resolve_schedule(schedule: &str) -> Result<(String, Option<String>)> {
    // Try as a direct cron expression first.
    let cron_parse_err = match Cron::new(schedule).parse() {
        Ok(_) => return Ok((schedule.to_owned(), None)),
        Err(e) => e.to_string(),
    };

    // Fall back to english-to-cron.
    let converted = english_to_cron::str_cron_syntax(schedule).map_err(|_| {
        anyhow::anyhow!(
            "invalid schedule '{schedule}': not a valid cron expression ({cron_parse_err}) and could not be parsed as human-readable input"
        )
    })?;

    // Verify the converted expression is accepted by croner.
    // english-to-cron may emit Quartz format (7 fields: seconds + year).
    // Try the raw converted string first, then strip seconds and year fields
    // if croner rejects the 7-field form.
    if Cron::new(&converted).parse().is_ok() {
        return Ok((converted, Some(schedule.to_owned())));
    }

    // Attempt to normalise from 7-field Quartz to 5-field standard by
    // dropping the leading seconds field and the trailing year field.
    let fields: Vec<&str> = converted.split_whitespace().collect();
    if fields.len() == 7 {
        let five_field = fields[1..6].join(" ");
        if Cron::new(&five_field).parse().is_ok() {
            return Ok((five_field, Some(schedule.to_owned())));
        }
    }

    Err(anyhow::anyhow!(
        "invalid schedule '{schedule}': english-to-cron produced '{converted}' which is not accepted by the scheduler"
    ))
}

#[allow(clippy::needless_pass_by_value)]
pub fn execute(
    schedule: String,
    command: Vec<String>,
    name: Option<String>,
    working_dir: Option<String>,
    capture_env: bool,
    once: bool,
) -> Result<()> {
    let (resolved_schedule, original_english) = resolve_schedule(&schedule)?;

    // When the user passes the command as a single string (e.g. "echo hello"),
    // use it as-is — it's already a complete shell command.  Only apply
    // shell_quote_join when there are multiple separate arguments that need
    // safe escaping and joining.
    let command_str = if command.len() == 1 {
        command[0].clone()
    } else {
        shell_quote_join(&command)
    };

    // Validate name if provided
    if let Some(ref n) = name {
        validate_job_name(n)?;
    }

    let job_id = generate_short_id();

    let env = if capture_env {
        Some(std::env::vars().collect())
    } else {
        // Always capture PATH so jobs find the same binaries the user had at
        // creation time — regardless of how the daemon's shell is configured.
        std::env::var("PATH")
            .ok()
            .map(|path| std::collections::HashMap::from([("PATH".to_string(), path)]))
    };

    // Save TOML config file — TOML is the single source of truth for job definitions.
    // Always store the resolved cron expression, not the English input.
    let job_config = JobConfig {
        job: JobDefinition {
            id: job_id.clone(),
            name: name.clone(),
            command: command_str.clone(),
            schedule: resolved_schedule.clone(),
            working_dir: working_dir.clone(),
            enabled: true,
            timeout: None,
            env,
            alert: None,
            once,
        },
    };
    let path = config::save_job(&job_config).context("failed to save job config")?;

    println!("Added job {job_id}");
    if let Some(ref n) = name {
        println!("  Name: {n}");
    }
    println!("  Config: {}", path.display());
    if let Some(english) = original_english {
        println!("  Schedule: {resolved_schedule} (from \"{english}\")");
    } else {
        println!("  Schedule: {resolved_schedule}");
    }
    println!("  Command: {command_str}");
    if once {
        println!("  Once: yes (will auto-remove after running)");
    }

    // Signal the daemon to reload configs immediately
    if kron_core::scheduler::signal_daemon_reload() {
        println!("  Daemon notified (config reloaded)");
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // resolve_schedule tests
    // ------------------------------------------------------------------

    #[test]
    fn raw_cron_passes_through_unchanged() {
        let (expr, original) = resolve_schedule("0 2 * * *").unwrap();
        assert_eq!(expr, "0 2 * * *");
        assert!(original.is_none());
    }

    #[test]
    fn every_minute_cron_passes_through() {
        let (expr, original) = resolve_schedule("* * * * *").unwrap();
        assert_eq!(expr, "* * * * *");
        assert!(original.is_none());
    }

    #[test]
    fn named_cron_at_midnight_passes_through() {
        let (expr, original) = resolve_schedule("0 0 * * *").unwrap();
        assert_eq!(expr, "0 0 * * *");
        assert!(original.is_none());
    }

    #[test]
    fn english_every_day_at_2am_converts() {
        let (expr, original) = resolve_schedule("every day at 2am").unwrap();
        // Result must be a valid 5-field cron expression
        assert_eq!(expr.split_whitespace().count(), 5, "expected 5 cron fields");
        // The hour field should be 2
        let fields: Vec<&str> = expr.split_whitespace().collect();
        assert_eq!(fields[1], "2", "hour field should be 2");
        // original English input is preserved
        assert_eq!(original.as_deref(), Some("every day at 2am"));
    }

    #[test]
    fn english_every_hour_converts() {
        let (expr, original) = resolve_schedule("every hour").unwrap();
        assert_eq!(expr.split_whitespace().count(), 5);
        assert!(original.is_some());
    }

    #[test]
    fn english_every_minute_converts() {
        let (expr, original) = resolve_schedule("every minute").unwrap();
        assert_eq!(expr.split_whitespace().count(), 5);
        assert!(original.is_some());
    }

    #[test]
    fn invalid_schedule_returns_error() {
        let result = resolve_schedule("this is not valid");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("invalid schedule"));
    }

    #[test]
    fn empty_schedule_returns_error() {
        assert!(resolve_schedule("").is_err());
    }

    #[test]
    fn five_field_with_day_of_week_passes_through() {
        // day-of-week range — valid 5-field cron
        let (expr, original) = resolve_schedule("0 9 * * 1-5").unwrap();
        assert_eq!(expr, "0 9 * * 1-5");
        assert!(original.is_none());
    }

    // ------------------------------------------------------------------
    // shell_quote / shell_quote_join tests
    // ------------------------------------------------------------------

    #[test]
    fn simple_args_unquoted() {
        let args = vec!["echo".into(), "hello".into()];
        assert_eq!(shell_quote_join(&args), "echo hello");
    }

    #[test]
    fn args_with_spaces_get_quoted() {
        let args = vec![
            "terminal-notifier".into(),
            "-title".into(),
            "💧 Hydration Reminder".into(),
            "-message".into(),
            "Time to drink some water!".into(),
            "-sound".into(),
            "default".into(),
        ];
        let result = shell_quote_join(&args);
        assert_eq!(
            result,
            "terminal-notifier -title '💧 Hydration Reminder' -message 'Time to drink some water!' -sound default"
        );
    }

    #[test]
    fn exclamation_mark_preserved() {
        let args = vec!["echo".into(), "Hello!".into()];
        assert_eq!(shell_quote_join(&args), "echo 'Hello!'");
    }

    #[test]
    fn internal_single_quotes_escaped() {
        let args = vec!["echo".into(), "it's alive".into()];
        assert_eq!(shell_quote_join(&args), "echo 'it'\\''s alive'");
    }

    #[test]
    fn empty_arg_quoted() {
        let args = vec!["cmd".into(), String::new(), "arg".into()];
        assert_eq!(shell_quote_join(&args), "cmd '' arg");
    }

    #[test]
    fn shell_metacharacters_quoted() {
        let args = vec!["echo".into(), "$HOME".into(), "*.txt".into(), "a|b".into()];
        assert_eq!(shell_quote_join(&args), "echo '$HOME' '*.txt' 'a|b'");
    }

    #[test]
    fn safe_punctuation_unquoted() {
        let args = vec![
            "/usr/bin/script.sh".into(),
            "--flag=value".into(),
            "a,b".into(),
        ];
        assert_eq!(
            shell_quote_join(&args),
            "/usr/bin/script.sh --flag=value a,b"
        );
    }
}
