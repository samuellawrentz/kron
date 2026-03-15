# kron — Product Requirements Document

| Field | Value |
|-------|-------|
| **Version** | 0.1.0 |
| **Date** | 2026-03-15 |
| **Status** | Draft |
| **License** | MIT |
| **Repository** | github.com/samm/kron |

---

## Executive Summary

kron is an open-source, CLI-first cron replacement written in Rust. It is a single binary that wraps the familiar cron model with automatic output capture, run history, environment management, secrets handling, and failure alerting — all with zero configuration. Where cron silently discards output, swallows failures, and offers no record that a job ever ran, kron captures everything by default and makes it queryable from the command line. It targets solo developers, DevOps engineers, SREs, and indie hackers who run scheduled jobs on one or a handful of servers and are tired of building observability scaffolding around cron.

---

## Problem Statement

Cron's problem is not its syntax — it is the **observability black hole**.

Jobs run in a stripped-down environment with a minimal `PATH`, produce no captured output by default, send no alerts on failure, and have no mechanism to confirm they ran at all. The result is an entire class of silent production incidents:

> "Critical database backup hadn't executed for 11 days, yet all monitoring systems showed green."
> — JP Broeders, DEV.to

> "One day, a critical job silently died. No error. No alert. No email. Just silence."
> — CronBeats, DEV.to

> "Traditional monitoring is terrible at catching things that don't happen."
> — CronMonitor blog

Environment mismatches are the #1 debugging time sink. Cron runs with `PATH=/usr/bin:/bin`, does not source shell profiles, defaults to `/bin/sh`, and provides no working directory context. Every developer learns this the hard way — and then learns it again six months later.

Failed jobs simply disappear. No retry, no backoff, no dead-letter queue, no record. An entire product category (Cronitor, Healthchecks.io, Dead Man's Snitch) exists purely to fill the gap that cron leaves open.

The competitive landscape confirms the opportunity: monitoring tools (Cronitor, Healthchecks.io) only watch, container schedulers (Supercronic, Ofelia) only run, and workflow platforms (Windmill, Temporal) are overkill for "run this script at 2am." **No single tool owns manage + monitor + history + good UX.** That quadrant is empty. kron fills it.

---

## Target Users & Personas

### 1. Solo Developer — "Alex"
- Runs 3-10 cron jobs on a single VPS (backups, cleanup scripts, data pulls)
- Has been burned by silent failures at least once
- Wants something that works out of the box with no SaaS dependency
- Values simplicity over configurability

### 2. DevOps Engineer — "Jordan"
- Manages 20-50 scheduled jobs across a small fleet
- Currently maintains wrapper scripts for logging, alerting, and lock files around every cron job
- Wants a single tool to replace the wrapper script ecosystem
- Cares about crontab import/export for migration

### 3. SRE — "Sam"
- Responsible for reliability of scheduled jobs in production
- Needs run history, duration trends, and failure categorization for postmortems
- Wants alerting that integrates with existing Slack/Telegram channels
- Values structured data (exit codes, durations, error categories) over raw logs

### 4. Indie Hacker — "Riley"
- Ships side projects on cheap infrastructure
- Runs background jobs (email sends, report generation, scraping) on a single server
- Wants `cargo install kron` and done — no Docker, no Kubernetes, no cloud services
- Appreciates human-readable schedule syntax over memorizing cron fields

---

## Product Vision

```
                    Simple <<------------------>> Complex
                    |                                   |
         CLI-first  |   kron *                          |  Dkron
                    |                                   |
                    |                                   |
                    |   Supercronic                      |  Windmill
      Runner-only   |   Ofelia                          |  Temporal
                    |                                   |
                    |                                   |
   Monitor-only     |   Healthchecks.io                 |  Cronitor
                    |                                   |
```

kron occupies the **"simple + CLI-first + full-featured"** quadrant. It is not a distributed scheduler, not a workflow engine, and not a monitoring SaaS. It is cron with the observability gap closed, distributed as a single binary with zero dependencies.

**Design Principles:**

1. **Single binary, zero dependencies** — `cargo install kron` and done. No runtime, no daemon dependencies.
2. **Zero-config observability** — Every run is logged automatically. stdout, stderr, exit code, duration, environment — all captured without redirect boilerplate.
3. **Cron-compatible** — Import existing crontabs, export back to cron format. Migration is a one-liner.
4. **Single file per job** — Each job is a TOML file. Version-control native. No systemd-style two-file dance.
5. **CLI-first** — The terminal is the primary interface. An optional web dashboard comes later and is read-only.

---

## Core Features

### Tier 1 — MVP (v0.1)

#### `kron add` — Job Creation

Create jobs from the command line with a human-readable schedule DSL or traditional cron expressions.

**Quick add:**
```bash
kron add "every day at 2am" ./backup.sh
kron add "weekdays at 9:00" /scripts/report.sh --name daily-report
kron add "*/15 * * * *" ./health-check.sh --name health-check
```

**Guided interactive setup:**
```bash
$ kron add --interactive
Job name: backup-db
Command: pg_dump mydb > /backups/mydb.sql
Schedule (human-readable or cron): every day at 2am
Working directory [/home/user]: /app
Capture environment? [Y/n]: y
Timeout (e.g. 30m, 1h) [none]: 30m

Created job 'backup-db' — next run: 2026-03-16 02:00:00
```

**From a TOML definition file:**
```bash
kron add --file backup-db.toml
```

**Schedule DSL examples:**
| Human-readable | Cron equivalent |
|---|---|
| `every day at 2am` | `0 2 * * *` |
| `weekdays at 9:00` | `0 9 * * 1-5` |
| `every 15 minutes` | `*/15 * * * *` |
| `every hour` | `0 * * * *` |
| `sundays at midnight` | `0 0 * * 0` |
| `1st of every month at 6am` | `0 6 1 * *` |
| `every 6 hours` | `0 */6 * * *` |

Standard cron expressions (`*/5 * * * *`) are always accepted as-is.

---

#### `kron list` — List All Jobs

Display all registered jobs in a clean table format.

```bash
$ kron list
Name           Schedule              Status    Last Run             Last Result   Next Run
─────────────  ────────────────────  ────────  ───────────────────  ────────────  ───────────────────
backup-db      every day at 2am      enabled   2026-03-15 02:00:01  success (0)   2026-03-16 02:00:00
daily-report   weekdays at 9:00      enabled   2026-03-14 09:00:00  failed (1)    2026-03-17 09:00:00
health-check   */15 * * * *          enabled   2026-03-15 14:45:00  success (0)   2026-03-15 15:00:00
cleanup        sundays at midnight   disabled  2026-03-09 00:00:02  success (0)   —
```

**Flags:**
- `kron list --json` — machine-readable output
- `kron list --enabled` / `kron list --disabled` — filter by state
- `kron list --quiet` — names only (useful for scripting)

---

#### `kron run` / `kron test` — Manual Execution and Dry-Run

**Force run** a job immediately, outside its schedule. Output streams to the terminal by default:

```bash
$ kron run backup-db
[2026-03-15 14:32:01] Running 'backup-db'...
pg_dump: dumping database "mydb"
pg_dump: complete

Finished in 12.4s — exit code 0 (success)
```

**Dry-run** validates the job configuration, resolves the environment, and shows what would happen without executing the command:

```bash
$ kron test backup-db
Dry-run for 'backup-db':
  Command:      pg_dump mydb > /backups/mydb.sql
  Working dir:  /app
  Environment:  12 vars (DATABASE_URL, PATH, ... +10)
  Timeout:      30m
  Next run:     2026-03-16 02:00:00
  Schedule:     every day at 2am (0 2 * * *)

Environment looks valid. Command found in PATH.
```

---

#### `kron logs` — Output Capture and Live Streaming

Every run automatically captures stdout and stderr. No redirect boilerplate needed.

**View last run output:**
```bash
$ kron logs backup-db
[run #47 — 2026-03-15 02:00:01 — exit 0 — 12.4s]

pg_dump: dumping database "mydb"
pg_dump: reading schemas
pg_dump: reading user-defined tables
pg_dump: complete
```

**View output from a specific run:**
```bash
kron logs backup-db --run 45
```

**Live tail of a running job** (works like `tail -f`):
```bash
$ kron logs backup-db --tail
Waiting for 'backup-db' to start...
[2026-03-16 02:00:01] pg_dump: dumping database "mydb"
[2026-03-16 02:00:03] pg_dump: reading schemas
[2026-03-16 02:00:05] pg_dump: reading user-defined tables
  ... (streaming) ^C to detach
```

**Flags:**
- `--tail` — follow live output (attach to running job or wait for next run)
- `--run <N>` — view output from a specific run number
- `--stderr` — show only stderr
- `--last <N>` — show output from the last N runs

**Streaming implementation notes:**
- v0.1 uses simple pipes (`tokio::process::Command` with `Stdio::piped()`), not PTY allocation. Programs that buffer stdout when not connected to a TTY (Python, Ruby) may appear delayed — document `stdbuf -oL` or `PYTHONUNBUFFERED=1` as workarounds.
- Job config supports `unbuffered = true` which wraps the command with `stdbuf -oL` automatically.
- Logs are written to disk files (`~/.local/share/kron/logs/<job>/<run-id>.log`), not SQLite blobs. `--tail` readers tail the file — this survives kron daemon restarts.
- Per-run log size cap: 10MB default (configurable via `max_output` in job policy). Truncated with a warning, not a silent drop.
- `kron run <job>` streams output to the terminal by default (tee to both terminal and log file).

---

#### `kron status` — Overview Dashboard

Show a summary of all jobs and the system's overall health:

```bash
$ kron status
kron daemon: running (pid 1234, uptime 4d 12h)
Jobs: 4 total, 3 enabled, 1 disabled
Last 24h: 28 runs, 26 succeeded, 2 failed

Recent failures:
  daily-report  2026-03-14 09:00:00  exit 1  "Connection refused"
  daily-report  2026-03-13 09:00:00  exit 1  "Connection refused"

Upcoming:
  health-check  in 12 minutes  (2026-03-15 15:00:00)
  backup-db     in 11 hours    (2026-03-16 02:00:00)
```

---

#### Environment Capture

When a job is created, kron snapshots the current `PATH`, environment variables, working directory, and shell. At runtime, the job executes in this captured environment — eliminating the #1 cron debugging headache.

```bash
# Environment is captured automatically at creation time
$ kron add "every day at 2am" ./backup.sh --name backup-db

# View what was captured
$ kron env backup-db
Captured environment for 'backup-db':
  PATH=/usr/local/bin:/usr/bin:/bin:/home/user/.cargo/bin
  HOME=/home/user
  SHELL=/bin/bash
  DATABASE_URL=postgres://localhost/mydb
  ... (8 more)
  Working dir: /home/user/scripts
  Captured at: 2026-03-15 14:30:00
```

Environment variables can also be set explicitly in the TOML job definition (see Job Definition Format below), which takes precedence over captured values.

---

#### SQLite Storage — Automatic Run History

All run history is stored in a local SQLite database (`~/.local/share/kron/kron.db`). Zero configuration required — the database is created automatically on first run.

```bash
$ kron history backup-db
Run  Started              Duration  Exit  Result
───  ───────────────────  ────────  ────  ──────────────
#47  2026-03-15 02:00:01  12.4s     0     success
#46  2026-03-14 02:00:01  11.8s     0     success
#45  2026-03-13 02:00:01  45.2s     0     success
#44  2026-03-12 02:00:01  12.1s     1     failed: timeout
#43  2026-03-11 02:00:01  11.9s     0     success

Showing 5 of 47 runs. Use --all for full history.
```

**Flags:**
- `--all` — show complete history
- `--last <N>` — show last N runs (default: 10)
- `--failures` — show only failed runs
- `--json` — machine-readable output

---

#### `kron import` / `kron export` — Crontab Compatibility

Zero-friction migration from existing crontab. This is the primary adoption wedge — users will not manually re-enter 30 crontab entries. **The `kron import` -> `kron status` path must work in under 60 seconds.**

```bash
# Import all jobs from system crontab
$ kron import
Found 5 cron entries. Importing...
  [1/5] */5 * * * * /scripts/health.sh -> health-sh (every 5 minutes)
  [2/5] 0 2 * * * /scripts/backup.sh  -> backup-sh (every day at 2am)
  ...
Imported 5 jobs. Review with 'kron list'.

# Preview without importing
$ kron import --dry-run

# Import from a file instead of system crontab
$ kron import --file my-crontab.txt

# Export back to crontab format
$ kron export
# Generated by kron — 2026-03-15
*/5 * * * * /scripts/health.sh
0 2 * * * /scripts/backup.sh
...

# Export to file
kron export > my-crontab.txt
```

**Import guarantees:** Imported jobs must be behavior-identical to their cron originals — same timing, environment semantics, and working directory. If kron changes any behavior on import, users will not trust it.

---

#### Exit Code Tracking and Error Capture

Every run records:
- **Exit code** — numeric value from the process
- **Result category** — `success`, `failed`, `timeout`, `killed`, `skipped`
- **Error reason** — captured from stderr (first meaningful line) when exit code is non-zero
- **Duration** — wall-clock time from start to finish
- **Timestamps** — started_at, finished_at

```bash
$ kron history daily-report --failures
Run  Started              Duration  Exit  Result
───  ───────────────────  ────────  ────  ─────────────────────────────
#12  2026-03-14 09:00:00  0.3s      1     failed: "Connection refused"
#11  2026-03-13 09:00:00  0.2s      1     failed: "Connection refused"
#8   2026-03-10 09:00:00  30m 0s    —     timeout (limit: 30m)
#5   2026-03-07 09:00:00  0.1s      127   failed: "command not found"
```

---

### Tier 2 — v0.2

#### `kron stats` — Aggregate Statistics

Show aggregate statistics for a job or across all jobs.

**Per-job statistics:**
```bash
$ kron stats backup-db
Job: backup-db
Schedule: every day at 2am (Asia/Kolkata)
Status: active, last run FAILED (2h ago, exit 1)

              Last 24h    Last 7d     Last 30d    All time
  Runs:            1          7          30         412
  Passed:          0          5          26         398
  Failed:          1          2           4          14
  Success:       0.0%      71.4%      86.7%       96.6%

Duration (last 30 runs):
  p50: 4m 12s    p95: 8m 30s    p99: 12m 01s    max: 14m 22s
  Trend: +18% over 30d ▲

Recent failures:
  #412  2h ago     exit 1    "pg_dump: connection refused"
  #405  5d ago     exit 1    "pg_dump: connection refused"
  #398  12d ago    timeout   killed after 30m
  #391  19d ago    exit 1    "pg_dump: relation does not exist"

Top error reasons (all time):
  connection refused     9 (64%)
  timeout                3 (21%)
  relation not exist     2 (14%)
```

**Global statistics:**
```bash
$ kron stats
Jobs: 12 active, 2 disabled, 14 total

Last 24h                    Last 7d
  Runs:       847             Runs:       5,891
  Passed:     831 (98.1%)     Passed:     5,702 (96.8%)
  Failed:      14 (1.7%)      Failed:        167 (2.8%)
  Skipped:      2 (0.2%)      Skipped:        22 (0.4%)

Trouble spots (>5% failure rate, last 7d):
  backup-db        12/168 failed (7.1%)   last failure: 2h ago
  sync-cdn          8/168 failed (4.8%)   last failure: 14h ago
  deploy-staging    3/42 failed  (7.1%)   last failure: 3d ago
```

**Design notes:**
- Time windows are fixed (24h, 7d, 30d, all-time) — not configurable in v0.1
- "Trouble spots" filters to jobs above 5% failure rate in last 7 days
- Duration percentiles require storing `duration_ms` per run from day one (cannot backfill)
- Error reason grouping: simple substring deduplication of last stderr line. No NLP clustering in v0.1

**Flags:**
- `--period <7d|30d|90d|all>` — time window (default: 30d)
- `--json` — machine-readable output

> **`--json` on every command:** All kron commands that produce output support `--json` for machine-readable structured output. This is critical for OSS adoption — users will pipe stats into their own dashboards, monitoring systems, and scripts. Design `--json` in from day one; retrofitting structured output is painful.

---

#### `kron env` — Environment Management

View, edit, and validate environment variables per job.

```bash
# View captured environment
kron env backup-db

# Add/update a variable
kron env backup-db set PGHOST=db.example.com

# Remove a variable
kron env backup-db unset TEMP_VAR

# Validate environment (check PATH entries exist, commands resolve)
kron env backup-db validate
```

---

#### `kron secret` — Secrets Management

Store sensitive values (API keys, tokens, passwords) separately from job definitions. Secrets are encrypted at rest and injected into the job's environment at runtime. Secret values are never printed in logs, output, or `kron env` listings.

```bash
# Add a secret
$ kron secret set backup-db DB_PASSWORD
Enter value: ********
Secret 'DB_PASSWORD' saved for job 'backup-db'.

# List secrets (values are never shown)
$ kron secret list backup-db
Name            Set At
──────────────  ───────────────────
DB_PASSWORD     2026-03-15 14:30:00
AWS_SECRET_KEY  2026-03-10 09:00:00

# Remove a secret
kron secret unset backup-db DB_PASSWORD

# Rotate a secret
$ kron secret set backup-db DB_PASSWORD
Value exists. Overwrite? [y/N]: y
Enter value: ********
Secret 'DB_PASSWORD' updated.
```

**v0.1 approach — Environment variable interpolation (recommended):**

TOML job files support `${VAR}` interpolation, resolved at runtime from the process environment or a `.env` file per job. This integrates with existing secret injection tools (Vault agent, systemd `EnvironmentFile=`, Docker `--env-file`) without reinventing secret storage.

```toml
[job.env]
DATABASE_URL = "postgres://${DB_USER}:${DB_PASSWORD}@localhost/mydb"
```

```bash
# Per-job .env file (gitignored, 0600 permissions)
# ~/.config/kron/jobs/backup-db.env
DB_PASSWORD=hunter2
AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI...
```

**v0.2+ — Encrypted secret store (optional upgrade):**
1. **Encrypted file** — AES-256-GCM encrypted file at `~/.local/share/kron/secrets.enc`, key derived from a master passphrase via Argon2id
2. **OS keyring** — macOS Keychain, Linux Secret Service — used when available, but NOT the default (headless servers and Docker containers typically lack a keyring daemon)

**Design principle:** kron is not a secrets manager. It integrates with them. The `.env` file approach is the pragmatic default that works everywhere.

**Redaction rules:**
- Secret values are replaced with `[REDACTED]` in all log output
- Secret values are never included in `kron env` output
- Secret names (not values) appear in `kron secret list`
- TOML job files reference secrets by name, never by value: `DB_PASSWORD = { secret = true }`

---

#### Alerting — Failure and Silence Notifications

Configure notification channels and alert rules per job or globally.

```bash
# Add a Telegram channel
kron alert add telegram --token BOT_TOKEN --chat-id CHAT_ID

# Add a Slack webhook
kron alert add slack --webhook-url https://hooks.slack.com/...

# Add a generic webhook
kron alert add webhook --url https://example.com/hook

# Test the alert channel
kron alert test telegram

# Configure per-job alert rules in TOML (see Job Definition Format)
```

**Alert triggers:**
- `on_failure` — alert when a job exits non-zero (default: on)
- `on_silence` — dead-man switch: alert if no run completes within a time window
- `on_recovery` — alert when a previously failing job succeeds again

---

#### Retry with Backoff

Configurable retry count and backoff strategy per job.

```toml
[job.policy]
retry = 3
backoff = "exponential"   # "fixed", "linear", "exponential"
retry_delay = "10s"       # base delay between retries
```

Retry attempts are logged as part of the same run record. The run is marked as `success` if any attempt succeeds, or `failed` with the last attempt's error if all retries are exhausted.

---

#### Overlap Prevention

Prevent a new instance of a job from starting while a previous instance is still running. Enabled by default.

```toml
[job.policy]
skip_if_running = true    # default: true
```

When a scheduled trigger fires and the previous instance is still running, the trigger is logged as `skipped (previous run still active)`.

---

### Tier 3 — v0.3+

#### Timezone Per Job

```toml
[job]
timezone = "Asia/Kolkata"
```

Schedules are evaluated in the specified timezone, with correct DST handling. If no timezone is specified, the system timezone is used.

---

#### Dependency Chains

```toml
[job.depends]
after = ["backup-db"]     # run only after backup-db succeeds
```

If a dependency fails, the dependent job is skipped and logged as `skipped (dependency 'backup-db' failed)`.

---

#### Web Dashboard

An optional, read-only web dashboard served by the kron daemon. Provides a visual overview of jobs, run history, and statistics. Not a management surface — all changes are made via CLI.

```bash
kron dashboard --port 8080
```

---

#### Progress Indicators for Long-Running Jobs

For jobs that emit progress information, kron can display a progress bar in the terminal during `kron run`:

```bash
$ kron run backup-db
[2026-03-15 14:32:01] Running 'backup-db'...
[=========>                    ] 33% — dumping table 'users' (3/9 tables)
```

Progress is detected from stdout patterns (e.g., percentage markers, line counts). Jobs can also emit structured progress via a simple protocol: lines matching `KRON_PROGRESS:<percent>:<message>` are parsed and displayed.

---

## CLI Reference

| Command | Description | Key Flags |
|---|---|---|
| `kron add <schedule> <command>` | Create a new job | `--name`, `--file`, `--interactive`, `--working-dir`, `--timeout` |
| `kron list` | List all registered jobs | `--json`, `--enabled`, `--disabled`, `--quiet` |
| `kron status` | Show system overview and job health | `--json` |
| `kron run <job>` | Force-run a job immediately | `--quiet` (suppress streaming) |
| `kron test <job>` | Dry-run: validate config without executing | — |
| `kron logs <job>` | View captured output | `--tail`, `--run <N>`, `--stderr`, `--last <N>` |
| `kron history <job>` | View run history | `--all`, `--last <N>`, `--failures`, `--json` |
| `kron stats [job]` | Aggregate run statistics | `--period <7d\|30d\|90d\|all>`, `--json` |
| `kron env <job>` | View/manage environment variables | `set <K=V>`, `unset <K>`, `validate` |
| `kron secret <action> <job>` | Manage secrets | `set`, `unset`, `list` |
| `kron edit <job>` | Edit job definition in $EDITOR | — |
| `kron remove <job>` | Remove a job and its history | `--keep-history` |
| `kron enable <job>` | Enable a disabled job | — |
| `kron disable <job>` | Disable a job without removing it | — |
| `kron alert add <channel>` | Add notification channel | `--token`, `--chat-id`, `--webhook-url`, `--url` |
| `kron alert test [channel]` | Send a test notification | — |
| `kron import` | Import from system crontab | `--file <path>`, `--dry-run` |
| `kron export` | Export to crontab format | `--file <path>` |
| `kron dashboard` | Start the read-only web dashboard | `--port <N>` |

---

## Job Definition Format (TOML)

Each job is defined as a single TOML file. Jobs can be created via `kron add` (which generates the file) or by writing the TOML directly.

**Job files are stored in:** `~/.config/kron/jobs/<name>.toml`

### Full Example

```toml
[job]
name = "backup-db"
description = "Nightly database backup to S3"
command = "pg_dump mydb | gzip > /backups/mydb-$(date +%F).sql.gz"
schedule = "every day at 2am"          # human-readable or cron expression
timezone = "America/New_York"
working_dir = "/app"
shell = "/bin/bash"                    # default: $SHELL or /bin/sh
enabled = true                         # default: true

[job.env]
PATH = "/usr/local/bin:/usr/bin:/bin"
DATABASE_URL = "postgres://localhost/mydb"
PGHOST = "localhost"

[job.secrets]
# Reference secrets stored via 'kron secret set'
# Values are injected at runtime, never stored in this file
DB_PASSWORD = { secret = true }
AWS_SECRET_ACCESS_KEY = { secret = true }

[job.policy]
timeout = "30m"                        # kill job after this duration
retry = 3                              # retry on failure (0 = no retry)
backoff = "exponential"                # "fixed", "linear", "exponential"
retry_delay = "10s"                    # base delay between retries
skip_if_running = true                 # default: true
max_output = "10MB"                    # truncate captured output after this

[job.alert]
on_failure = true                      # alert on non-zero exit (default: true)
on_silence = "1h"                      # alert if no run completes within window
on_recovery = true                     # alert when job recovers after failure
channels = ["telegram", "slack"]       # which alert channels to use

[job.depends]
after = ["backup-files"]               # run after these jobs succeed
```

### Minimal Example

```toml
[job]
name = "health-check"
command = "./check.sh"
schedule = "every 5 minutes"
```

That is all that is required. kron fills in sensible defaults for everything else: environment captured at creation, `skip_if_running = true`, alerts on failure enabled, output captured automatically.

---

## Data Model

### SQLite Schema

```sql
-- Job definitions (source of truth is TOML files; this is a cache/index)
CREATE TABLE jobs (
    id          TEXT PRIMARY KEY,       -- job name (unique)
    config_path TEXT NOT NULL,          -- path to TOML file
    schedule    TEXT NOT NULL,          -- normalized cron expression
    enabled     INTEGER NOT NULL DEFAULT 1,
    created_at  TEXT NOT NULL,          -- ISO 8601
    updated_at  TEXT NOT NULL           -- ISO 8601
);

-- Run history
CREATE TABLE runs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id      TEXT NOT NULL REFERENCES jobs(id),
    run_number  INTEGER NOT NULL,       -- per-job sequential counter
    started_at  TEXT NOT NULL,          -- ISO 8601
    finished_at TEXT,                   -- ISO 8601 (NULL if still running)
    duration_ms INTEGER,               -- wall-clock milliseconds
    exit_code   INTEGER,               -- process exit code (NULL if timeout/killed)
    result      TEXT NOT NULL,          -- 'success', 'failed', 'timeout', 'killed', 'skipped'
    error       TEXT,                   -- first meaningful stderr line on failure
    retry_count INTEGER NOT NULL DEFAULT 0,  -- number of retries attempted
    trigger     TEXT NOT NULL DEFAULT 'schedule',  -- 'schedule', 'manual', 'dependency'
    UNIQUE(job_id, run_number)
);

-- Run output paths (logs stored as files on disk, NOT in SQLite)
-- Large blobs degrade SQLite query performance and bloat the database.
CREATE TABLE run_output (
    run_id      INTEGER PRIMARY KEY REFERENCES runs(id),
    log_path    TEXT NOT NULL,          -- path to log file: ~/.local/share/kron/logs/<job>/<run-id>.log
    log_size    INTEGER                 -- file size in bytes
);

-- Indexes
CREATE INDEX idx_runs_job_id ON runs(job_id);
CREATE INDEX idx_runs_started_at ON runs(started_at);
CREATE INDEX idx_runs_result ON runs(result);
```

**Storage locations:**
- Database: `~/.local/share/kron/kron.db`
- Run logs: `~/.local/share/kron/logs/<job-name>/<run-id>.log`
- Job configs: `~/.config/kron/jobs/<name>.toml`
- Job env files: `~/.config/kron/jobs/<name>.env`

**Retention:** Run history metadata is kept indefinitely by default. Log files on disk are subject to a configurable retention policy (default: 90 days). A `kron gc` command prunes old log files and their corresponding `run_output` rows.

---

## Secrets & Security

### Threat Model

kron handles two categories of sensitive data:
1. **Secrets** (API keys, passwords, tokens) — must be encrypted at rest, never logged
2. **Job output** — may inadvertently contain sensitive data; redaction is best-effort

### Secret Storage

Secrets are managed through a layered strategy:

| Priority | Backend | When Used |
|---|---|---|
| 1 (default) | **Environment variable interpolation** | `${VAR}` in TOML, resolved from process env or per-job `.env` file. Works everywhere, integrates with Vault, systemd, Docker. |
| 2 (v0.2+) | **Encrypted file** | `~/.local/share/kron/secrets.enc` — AES-256-GCM, key derived from master passphrase via Argon2id |
| 3 (v0.2+) | **OS keyring** | macOS Keychain, Linux Secret Service — optional, NOT default (headless servers lack keyring daemons) |

**v0.1 behavior:** `kron secret set` stores values in the per-job `.env` file with `0600` permissions. The `.env` file is the pragmatic default that works on every platform including headless servers and containers. kron is not a secrets manager — it integrates with them.

**Warning system:** kron warns when it detects what appears to be a hardcoded secret in a TOML `[job.env]` section (e.g., strings matching password/token/key patterns) and suggests using `${VAR}` interpolation instead.

### Environment Isolation

- Jobs run in a **controlled environment** — only the captured/configured variables are set
- The parent process environment is not leaked into job execution
- `PATH` is explicitly set from the captured or configured value
- Working directory is set from the job configuration

### Redaction Rules

| Context | Behavior |
|---|---|
| `kron logs` | Secret values replaced with `[REDACTED]` |
| `kron env` | Secret-backed vars show `[SECRET]` instead of value |
| `kron history` | Error messages are captured from stderr; secrets are redacted |
| TOML job files | Secrets are referenced by name only (`{ secret = true }`) |
| SQLite database | Output is stored as captured; secrets are redacted before storage |
| Alert notifications | Secret values are redacted in all notification payloads |

### File Permissions

| File | Permissions |
|---|---|
| `~/.config/kron/jobs/*.toml` | `0600` (owner read/write) |
| `~/.local/share/kron/kron.db` | `0600` |
| `~/.local/share/kron/secrets.enc` | `0600` |

---

## AI Agent Integration

AI agents (Claude Code, Codex, Devin, custom LLM agents, CI/CD bots) are first-class users of kron. Agents create, monitor, and react to scheduled jobs programmatically. Every feature should work without human interaction — no interactive prompts required, structured output everywhere, machine-parseable errors.

### Design Principle

**If a human can do it in the terminal, an agent can do it in a script.** Every kron command must work non-interactively with `--json` output. No command should require TTY input unless explicitly using `--interactive`.

### Agent-Specific Features

#### Tier 1 (v0.1) — Built into core commands

**`--json` on every command (already planned)**

Every command that produces output supports `--json`. This is not optional — it's how agents consume kron.

```bash
# Agent checks if a job is healthy
kron status --json | jq '.jobs[] | select(.name == "backup-db") | .last_result'

# Agent lists all failing jobs
kron list --json | jq '.[] | select(.last_result != "success")'
```

**`kron add --from-stdin` — Pipe job definitions**

Agents generate TOML programmatically. They shouldn't need temp files.

```bash
# Agent creates a job from generated TOML
cat <<EOF | kron add --from-stdin
[job]
name = "agent-data-sync"
command = "/scripts/sync.sh"
schedule = "every 6 hours"
[job.env]
API_KEY = "\${SYNC_API_KEY}"
EOF

# Or pipe from another tool
generate-job-config | kron add --from-stdin
```

**`KRON_*` environment variables injected into every job**

Every job gets metadata about its own execution context:

```bash
KRON_JOB_NAME=backup-db        # Job name
KRON_RUN_ID=47                  # Sequential run number
KRON_RUN_UUID=a1b2c3d4...      # Unique run identifier
KRON_TRIGGER=schedule           # "schedule", "manual", "dependency"
KRON_ATTEMPT=1                  # Retry attempt number (1 = first try)
KRON_TIMEOUT=1800               # Timeout in seconds (0 = no timeout)
KRON_LOG_FILE=/path/to/run.log  # Path to this run's log file
```

Scripts can use these for self-reporting:
```bash
#!/bin/bash
echo "Run $KRON_RUN_ID starting (attempt $KRON_ATTEMPT)"
# ... do work ...
echo "KRON_PROGRESS:75:migrated 3/4 tables"
```

**Structured error output**

When `--json` is active, errors are also JSON:
```json
{"error": "job 'backup-db' not found", "code": "JOB_NOT_FOUND", "suggestion": "run 'kron list' to see available jobs"}
```

Error codes are a fixed enum agents can switch on: `JOB_NOT_FOUND`, `JOB_ALREADY_EXISTS`, `INVALID_SCHEDULE`, `INVALID_TOML`, `DAEMON_NOT_RUNNING`, `JOB_ALREADY_RUNNING`, `TIMEOUT`, `PERMISSION_DENIED`.

---

#### Tier 2 (v0.2) — Agent-optimized observability

**`kron health [job]` — Single-call health check**

One command that returns everything an agent needs to decide "is this okay?":

```bash
$ kron health backup-db --json
{
  "job": "backup-db",
  "healthy": false,
  "status": "enabled",
  "success_rate_24h": 0.0,
  "success_rate_7d": 0.714,
  "last_run": {
    "result": "failed",
    "exit_code": 1,
    "error": "pg_dump: connection refused",
    "error_category": "connection_error",
    "ago": "2h"
  },
  "next_run": "2026-03-16T02:00:00",
  "next_in_seconds": 43200,
  "trend": "degrading",
  "consecutive_failures": 1
}

# Global health check
$ kron health --json
{
  "overall": "degraded",
  "jobs_total": 12,
  "jobs_healthy": 10,
  "jobs_degraded": 1,
  "jobs_failing": 1,
  "trouble_spots": ["backup-db"]
}
```

Agents use this for automated decisions:
- `healthy: true` → do nothing
- `healthy: false` + `error_category: connection_error` → check if DB is up, restart if needed
- `healthy: false` + `trend: degrading` → alert the team
- `consecutive_failures > 3` → escalate to human

**Error categorization**

kron classifies errors from stderr patterns into machine-actionable categories:

| Category | Pattern | Agent action |
|---|---|---|
| `connection_error` | "connection refused", "no route to host", "timeout" | Check upstream service, retry |
| `auth_error` | "permission denied", "access denied", "401", "403" | Rotate credentials |
| `not_found` | "command not found", "no such file", "404" | Fix path or deploy missing artifact |
| `timeout` | Job killed by timeout policy | Increase timeout or optimize job |
| `oom_killed` | Exit 137, "killed", "out of memory" | Increase memory or reduce batch size |
| `disk_full` | "no space left", "disk quota exceeded" | Clean up disk, alert ops |
| `dependency_error` | "module not found", "import error" | Fix dependencies |
| `unknown` | Anything else | Log and alert human |

**Structured log events — `kron logs --json`**

Newline-delimited JSON for agent consumption:

```bash
$ kron logs backup-db --json
{"ts":"2026-03-15T02:00:01Z","run":47,"stream":"stdout","line":"pg_dump: dumping database \"mydb\""}
{"ts":"2026-03-15T02:00:03Z","run":47,"stream":"stdout","line":"pg_dump: reading schemas"}
{"ts":"2026-03-15T02:00:05Z","run":47,"stream":"stderr","line":"pg_dump: connection refused"}
{"ts":"2026-03-15T02:00:05Z","run":47,"stream":"meta","event":"exit","code":1,"duration_ms":4200,"category":"connection_error"}
```

The final `meta` event summarizes the run. Agents can stream `kron logs --tail --json` and react to patterns in real time.

**Webhook payload spec**

When alerting ships, webhook payloads are structured for agent consumption:

```json
{
  "event": "job_failed",
  "job": "backup-db",
  "run_id": 47,
  "exit_code": 1,
  "error": "pg_dump: connection refused",
  "error_category": "connection_error",
  "consecutive_failures": 2,
  "success_rate_7d": 0.714,
  "trend": "degrading",
  "timestamp": "2026-03-15T02:00:05Z",
  "kron_version": "0.2.0"
}
```

**`kron annotate` — Jobs report back to kron**

Scripts can annotate their own runs with structured metadata:

```bash
# Inside a job script:
kron annotate $KRON_RUN_ID "migrated 1.2M rows in 4m"
kron annotate $KRON_RUN_ID --key rows_migrated --value 1200000
kron annotate $KRON_RUN_ID --tag slow  # tag for filtering
```

Annotations appear in `kron history --json` and `kron logs --json`, giving agents rich context about what a job actually did.

---

#### Tier 3 (v0.3+) — Advanced agent integration

**Unix socket API**

For agents that don't want to shell out to CLI:

```
~/.local/share/kron/kron.sock
```

JSON-RPC over Unix socket. Same operations as CLI, lower latency, no process spawn overhead. Enables:
- Real-time event subscription (job started/finished/failed)
- Bulk operations (create 50 jobs at once)
- Long-lived connections for monitoring dashboards

**`kron watch` — Event stream**

```bash
$ kron watch --json
{"event":"job_started","job":"backup-db","run":48,"ts":"2026-03-16T02:00:00Z"}
{"event":"job_output","job":"backup-db","run":48,"stream":"stdout","line":"dumping..."}
{"event":"job_finished","job":"backup-db","run":48,"result":"success","duration_ms":12400}
{"event":"job_started","job":"health-check","run":121,"ts":"2026-03-16T02:05:00Z"}
```

Agents subscribe to this stream instead of polling `kron status` in a loop.

---

### Agent Integration Patterns

**Pattern 1: Self-healing infrastructure**
```bash
# Agent monitors kron and fixes issues automatically
while true; do
  troubles=$(kron health --json | jq -r '.trouble_spots[]')
  for job in $troubles; do
    category=$(kron health "$job" --json | jq -r '.last_run.error_category')
    case $category in
      connection_error) systemctl restart postgresql ;;
      disk_full) /scripts/cleanup-old-backups.sh ;;
      *) kron annotate "$job" --tag needs-human ;;
    esac
  done
  sleep 300
done
```

**Pattern 2: AI agent creates and monitors its own jobs**
```python
# AI agent schedules a data pipeline and monitors it
import subprocess, json

# Create the job
subprocess.run(["kron", "add", "every 6 hours", "/scripts/sync.sh", "--name", "ai-sync"])

# Check health later
result = subprocess.run(["kron", "health", "ai-sync", "--json"], capture_output=True)
health = json.loads(result.stdout)

if not health["healthy"]:
    if health["last_run"]["error_category"] == "connection_error":
        # Agent decides to retry with different endpoint
        subprocess.run(["kron", "env", "ai-sync", "set", "API_ENDPOINT=backup.example.com"])
```

**Pattern 3: CI/CD integration**
```yaml
# GitHub Actions: verify scheduled jobs are healthy before deploy
- name: Check kron health
  run: |
    health=$(ssh prod "kron health --json")
    failing=$(echo "$health" | jq '.jobs_failing')
    if [ "$failing" -gt 0 ]; then
      echo "::error::$failing kron jobs are failing. Fix before deploying."
      exit 1
    fi
```

---

## Non-Goals

kron explicitly does **not** aim to be:

- **A distributed scheduler** — kron runs on a single machine. Multi-server coordination is out of scope. Use Dkron or Temporal for that.
- **A workflow orchestration engine** — kron runs individual jobs with simple dependency chains. DAGs, branching, fan-out/fan-in are out of scope. Use Airflow, Windmill, or Temporal.
- **A GUI-first application** — The CLI is the primary interface. The web dashboard is optional, read-only, and comes in Tier 3.
- **A cloud service** — No accounts, no SaaS, no telemetry, no phone-home. kron is a local binary.
- **A container orchestrator** — kron can run inside containers but does not manage them. Use Supercronic or Ofelia for container-native scheduling.
- **A replacement for systemd timers** — kron is for scheduled commands. If you need socket activation, service management, or cgroup integration, use systemd.
- **A process supervisor** — kron schedules and observes. It does not keep long-running daemons alive. Use systemd, supervisord, or s6 for that.

---

## Success Metrics

As an open-source project, success is measured by adoption and community engagement:

| Metric | 6 months | 12 months | 24 months |
|---|---|---|---|
| GitHub stars | 500 | 2,000 | 5,000 |
| Monthly crate downloads | 200 | 1,000 | 5,000 |
| Contributors | 5 | 15 | 30 |
| Open issues (healthy range) | 20-50 | 50-100 | 50-150 |
| HN/Reddit front page posts | 1 | 2 | 3+ |

**Qualitative signals:**
- Appearance in "awesome-rust" and "awesome-cli" lists
- Blog posts and tutorials written by users (not maintainers)
- Requests for features in Tier 3 (indicates users hitting the edges of Tier 1-2)
- Adoption by CI/CD pipelines and container images
- "I replaced cron with kron" testimonials
- AI agent frameworks and tools integrating kron as their default scheduler
- Mentions in AI/LLM tooling guides ("how to schedule tasks for your AI agent")

---

## Open Questions

1. **Daemon vs. crontab wrapper** — Should kron run its own scheduler daemon, or generate and manage system crontab entries? A daemon gives full control (overlap prevention, streaming) but adds operational complexity. Current leaning: daemon with `kron daemon start/stop` commands.

2. **Config file format** — TOML is the current choice (Rust ecosystem native). Should YAML be supported as an alternative for users coming from Kubernetes/Ansible? Current leaning: TOML only for v0.1, consider YAML in v0.3.

3. **Output storage limits** — How much stdout/stderr should be stored per run? Current proposal: 10MB default, configurable via `max_output` in job policy. What happens when the SQLite database grows large? Consider automatic rotation or a `kron gc` command.

4. **Secret storage UX** — Should the master passphrase be prompted on every daemon start, or cached in a session agent? OS keyring avoids this question, but not all environments have one (e.g., headless servers, Docker containers).

5. **Schedule DSL scope** — How far should the human-readable parser go? "every day at 2am" is clear, but what about "every 3rd weekday" or "last friday of the month"? Current leaning: support common patterns, fall back to cron syntax for edge cases.

6. **Log streaming protocol** — For `kron logs --tail`, should kron use Unix domain sockets, named pipes, or file watching? Needs to work across Linux and macOS.

7. **Plugin/hook system** — Should kron support pre/post-run hooks? This would enable custom integrations but adds complexity. Defer to v0.3+.

8. **Notification rate limiting** — If a job fails every minute, should kron send 1,440 alerts per day? What is the default deduplication/throttling behavior? Without rate limiting, kron becomes the alert spam tool users hate. Proposed: deduplicate identical failures, max 1 alert per job per hour by default.

9. **Missed schedule catch-up** — If the machine was powered off when a job was scheduled, should kron run it on boot? Options: always catch up, catch up only if missed within N minutes, never catch up (cron behavior), configurable per job. This directly impacts the "silent failure" thesis.

10. **Multi-user support** — Does kron manage jobs per-user (like crontab) or system-wide? If per-user, how does it handle jobs that need root? Affects file paths, SQLite location, process privileges, and security model.

11. **Log retention policy** — How long are run logs kept on disk? Forever? Last N runs? Last N days? Auto-pruned? Disk usage on a busy server with many jobs can grow fast. Must have a sensible default (proposed: 90 days, configurable via `kron gc`).

12. **Signal handling for child processes** — When kron receives SIGTERM (e.g., system shutdown), does it forward the signal to running children? Wait for them to finish? Kill immediately? Data corruption risk if a backup job is killed mid-write.

13. **What does `kron list` show for system cron jobs?** — Does kron have read-only visibility into the system crontab, or only its own managed jobs? Users expect "list active crons" to mean *all* crons, not just kron-managed ones.

---

## Appendix: Competitive Matrix

| Feature | kron (planned) | Cron | Cronitor | Healthchecks.io | Supercronic | Dkron | Windmill |
|---|---|---|---|---|---|---|---|
| **Run/schedule jobs** | Yes | Yes | No | No | Yes | Yes | Yes |
| **Human-readable schedule** | Yes | No | N/A | N/A | No | No | Yes |
| **Output capture** | Yes | No | No | No | stdout only | No | Yes |
| **Run history** | Yes | No | Yes | Partial | No | Partial | Yes |
| **Exit code tracking** | Yes | No | Yes | Yes | No | No | Yes |
| **Failure alerting** | Yes | MAILTO | Yes | Yes | No | Partial | Yes |
| **Dead-man switch** | Yes | No | Yes | Yes | No | No | No |
| **Retry with backoff** | Yes | No | No | No | No | No | Yes |
| **Overlap prevention** | Yes | No | No | N/A | No | Yes | Yes |
| **Environment management** | Yes | No | No | No | Partial | No | Yes |
| **Secrets management** | Yes | No | No | No | No | No | Yes |
| **Timezone per job** | Yes | No | N/A | N/A | Yes | Yes | Yes |
| **Crontab import/export** | Yes | N/A | No | No | No | No | No |
| **CLI-first** | Yes | Yes | No | No | Yes | Partial | No |
| **Single binary** | Yes | Yes | N/A | No | Yes | Yes | No |
| **Self-hosted** | Yes | Yes | No | Yes | Yes | Yes | Yes |
| **Free / OSS** | MIT | Yes | Freemium | BSD | MIT | LGPL | AGPL |
| **Web dashboard** | Planned | No | Yes | Yes | No | Yes | Yes |
| **Distributed** | No | No | No | No | No | Yes | No |
| **Agent-friendly API** | Yes | No | No | No | No | Partial | Partial |
| **Structured errors** | Yes | No | No | No | No | No | Partial |
| **Health check endpoint** | Yes | No | Yes | Yes | No | Yes | Yes |

---

*kron is open source under the MIT license. Contributions, bug reports, and feature requests are welcome.*
