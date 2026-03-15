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
kron add "every day at 2am" ./backup.sh

# Or use standard cron syntax with a custom name
kron add --name db-backup "0 2 * * *" pg_dump mydb

# See all your jobs
kron list

# Don't wait until 2am — run it now (by ID or name)
kron run db-backup

# What happened?
kron status           # quick overview of all jobs
kron history db-backup   # full run history
kron logs db-backup      # stdout + stderr from last run

# Start the scheduler (runs in background)
kron daemon
```

## Why Not Just Cron?

| | cron | kron |
|---|---|---|
| Output capture | Manual redirect to file | Automatic, queryable |
| Run history | None | Every run stored with exit code + duration |
| Failure detection | Configure sendmail (lol) | `kron status` shows it |
| Job config | One cryptic line in crontab | Readable TOML files |
| Schedule syntax | `0 2 * * *` (memorize it) | `"every day at 2am"` (or cron) |
| Overlap prevention | None (jobs pile up) | Built-in, automatic |
| Timeout | None | Per-job configurable |
| Environment | Stripped to nothing | Inherits your shell env |

## Commands

| Command | What it does |
|---|---|
| `kron add <schedule> <command>` | Add a new job (auto-generates short ID) |
| `kron list` | List all jobs |
| `kron status` | Overview — each job + its last run result |
| `kron history <job>` | Run history with exit codes and durations |
| `kron logs <job>` | Captured stdout + stderr from a run |
| `kron run <job>` | Force-run a job right now |
| `kron remove <job>` | Remove a job |
| `kron daemon` | Start the scheduler (background; `--foreground` for fg) |

## Job Configuration

Jobs live as individual TOML files in `~/.config/kron/jobs/`. Edit them directly or let `kron add` generate one.

```toml
[job]
id = "7a3f2bc1"
name = "backup-db"
command = "pg_dump mydb > /backups/mydb.sql"
schedule = "0 2 * * *"
working_dir = "/app"
enabled = true
timeout = "30m"
```

`id` is auto-generated when you run `kron add`. `name` is optional — a human-friendly label you can pass instead of the ID.

**Schedule format:** Human-readable or standard cron expressions.

```
every day at 2am          → 0 2 * * *
every 5 minutes           → 0 */5 * * * ? *
every monday at 9am       → 0 0 9 * * MON *
at midnight on the 1st    → 0 0 0 1 * ? *
```

Standard cron expressions also work: `0 2 * * *`, `*/5 * * * *`, etc.

## How It Works

```
~/.config/kron/jobs/*.toml    ← you define jobs here (source of truth)
~/.local/share/kron/kron.db   ← kron records every run here (SQLite)
```

The **daemon** runs in the background by default (`kron daemon`). Use `--foreground` to keep it in the terminal. It ticks every second, checks which jobs match the current time, executes them via `sh -c`, captures all output, and writes the result to SQLite. Jobs won't overlap — if a previous run is still going, the next trigger is skipped. Jobs are identified by short auto-generated IDs (e.g. `7a3f2bc1`); all commands accept job ID, ID prefix, or name.

The **CLI** reads both the TOML files and the database to show you what's defined and what actually happened.

## Performance

kron is fast and light. There's no runtime, no garbage collector, no JIT warmup — just a native binary.

| Metric | Value |
|---|---|
| Binary size | **4.2 MB** (statically linked, stripped) |
| Cold start (`kron --help`) | **~7ms** |
| List jobs (`kron list`) | **~7ms** |
| Status query (`kron status`) | **~7ms** (includes SQLite read) |
| Job execution (`kron run`) | **~14ms** overhead (execute + capture + write to DB) |
| Peak memory | **~1 MB** RSS |
| Scheduler tick | **1 second** interval, <1ms per tick |
| Config reload | **10 second** interval (cached, no disk read between reloads) |

Measured on Linux x86_64 with release build (`lto = true`, `codegen-units = 1`, `strip = true`).

For comparison, just *parsing* a crontab in Python takes longer than kron takes to execute a job and record the results.

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
# Add a job (supports human-readable schedules or cron expressions)
kron add "every 5 minutes" "echo hello > /tmp/kron-test.txt"
# Output: Added job a3f7b2c1

# Standard cron expressions also work
kron add "*/5 * * * *" "echo hello > /tmp/kron-test.txt"

# Add a job with a specific name
kron add --name my-task "every hour" "/path/to/script.sh"
# Output: Added job f2e8c4d0
#   Name: my-task
```

Or write TOML directly to `~/.config/kron/jobs/<name>.toml`:
```toml
[job]
id = "f2e8c4d0"
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
# Start in background (default)
kron daemon

# Start in foreground (for process managers like systemd)
kron daemon --foreground

# The daemon:
# - Runs in background by default (PID saved to ~/.local/share/kron/daemon.pid)
# - Checks jobs every 1 second
# - Reloads TOML configs every 10 seconds (hot-reload, no restart needed)
# - Prevents job overlap automatically
# - Captures all stdout/stderr to SQLite
# - Exits cleanly on SIGTERM/SIGINT
# - Logs to ~/.local/share/kron/daemon.log

# Stop the daemon
kill $(cat ~/.local/share/kron/daemon.pid)
```

### File Locations

| Path | Purpose |
|---|---|
| `~/.config/kron/jobs/*.toml` | Job definitions (source of truth — create/edit/delete these) |
| `~/.local/share/kron/kron.db` | SQLite database (run history — read-only for you, kron manages it) |
| `~/.local/share/kron/daemon.log` | Daemon output log |
| `~/.local/share/kron/daemon.pid` | Daemon PID file |
| `~/.local/bin/kron` | Binary (default install location) |

### Key Behaviors for Automation

- **Job IDs** are auto-generated 8-character hex strings. The optional `--name` flag adds a human-friendly label. All commands accept job ID, ID prefix, or name.
- **Human-readable schedules** are supported in `kron add`. They are converted to cron expressions at add time — the TOML file always stores the resolved cron expression.
- **Exit code 0** = success, anything else = failure. Check with `kron status`.
- **TOML is the source of truth** for job definitions. Editing files directly is the intended workflow — changes are picked up within 10 seconds by the daemon.
- **No job overlap** — if a job is still running when its next trigger fires, the trigger is skipped.
- **Daemon** runs in background by default. PID is saved to `~/.local/share/kron/daemon.pid`. Stop with `kill $(cat ~/.local/share/kron/daemon.pid)`. Use `--foreground` for foreground mode.
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

# Start the daemon (backgrounds automatically)
kron daemon
```

---

## License

MIT
