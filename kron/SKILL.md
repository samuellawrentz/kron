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

# Every minute (for testing)
kron add --name heartbeat "* * * * *" echo "alive"
```

The `--name` flag sets the job name (used in all other commands). If omitted, kron derives a name from the command basename.

The schedule comes first (quoted cron expression), then the command to run.

### List all jobs

```bash
kron list
```

Output:
```
NAME                 ENABLED  SCHEDULE                  COMMAND
---------------------------------------------------------------------------
backup               yes      0 2 * * *                 pg_dump mydb -f /backups/mydb.sql
heartbeat            yes      * * * * *                 echo "alive"
```

### Force-run a job now

```bash
kron run backup
```

Executes the job immediately (outside the schedule), captures output, and records the run.

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

### View run history

```bash
kron history backup
kron history backup -n 20   # last 20 runs
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

### Start the scheduler daemon

```bash
kron daemon
```

Runs in the foreground. Checks every second for jobs due to run. Handles SIGINT and SIGTERM for graceful shutdown.

For production, run as a systemd service:

```ini
[Unit]
Description=kron scheduler daemon
After=network.target

[Service]
ExecStart=/usr/local/bin/kron daemon
Restart=on-failure
User=youruser

[Install]
WantedBy=multi-user.target
```

## Job Configuration (TOML)

Jobs are stored as TOML files in `~/.config/kron/jobs/<name>.toml`:

```toml
[job]
name = "backup"
command = "pg_dump mydb -f /backups/mydb.sql"
schedule = "0 2 * * *"
working_dir = "/app"
enabled = true
timeout = "30m"
```

| Field | Required | Description |
|-------|----------|-------------|
| `name` | Yes | Job identifier (alphanumeric, hyphens, underscores) |
| `command` | Yes | Shell command to execute (run via `sh -c`) |
| `schedule` | Yes | Standard 5-field cron expression |
| `working_dir` | No | Working directory for the command |
| `enabled` | No | `true` (default) or `false` to disable without removing |
| `timeout` | No | Max run duration: `"30s"`, `"5m"`, `"1h"` |

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
- **Run history**: `~/.local/share/kron/kron.db` (SQLite)

## Built-in Safety Features

- **Overlap prevention**: A job won't start if a previous instance is still running
- **Timeout**: Jobs can be killed after a configurable duration
- **Output capture**: stdout and stderr always recorded, never lost
- **Graceful shutdown**: SIGINT/SIGTERM cleanly stops the daemon

## Common Workflows

### Migrate from crontab

1. View existing crontab: `crontab -l`
2. For each entry, add to kron:
   ```bash
   kron add --name <descriptive-name> "<schedule>" <command>
   ```
3. Start the daemon: `kron daemon`
4. Remove from crontab: `crontab -r` (after verifying kron runs correctly)

### Debug a failing job

```bash
kron status              # See which jobs are failing
kron history myjob       # See recent run history
kron logs myjob          # See the actual error output
kron run myjob           # Re-run manually to test
```

### Disable a job temporarily

Edit `~/.config/kron/jobs/myjob.toml` and set `enabled = false`. The daemon picks up changes within 10 seconds.
