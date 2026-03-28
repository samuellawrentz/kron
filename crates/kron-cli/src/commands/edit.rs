use anyhow::{Context, Result};
use croner::Cron;

use kron_core::config::{self, validate_job_name};
use kron_core::scheduler::parse_duration;

pub struct EditArgs<'a> {
    pub query: &'a str,
    pub schedule: Option<String>,
    pub command: Option<String>,
    pub name: Option<String>,
    pub timeout: Option<String>,
    pub working_dir: Option<String>,
    pub enable: bool,
    pub disable: bool,
    pub once: Option<bool>,
}

#[allow(clippy::needless_pass_by_value)]
pub fn execute(args: EditArgs<'_>) -> Result<()> {
    // Must provide at least one edit flag
    if args.schedule.is_none()
        && args.command.is_none()
        && args.name.is_none()
        && args.timeout.is_none()
        && args.working_dir.is_none()
        && !args.enable
        && !args.disable
        && args.once.is_none()
    {
        anyhow::bail!(
            "nothing to edit — provide at least one flag (--schedule, --command, --name, --timeout, --working-dir, --enable, --disable)"
        );
    }

    let mut job = config::find_job(args.query)
        .context("failed to load jobs")?
        .with_context(|| format!("job '{}' not found", args.query))?;

    let mut changes: Vec<String> = Vec::new();

    // Validate and apply schedule
    if let Some(ref sched) = args.schedule {
        Cron::new(sched)
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid cron expression '{sched}': {e}"))?;
        changes.push(format!(
            "schedule: \"{}\" -> \"{}\"",
            job.job.schedule, sched
        ));
        job.job.schedule.clone_from(sched);
    }

    // Apply command
    if let Some(ref cmd) = args.command {
        changes.push(format!("command: \"{}\" -> \"{}\"", job.job.command, cmd));
        job.job.command.clone_from(cmd);
    }

    // Validate and apply name
    if let Some(ref new_name) = args.name {
        validate_job_name(new_name)?;

        // Check for name collision
        if let Some(existing) = config::find_job(new_name).context("failed to check name")?
            && existing.job.id != job.job.id
        {
            anyhow::bail!(
                "a job named '{new_name}' already exists ({})",
                existing.job.id
            );
        }

        let old_name = job.job.name.as_deref().unwrap_or("(none)").to_string();
        changes.push(format!("name: \"{old_name}\" -> \"{new_name}\""));
        job.job.name = Some(new_name.clone());
    }

    // Validate and apply timeout
    if let Some(ref t) = args.timeout {
        if t == "none" || t == "off" || t.is_empty() {
            let old = job.job.timeout.as_deref().unwrap_or("(none)");
            changes.push(format!("timeout: \"{old}\" -> (none)"));
            job.job.timeout = None;
        } else if parse_duration(t).is_none() {
            anyhow::bail!("invalid timeout '{t}' — use e.g. \"30s\", \"5m\", \"1h\"");
        } else {
            let old = job.job.timeout.as_deref().unwrap_or("(none)");
            changes.push(format!("timeout: \"{old}\" -> \"{t}\""));
            job.job.timeout = Some(t.clone());
        }
    }

    // Apply working directory
    if let Some(ref wd) = args.working_dir {
        let old = job
            .job
            .working_dir
            .as_deref()
            .unwrap_or("(none)")
            .to_string();
        changes.push(format!("working_dir: \"{old}\" -> \"{wd}\""));
        job.job.working_dir = Some(wd.clone());
    }

    // Apply enable/disable
    if args.enable {
        if !job.job.enabled {
            changes.push("enabled: false -> true".to_string());
        }
        job.job.enabled = true;
    } else if args.disable {
        if job.job.enabled {
            changes.push("enabled: true -> false".to_string());
        }
        job.job.enabled = false;
    }

    // Apply once flag
    if let Some(new_once) = args.once
        && new_once != job.job.once
    {
        changes.push(format!("once: {} -> {}", job.job.once, new_once));
        job.job.once = new_once;
    }

    if changes.is_empty() {
        println!("No changes — job already has the requested values.");
        return Ok(());
    }

    // Write back (same file path, atomic via save_job)
    config::save_job(&job).context("failed to save job config")?;

    let display_name = job.job.name.as_deref().unwrap_or(&job.job.id);
    println!("Updated job '{display_name}' ({}):", job.job.id);
    for change in &changes {
        println!("  {change}");
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
    use croner::Cron;
    use kron_core::config::validate_job_name;
    use kron_core::scheduler::parse_duration;

    // ------------------------------------------------------------------
    // Schedule validation (mirrors the logic in execute())
    // ------------------------------------------------------------------

    #[test]
    fn valid_cron_expressions_accepted() {
        assert!(Cron::new("* * * * *").parse().is_ok());
        assert!(Cron::new("0 2 * * *").parse().is_ok());
        assert!(Cron::new("30 6 * * 1-5").parse().is_ok());
        assert!(Cron::new("0 0 1 * *").parse().is_ok());
    }

    #[test]
    fn invalid_cron_expressions_rejected() {
        assert!(Cron::new("not a cron").parse().is_err());
        assert!(Cron::new("99 99 99 99 99").parse().is_err());
        assert!(Cron::new("").parse().is_err());
    }

    // ------------------------------------------------------------------
    // Name validation (mirrors the validate_job_name call in execute())
    // ------------------------------------------------------------------

    #[test]
    fn valid_names_accepted_by_edit_validation() {
        assert!(validate_job_name("backup").is_ok());
        assert!(validate_job_name("my-job").is_ok());
        assert!(validate_job_name("job_123").is_ok());
        assert!(validate_job_name(&"a".repeat(64)).is_ok());
    }

    #[test]
    fn invalid_names_rejected_by_edit_validation() {
        assert!(validate_job_name("").is_err());
        assert!(validate_job_name("has space").is_err());
        assert!(validate_job_name("has.dot").is_err());
        assert!(validate_job_name(&"a".repeat(65)).is_err());
        assert!(validate_job_name("../traversal").is_err());
    }

    // ------------------------------------------------------------------
    // Timeout validation (mirrors the parse_duration call in execute())
    // ------------------------------------------------------------------

    #[test]
    fn valid_timeout_strings_parsed() {
        assert!(parse_duration("30s").is_some());
        assert!(parse_duration("5m").is_some());
        assert!(parse_duration("1h").is_some());
        assert!(parse_duration("120").is_some()); // bare seconds
    }

    #[test]
    fn invalid_timeout_strings_rejected() {
        assert!(parse_duration("").is_none());
        assert!(parse_duration("abc").is_none());
        assert!(parse_duration("5x").is_none());
    }

    #[test]
    fn timeout_removal_keywords_are_not_parsed_as_duration() {
        // "none" and "off" are handled specially in execute() before
        // parse_duration is called, so they must NOT produce a valid duration.
        assert!(parse_duration("none").is_none());
        assert!(parse_duration("off").is_none());
    }
}
