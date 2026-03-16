---
name: kron
description: Use kron, a modern cron replacement CLI for scheduling jobs with built-in output capture, run history, and failure tracking. Use when managing scheduled tasks, cron jobs, job monitoring, or when the user mentions kron, crontab, scheduled jobs, or job history.
---

# kron — Modern Cron Replacement

kron is a CLI-first, single-binary cron replacement written in Rust. It solves cron's observability black hole: every run is automatically captured with stdout, stderr, exit code, and duration. No silent failures.

## When to Use This Skill

- User wants to schedule a recurring job or task
- User wants to check if a scheduled job ran successfully
- User wants to see output/logs from a past job run
- User mentions cron, crontab, job scheduling, or kron
- User wants to migrate from system crontab

## Installation

```bash
cargo install kron
```

Single binary, zero runtime dependencies.

## Core Commands

### Add a job

```bash
# With a cron expression
kron add --name backup "0 2 * * *" pg_dump mydb -f /backups/mydb.sql

# With human-readable schedule
kron add --name backup "every day at 2am" pg_dump mydb -f /backups/mydb.sql

# Every minute (for testing)
kron add --name heartbeat "* * * * *" echo "alive"

# Capture current environment variables
kron add --capture-env --name deploy "0 3 * * *" ./deploy.sh
```

The `--name` flag is optional. The schedule comes first (quoted cron expression or human-readable), then the command to run.

### List all jobs

```bash
kron list
```

Output:
```
ID       NAME                 ENABLED  SCHEDULE       RUNS   COMMAND
──────────────────────────────────────────────────────────────────────
a1b2c3d4 backup               yes      0 2 * * *      5/5    pg_dump mydb -f /backups/mydb.sql
e5f6a7b8 heartbeat            yes      * * * * *      10/10  echo "alive"
```

Jobs can be referenced by ID (or prefix) or name in all commands.

### Import from crontab

```bash
kron import              # interactive selection
kron import --all        # import all entries
```

Reads the current user's crontab, displays entries in a numbered list, and lets you select which to import (e.g. `1,3-5` or `all`). Handles `@hourly`/`@daily`/etc. aliases, skips comments, env vars, and `@reboot` entries. Derives job names from inline comments or command basenames.

### Force-run a job now

```bash
kron run backup
```

Executes the job immediately (outside the schedule), captures output, and records the run.

### Dry-run a job

```bash
kron test backup
```

Executes the job but does not record the run. Useful for testing commands.

### Check job status

```bash
kron status
```

Output:
```
NAME                 STATUS     EXIT CODE    LAST RUN
-----------------------------------------------------------------
backup               success    0            2026-03-15 02:00:01 (3s)
heartbeat            success    0            2026-03-15 15:59:00 (0s)
```

All timestamps are displayed in local timezone.

### View run history

```bash
kron history backup
kron history backup -n 20   # last 20 runs
kron history                 # defaults to most recently run job
```

Output:
```
#    STATUS     EXIT CODE    DURATION   STARTED
-----------------------------------------------------------------
1    success    0            3s         2026-03-15 02:00:01
2    success    0            3s         2026-03-14 02:00:01
3    failed     1            1s         2026-03-13 02:00:00
```

### View logs from a run

```bash
kron logs backup           # most recent run
kron logs backup --run 3   # specific run
kron logs                  # defaults to most recently run job
```

Output:
```
=== Job: backup | Run #1 | 2026-03-15 02:00:01 ===
Status: success | Exit code: 0
Duration: 3s

--- stdout ---
DUMP DATABASE mydb
...
```

### Remove a job

```bash
kron remove backup
```

### Manage alerts

```bash
kron alert add-telegram --token <BOT_TOKEN> --chat-id <CHAT_ID>
kron alert add-slack --webhook-url <URL>
kron alert add-webhook --url <URL>
kron alert list
kron alert test            # send test notification to all providers
kron alert remove <index>
```

### Daemon management

```bash
kron daemon start              # run scheduler in foreground
kron daemon start --foreground # explicit foreground mode
kron daemon install            # install as systemd/launchd service
kron daemon uninstall          # remove system service
kron daemon status             # check service status
```

### Self-update

```bash
kron update
```

## Job Configuration (TOML)

Jobs are stored as TOML files in `~/.config/kron/jobs/<id>.toml`:

```toml
[job]
id = "a1b2c3d4"
name = "backup"
command = "pg_dump mydb -f /backups/mydb.sql"
schedule = "0 2 * * *"
working_dir = "/app"
enabled = true
timeout = "30m"

[job.env]
DATABASE_URL = "postgres://localhost/mydb"

[job.alert]
on_failure = true
on_success = false
on_silence = "1h"
```

| Field | Required | Description |
|-------|----------|-------------|
| `id` | Yes | 8-char hex ID (auto-generated) |
| `name` | No | Human-friendly name (alphanumeric, hyphens, underscores, max 64) |
| `command` | Yes | Shell command to execute (run via `sh -c`) |
| `schedule` | Yes | Standard 5-field cron expression |
| `working_dir` | No | Working directory for the command |
| `enabled` | No | `true` (default) or `false` to disable without removing |
| `timeout` | No | Max run duration: `"30s"`, `"5m"`, `"1h"` |
| `env` | No | Environment variables as key-value pairs |
| `alert.on_failure` | No | Alert on non-zero exit (default: true) |
| `alert.on_success` | No | Alert on success (default: false) |
| `alert.on_silence` | No | Dead-man switch: alert if job hasn't run in duration |

## Cron Expression Reference

```
┌───────────── minute (0-59)
│ ┌───────────── hour (0-23)
│ │ ┌───────────── day of month (1-31)
│ │ │ ┌───────────── month (1-12)
│ │ │ │ ┌───────────── day of week (0-6, Sun=0)
│ │ │ │ │
* * * * *
```

Common patterns:

| Expression | Meaning |
|-----------|---------|
| `* * * * *` | Every minute |
| `0 * * * *` | Every hour |
| `0 2 * * *` | Daily at 2:00 AM |
| `0 0 * * 0` | Weekly on Sunday midnight |
| `*/5 * * * *` | Every 5 minutes |
| `0 9-17 * * 1-5` | Hourly, weekdays 9 AM - 5 PM |

## Data Storage

- **Job definitions**: `~/.config/kron/jobs/*.toml` (single source of truth)
- **Run history**: `~/.local/share/kron/kron.db` (SQLite, WAL mode)
- **Alert config**: `~/.config/kron/alerts.toml`

## Built-in Safety Features

- **Overlap prevention**: A job won't start if a previous instance is still running
- **Timeout**: Jobs can be killed after a configurable duration
- **Output capture**: stdout and stderr always recorded (max 1 MiB each), never lost
- **Graceful shutdown**: SIGINT/SIGTERM cleanly stops the daemon
- **File permissions**: Job files 0600, jobs dir 0700, DB file 0600

## Common Workflows

### Migrate from crontab

```bash
kron import                  # select entries interactively
kron daemon install          # install as system service
crontab -r                   # remove old crontab (after verifying)
```

### Debug a failing job

```bash
kron status              # See which jobs are failing
kron history myjob       # See recent run history
kron logs myjob          # See the actual error output
kron test myjob          # Dry-run to test without recording
kron run myjob           # Re-run and record the result
```

### Disable a job temporarily

Edit `~/.config/kron/jobs/<id>.toml` and set `enabled = false`. The daemon picks up changes within 10 seconds.
