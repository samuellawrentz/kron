use crate::error::CoreError;

/// A single entry parsed from a crontab file.
#[derive(Debug, Clone)]
pub struct CrontabEntry {
    /// The cron schedule expression (5 fields).
    pub schedule: String,
    /// The command to execute.
    pub command: String,
    /// Optional inline comment from the original line.
    pub comment: Option<String>,
}

/// Map `@` shorthand aliases to standard 5-field cron expressions.
fn expand_special(keyword: &str) -> Option<&'static str> {
    match keyword {
        "@yearly" | "@annually" => Some("0 0 1 1 *"),
        "@monthly" => Some("0 0 1 * *"),
        "@weekly" => Some("0 0 * * 0"),
        "@daily" | "@midnight" => Some("0 0 * * *"),
        "@hourly" => Some("0 * * * *"),
        _ => None,
    }
}

/// Parse raw crontab text into a list of entries.
///
/// Skips blank lines, comments, environment variable assignments, and
/// `@reboot` entries (which have no periodic equivalent).
pub fn parse_crontab(contents: &str) -> Vec<CrontabEntry> {
    let mut entries = Vec::new();

    for line in contents.lines() {
        let line = line.trim();

        // Skip empty lines and pure comments.
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Skip environment variable assignments (KEY=value).
        if line.contains('=') && !line.starts_with('@') && !line.starts_with('*') {
            // Heuristic: if the first token contains '=' it's likely an env var.
            if let Some(first) = line.split_whitespace().next()
                && first.contains('=')
            {
                continue;
            }
        }

        // Handle @reboot — no periodic equivalent, skip with a note.
        if line.starts_with("@reboot") {
            continue;
        }

        // Handle @special shortcuts.
        if let Some(first) = line.split_whitespace().next()
            && let Some(expanded) = expand_special(first)
        {
            let rest = line[first.len()..].trim();
            let (command, comment) = split_inline_comment(rest);
            if !command.is_empty() {
                entries.push(CrontabEntry {
                    schedule: expanded.to_string(),
                    command,
                    comment,
                });
            }
            continue;
        }

        // Standard 5-field cron line: min hour dom month dow command
        let parts: Vec<&str> = line.splitn(6, char::is_whitespace).collect();
        if parts.len() < 6 {
            continue; // Not enough fields.
        }

        let schedule = parts[..5].join(" ");
        let raw_command = parts[5].trim();
        let (command, comment) = split_inline_comment(raw_command);

        if !command.is_empty() {
            entries.push(CrontabEntry {
                schedule,
                command,
                comment,
            });
        }
    }

    entries
}

/// Split a command string on an unquoted `#` to extract an inline comment.
fn split_inline_comment(s: &str) -> (String, Option<String>) {
    // Simple heuristic: split on ` #` (space-hash) that isn't inside quotes.
    // This handles the common case without a full shell parser.
    let mut in_single = false;
    let mut in_double = false;
    let bytes = s.as_bytes();

    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'#' if !in_single && !in_double => {
                // Only treat as comment if preceded by whitespace.
                if i > 0 && bytes[i - 1] == b' ' {
                    let cmd = s[..i].trim().to_string();
                    let comment = s[i + 1..].trim().to_string();
                    let comment = if comment.is_empty() {
                        None
                    } else {
                        Some(comment)
                    };
                    return (cmd, comment);
                }
            }
            _ => {}
        }
    }

    (s.trim().to_string(), None)
}

/// Read the current user's crontab.
///
/// # Errors
/// Returns `CoreError::Execution` if `crontab -l` fails.
pub fn read_system_crontab() -> Result<String, CoreError> {
    let output = std::process::Command::new("crontab")
        .arg("-l")
        .output()
        .map_err(|e| CoreError::Execution(format!("failed to run 'crontab -l': {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // "no crontab for user" is not really an error.
        if stderr.contains("no crontab for") {
            return Ok(String::new());
        }
        return Err(CoreError::Execution(format!("crontab -l failed: {stderr}")));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_standard_entry() {
        let input = "0 2 * * * /usr/bin/backup.sh";
        let entries = parse_crontab(input);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].schedule, "0 2 * * *");
        assert_eq!(entries[0].command, "/usr/bin/backup.sh");
        assert!(entries[0].comment.is_none());
    }

    #[test]
    fn test_parse_multiple_entries() {
        let input = "\
0 2 * * * /usr/bin/backup.sh
*/5 * * * * /usr/bin/check-health
30 8 * * 1-5 /home/user/report.py
";
        let entries = parse_crontab(input);
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn test_skip_comments_and_blank_lines() {
        let input = "\
# This is a comment
0 2 * * * /usr/bin/backup.sh

# Another comment
";
        let entries = parse_crontab(input);
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn test_skip_env_vars() {
        let input = "\
SHELL=/bin/bash
PATH=/usr/local/bin:/usr/bin
MAILTO=user@example.com
0 2 * * * /usr/bin/backup.sh
";
        let entries = parse_crontab(input);
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn test_special_shortcuts() {
        let input = "\
@hourly /usr/bin/check
@daily /usr/bin/backup
@weekly /usr/bin/report
@monthly /usr/bin/invoice
@yearly /usr/bin/audit
";
        let entries = parse_crontab(input);
        assert_eq!(entries.len(), 5);
        assert_eq!(entries[0].schedule, "0 * * * *");
        assert_eq!(entries[1].schedule, "0 0 * * *");
        assert_eq!(entries[2].schedule, "0 0 * * 0");
        assert_eq!(entries[3].schedule, "0 0 1 * *");
        assert_eq!(entries[4].schedule, "0 0 1 1 *");
    }

    #[test]
    fn test_skip_reboot() {
        let input = "@reboot /usr/bin/startup.sh";
        let entries = parse_crontab(input);
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn test_inline_comment() {
        let input = "0 2 * * * /usr/bin/backup.sh # nightly backup";
        let entries = parse_crontab(input);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].command, "/usr/bin/backup.sh");
        assert_eq!(entries[0].comment.as_deref(), Some("nightly backup"));
    }

    #[test]
    fn test_command_with_hash_in_quotes() {
        let input = r#"0 * * * * echo "color=#ff0000""#;
        let entries = parse_crontab(input);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].command, r#"echo "color=#ff0000""#);
    }

    #[test]
    fn test_expand_special_aliases() {
        assert_eq!(expand_special("@annually"), Some("0 0 1 1 *"));
        assert_eq!(expand_special("@midnight"), Some("0 0 * * *"));
        assert!(expand_special("@reboot").is_none());
        assert!(expand_special("invalid").is_none());
    }
}
