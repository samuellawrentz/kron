use anyhow::{Context, Result};
use croner::Cron;

use kron_core::config::{self, JobConfig, JobDefinition, generate_short_id, validate_job_name};

/// Resolve a schedule string into a valid cron expression.
///
/// Tries parsing as a standard cron expression first. If that fails, tries
/// converting from a human-readable English description via `english-to-cron`.
/// Returns the resolved cron expression and an optional original English input
/// (present only when a conversion was performed).
fn resolve_schedule(schedule: &str) -> Result<(String, Option<String>)> {
    // Try as a direct cron expression first.
    if Cron::new(schedule).parse().is_ok() {
        return Ok((schedule.to_owned(), None));
    }
    let cron_parse_err = Cron::new(schedule)
        .parse()
        .err()
        .map_or_else(|| "invalid cron expression".to_owned(), |e| e.to_string());

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
) -> Result<()> {
    let (resolved_schedule, original_english) = resolve_schedule(&schedule)?;

    let command_str = command.join(" ");

    // Validate name if provided
    if let Some(ref n) = name {
        validate_job_name(n)?;
    }

    let job_id = generate_short_id();

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

    Ok(())
}
