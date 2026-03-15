use std::io::{self, BufRead, Write};

use anyhow::{Context, Result};

use kron_core::config::{self, JobConfig, JobDefinition, generate_short_id};
use kron_core::crontab::{self, CrontabEntry};

/// Parse a selection string like "1,3,5", "1-3", "all", or a mix like "1,3-5".
/// Returns sorted, deduplicated 0-based indices.
fn parse_selection(input: &str, max: usize) -> Result<Vec<usize>> {
    let input = input.trim().to_lowercase();

    if input == "all" || input == "a" {
        return Ok((0..max).collect());
    }

    let mut selected = Vec::new();
    for part in input.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((start, end)) = part.split_once('-') {
            let start: usize = start
                .trim()
                .parse()
                .with_context(|| format!("invalid number: {start}"))?;
            let end: usize = end
                .trim()
                .parse()
                .with_context(|| format!("invalid number: {end}"))?;
            if start == 0 || end == 0 || start > max || end > max {
                anyhow::bail!("selection out of range (1-{max}): {part}");
            }
            let (lo, hi) = if start <= end {
                (start, end)
            } else {
                (end, start)
            };
            for i in lo..=hi {
                selected.push(i - 1);
            }
        } else {
            let n: usize = part
                .parse()
                .with_context(|| format!("invalid number: {part}"))?;
            if n == 0 || n > max {
                anyhow::bail!("selection out of range (1-{max}): {n}");
            }
            selected.push(n - 1);
        }
    }

    selected.sort_unstable();
    selected.dedup();
    Ok(selected)
}

/// Derive a job name from a crontab entry's command.
///
/// Takes the basename of the first token, strips common extensions, and
/// sanitises to valid job name characters (alphanumeric, hyphens, underscores).
fn derive_name(entry: &CrontabEntry) -> Option<String> {
    // If there's an inline comment, prefer it as a name hint.
    if let Some(ref comment) = entry.comment {
        let sanitised: String = comment
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        let trimmed = sanitised.trim_matches('-').to_string();
        if !trimmed.is_empty() && trimmed.len() <= 64 {
            return Some(trimmed);
        }
    }

    // Fall back to basename of the command.
    let first_token = entry.command.split_whitespace().next()?;
    let basename = std::path::Path::new(first_token).file_stem()?.to_str()?;
    let sanitised: String = basename
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = sanitised.trim_matches('-').to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Ensure a name is unique among existing jobs by appending a suffix.
fn make_unique_name(base: &str, existing: &[String]) -> String {
    if !existing.contains(&base.to_string()) {
        return base.to_string();
    }
    for i in 2.. {
        let candidate = format!("{base}-{i}");
        if !existing.contains(&candidate) {
            return candidate;
        }
    }
    unreachable!()
}

fn display_entries(entries: &[CrontabEntry]) {
    println!();
    println!("  #  Schedule             Command");
    println!("  ── ──────────────────── ──────────────────────────────────");
    for (i, entry) in entries.iter().enumerate() {
        let schedule = if entry.schedule.len() > 20 {
            format!("{}…", &entry.schedule[..19])
        } else {
            format!("{:<20}", entry.schedule)
        };
        let cmd_display = if entry.command.len() > 50 {
            format!("{}…", &entry.command[..49])
        } else {
            entry.command.clone()
        };
        let comment_suffix = entry
            .comment
            .as_ref()
            .map_or(String::new(), |c| format!("  # {c}"));
        println!("  {:<2} {schedule} {cmd_display}{comment_suffix}", i + 1);
    }
    println!();
}

pub fn execute(all: bool) -> Result<()> {
    let raw = crontab::read_system_crontab().context("failed to read system crontab")?;

    if raw.is_empty() {
        println!("No crontab found for current user.");
        return Ok(());
    }

    let entries = crontab::parse_crontab(&raw);

    if entries.is_empty() {
        println!("No importable entries found in crontab.");
        println!("(Comments, environment variables, and @reboot entries are skipped.)");
        return Ok(());
    }

    println!("Found {} crontab entries:", entries.len());
    display_entries(&entries);

    // Determine which entries to import.
    let selected = if all {
        (0..entries.len()).collect()
    } else {
        print!("Select entries to import (e.g. 1,3-5 or 'all'): ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin()
            .lock()
            .read_line(&mut input)
            .context("failed to read selection")?;

        if input.trim().is_empty() {
            println!("No selection made. Aborting import.");
            return Ok(());
        }

        parse_selection(&input, entries.len())?
    };

    if selected.is_empty() {
        println!("No entries selected. Nothing to import.");
        return Ok(());
    }

    // Collect existing job names for uniqueness checks.
    let existing_jobs = config::load_all_jobs().unwrap_or_default();
    let mut existing_names: Vec<String> = existing_jobs
        .iter()
        .filter_map(|j| j.job.name.clone())
        .collect();

    let mut imported = 0;
    for &idx in &selected {
        let entry = &entries[idx];

        let name = derive_name(entry).map(|n| {
            let unique = make_unique_name(&n, &existing_names);
            existing_names.push(unique.clone());
            unique
        });

        let job_id = generate_short_id();
        let job_config = JobConfig {
            job: JobDefinition {
                id: job_id.clone(),
                name: name.clone(),
                command: entry.command.clone(),
                schedule: entry.schedule.clone(),
                working_dir: None,
                enabled: true,
                timeout: None,
                env: None,
                alert: None,
            },
        };

        let path = config::save_job(&job_config)
            .with_context(|| format!("failed to save job for: {}", entry.command))?;

        let display_name = name.as_ref().map_or(job_id.as_str(), String::as_str);
        println!(
            "  Imported: {display_name} ({}) -> {}",
            entry.schedule,
            path.display()
        );
        imported += 1;
    }

    println!("\nImported {imported} job(s). Run 'kron list' to see them.");
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_selection_single() {
        assert_eq!(parse_selection("1", 5).unwrap(), vec![0]);
        assert_eq!(parse_selection("3", 5).unwrap(), vec![2]);
    }

    #[test]
    fn test_parse_selection_multiple() {
        assert_eq!(parse_selection("1,3,5", 5).unwrap(), vec![0, 2, 4]);
    }

    #[test]
    fn test_parse_selection_range() {
        assert_eq!(parse_selection("2-4", 5).unwrap(), vec![1, 2, 3]);
    }

    #[test]
    fn test_parse_selection_mixed() {
        assert_eq!(parse_selection("1,3-5", 5).unwrap(), vec![0, 2, 3, 4]);
    }

    #[test]
    fn test_parse_selection_all() {
        assert_eq!(parse_selection("all", 3).unwrap(), vec![0, 1, 2]);
        assert_eq!(parse_selection("a", 3).unwrap(), vec![0, 1, 2]);
    }

    #[test]
    fn test_parse_selection_dedup() {
        assert_eq!(parse_selection("1,1,2", 3).unwrap(), vec![0, 1]);
    }

    #[test]
    fn test_parse_selection_out_of_range() {
        assert!(parse_selection("0", 3).is_err());
        assert!(parse_selection("4", 3).is_err());
    }

    #[test]
    fn test_derive_name_from_comment() {
        let entry = CrontabEntry {
            schedule: "0 2 * * *".to_string(),
            command: "/usr/bin/backup.sh".to_string(),
            comment: Some("nightly backup".to_string()),
        };
        assert_eq!(derive_name(&entry), Some("nightly-backup".to_string()));
    }

    #[test]
    fn test_derive_name_from_command() {
        let entry = CrontabEntry {
            schedule: "0 2 * * *".to_string(),
            command: "/usr/bin/backup.sh".to_string(),
            comment: None,
        };
        assert_eq!(derive_name(&entry), Some("backup".to_string()));
    }

    #[test]
    fn test_make_unique_name() {
        let existing = vec!["backup".to_string()];
        assert_eq!(make_unique_name("backup", &existing), "backup-2");
        assert_eq!(make_unique_name("report", &existing), "report");
    }
}
