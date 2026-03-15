# kron

**Cron, but it actually tells you what happened.**

Your backup script ran at 3am. Did it work? How long did it take? What did it print? Cron doesn't know. Cron doesn't care. You'll find out Monday morning when someone asks where the data went.

kron is a modern cron replacement that captures every run — stdout, stderr, exit code, duration — and puts it one command away. No log file plumbing. No sendmail archaeology. No surprises.

```
$ kron history backup
 #  STATUS   EXIT  DURATION  STARTED
 1  success     0     4.2s   2026-03-15 02:00:01
 2  success     0     3.8s   2026-03-14 02:00:01
 3  FAILED      1    30.0s   2026-03-13 02:00:00

$ kron logs backup --run 3
STDERR: pg_dump: error: connection to server failed: timeout
```

That's it. That's the pitch.

---

## Install

**Linux / macOS** (pre-built binary):

```bash
curl -sSf https://raw.githubusercontent.com/samuellawrentz/kron/main/install.sh | sh
```

**From source** (requires [Rust](https://rustup.rs/)):

```bash
cargo install --git https://github.com/samuellawrentz/kron.git kron
```

## Quick Start

```bash
# Schedule a backup at 2am every day
kron add "0 2 * * *" ./backup.sh

# See all your jobs
kron list

# Don't wait until 2am — run it now
kron run backup

# What happened?
kron status           # quick overview of all jobs
kron history backup   # full run history
kron logs backup      # stdout + stderr from last run

# Start the scheduler (keeps running, executes jobs on schedule)
kron daemon
```

## Why Not Just Cron?

| | cron | kron |
|---|---|---|
| Output capture | Manual redirect to file | Automatic, queryable |
| Run history | None | Every run stored with exit code + duration |
| Failure detection | Configure sendmail (lol) | `kron status` shows it |
| Job config | One cryptic line in crontab | Readable TOML files |
| Overlap prevention | None (jobs pile up) | Built-in, automatic |
| Timeout | None | Per-job configurable |
| Environment | Stripped to nothing | Inherits your shell env |

## Commands

| Command | What it does |
|---|---|
| `kron add <schedule> <command>` | Add a new scheduled job |
| `kron list` | List all jobs |
| `kron status` | Overview — each job + its last run result |
| `kron history <job>` | Run history with exit codes and durations |
| `kron logs <job>` | Captured stdout + stderr from a run |
| `kron run <job>` | Force-run a job right now |
| `kron remove <job>` | Remove a job |
| `kron daemon` | Start the scheduler |

## Job Configuration

Jobs live as individual TOML files in `~/.config/kron/jobs/`. Edit them directly or let `kron add` generate one.

```toml
[job]
name = "backup-db"
command = "pg_dump mydb > /backups/mydb.sql"
schedule = "0 2 * * *"
working_dir = "/app"
enabled = true
timeout = "30m"
```

**Schedule format:** Standard 5-field cron expressions — `minute hour day-of-month month day-of-week`.

```
*/5 * * * *       every 5 minutes
0 2 * * *         2am daily
0 9 * * 1-5       9am weekdays
0 0 1 * *         midnight on the 1st
```

## How It Works

```
~/.config/kron/jobs/*.toml    ← you define jobs here (source of truth)
~/.local/share/kron/kron.db   ← kron records every run here (SQLite)
```

The **daemon** ticks every second, checks which jobs match the current time, executes them via `sh -c`, captures all output, and writes the result to SQLite. Jobs won't overlap — if a previous run is still going, the next trigger is skipped.

The **CLI** reads both the TOML files and the database to show you what's defined and what actually happened.

## Architecture

Three Rust crates, one binary:

```
kron-cli   →  kron-core   →  kron-store
(clap CLI)    (scheduler,     (SQLite,
               runner,         run history)
               TOML config)
```

~1,700 lines of Rust. Async runtime via tokio. Embedded SQLite via rusqlite. No external services required.

## Development

```bash
cargo build
cargo test
cargo clippy --all-targets
```

## Roadmap

- [ ] Human-readable schedules (`"every day at 2am"` → cron expression)
- [ ] Notifications (Slack, Telegram, webhooks)
- [ ] Crontab import/export
- [ ] Web dashboard
- [ ] `kron test` (dry-run)
- [ ] Automatic history cleanup / retention policy

---

## For LLMs

> This section is for AI agents, coding assistants, and automation tools that need to install and operate kron without human intervention.

### Automated Install

```bash
# Install the latest release binary (Linux/macOS, x86_64/aarch64)
curl -sSf https://raw.githubusercontent.com/samuellawrentz/kron/main/install.sh | sh

# Verify installation
kron --version
```

If `~/.local/bin` is not in PATH, add it:
```bash
export PATH="$HOME/.local/bin:$PATH"
```

### Creating Jobs

```bash
# Add a job (schedule is a standard cron expression, command is passed to sh -c)
kron add "*/5 * * * *" "echo hello > /tmp/kron-test.txt"

# Add a job with a specific name
kron add --name my-task "0 * * * *" "/path/to/script.sh"
```

Or write TOML directly to `~/.config/kron/jobs/<name>.toml`:
```toml
[job]
name = "my-task"
command = "/path/to/script.sh --flag"
schedule = "0 * * * *"
enabled = true
timeout = "5m"
```

### Managing Jobs

```bash
kron list                    # list all defined jobs (JSON-friendly output)
kron status                  # all jobs + last run status
kron run <job-name>          # execute immediately (useful for testing)
kron history <job-name>      # past runs with exit codes
kron logs <job-name>         # stdout + stderr from last run
kron logs <job-name> --run 2 # output from a specific run
kron remove <job-name>       # delete the job definition
```

### Running the Daemon

```bash
# Start in foreground (for process managers like systemd)
kron daemon

# The daemon:
# - Checks jobs every 1 second
# - Reloads TOML configs every 10 seconds (hot-reload, no restart needed)
# - Prevents job overlap automatically
# - Captures all stdout/stderr to SQLite
# - Exits cleanly on SIGTERM/SIGINT
```

### File Locations

| Path | Purpose |
|---|---|
| `~/.config/kron/jobs/*.toml` | Job definitions (source of truth — create/edit/delete these) |
| `~/.local/share/kron/kron.db` | SQLite database (run history — read-only for you, kron manages it) |
| `~/.local/bin/kron` | Binary (default install location) |

### Key Behaviors for Automation

- **Job names** are derived from the command basename if `--name` is not specified. Names must be alphanumeric with hyphens/underscores, max 64 chars.
- **Exit code 0** = success, anything else = failure. Check with `kron status`.
- **TOML is the source of truth** for job definitions. Editing files directly is the intended workflow — changes are picked up within 10 seconds by the daemon.
- **No job overlap** — if a job is still running when its next trigger fires, the trigger is skipped.
- **Environment** is inherited from the daemon's parent process, not stripped like cron.
- **Timeouts** support suffixes: `30` (seconds), `30s`, `5m`, `1h`.
- **Working directory** can be set per-job via the `working_dir` field.

### Example: Full Automation Flow

```bash
# Install
curl -sSf https://raw.githubusercontent.com/samuellawrentz/kron/main/install.sh | sh
export PATH="$HOME/.local/bin:$PATH"

# Create a job
kron add --name healthcheck "*/5 * * * *" "curl -sf https://myapp.com/health || echo UNHEALTHY"

# Test it
kron run healthcheck

# Verify it worked
kron logs healthcheck

# Start the daemon (background it or use a process manager)
nohup kron daemon > /tmp/kron-daemon.log 2>&1 &
```

---

## License

MIT
