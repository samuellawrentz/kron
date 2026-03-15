# kron

Cron, but it actually tells you what happened.

A modern cron replacement written in Rust. CLI-first, single binary, zero config to get started.

## The Problem

Cron is an observability black hole: jobs run silently, stdout and stderr are discarded unless you manually redirect them, failure notifications depend on a working `sendmail` setup that nobody has. You never know if your backup ran, how long it took, or why it failed at 3am.

kron captures every run automatically — output, exit code, duration — and puts it a single command away.

## Install

```bash
cargo install kron
```

## Quick Start

```bash
# Add a job
kron add --name backup "0 2 * * *" ./backup.sh

# List jobs
kron list

# Force-run a job now
kron run backup

# See what happened
kron history backup
kron logs backup

# Start the scheduler
kron daemon
```

## Commands

| Command | Description |
|---|---|
| `kron add <schedule> <command>` | Add a new scheduled job |
| `kron list` | List all jobs |
| `kron status` | Show job status summary (last run, next run) |
| `kron history <job>` | Show run history with exit codes and durations |
| `kron logs <job>` | Show captured output from the last run |
| `kron run <job>` | Force-run a job immediately |
| `kron remove <job>` | Remove a job |
| `kron daemon` | Start the scheduler daemon |

## Job Configuration

Jobs are stored as TOML files in `~/.config/kron/jobs/`. You can edit them directly or let `kron add` create one for you.

```toml
[job]
name = "backup-db"
command = "pg_dump mydb > /backups/mydb.sql"
schedule = "0 2 * * *"
timezone = "Asia/Kolkata"
working_dir = "/app"

[job.env]
DATABASE_URL = "postgres://..."
PATH = "/usr/local/bin:/usr/bin:/bin"

[job.policy]
timeout = "30m"
retry = 3
backoff = "exponential"
skip_if_running = true

[job.alert]
on_failure = true
on_silence = "1h"
```

## How It Works

- **TOML files** in `~/.config/kron/jobs/` define your jobs (single source of truth)
- **SQLite database** in `~/.local/share/kron/kron.db` stores run history
- **Daemon** checks every second for jobs that match, executes them, captures all output
- **CLI** queries both to show you what's happening

## Features

- Automatic output capture (stdout + stderr)
- Run history with exit codes and duration
- Job overlap prevention
- Timeout support
- Human-readable schedule syntax (planned)
- Notifications via webhook/Slack/Telegram (planned)
- Crontab import/export (planned)

## Development

```bash
cargo build
cargo test
cargo clippy --all-targets
```

## Project Structure

Three-crate workspace:

```
crates/
  kron-store/   # SQLite storage layer (job records + run history)
  kron-core/    # Scheduler daemon, async job runner, TOML config parsing
  kron-cli/     # clap-based CLI binary
```

Dependency direction: `kron-cli` → `kron-core` → `kron-store`

## License

MIT
