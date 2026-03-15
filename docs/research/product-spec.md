# kron — Product Specification
**Date:** 2026-03-15
**Tagline:** Cron, but it actually tells you what happened.

---

## Core Insight

The cron problem is not the schedule syntax. It's the **observability black hole**: jobs run in a stripped-down environment, produce no captured output by default, send no alerts on failure, and have no mechanism to confirm they ran at all.

The highest-leverage thing kron can do: **capture everything, alert on anything unexpected, zero configuration to get started.**

---

## Design Principles

1. **CLI-first, single binary** — `curl | install` and done. No runtime, no dependencies.
2. **Zero-config observability** — every run is logged automatically. No redirect boilerplate.
3. **Cron-compatible** — import existing crontabs, export back to cron format.
4. **Single file per job** — TOML/YAML. Version-control native.
5. **Don't replace the OS scheduler** — wrap it, add observability.

---

## Feature Tiers

### Tier 1 — Must Have (solves top 3 pain points)

1. **Automatic output capture** — stdout + stderr logged per run. Queryable via CLI.
2. **Built-in alerting** — dead-man switch + failure alerts. Telegram, Slack, webhook. No external service.
3. **Environment snapshot** — capture PATH, env vars, shell at job creation. Replay faithfully at runtime.

### Tier 2 — Strong Differentiators

4. **Human-readable schedule DSL** — `"every day at 2am"`, `"weekdays every 15min"`, `"30min after last success"`. Compiled to cron expressions for portability.
5. **Single-file job definitions** — TOML with inline schedule, env, retry policy, timeout.
6. **Dry-run mode** — `kron test my-job` — run in production-equivalent context without waiting for schedule.
7. **Overlap prevention** — `skip_if_running: true` (default on).
8. **Retry with backoff** — `retry = 3`, `backoff = "exponential"`.

### Tier 3 — Killer Features

9. **Job history** — `kron history my-job` — last N runs, durations, exit codes, output.
10. **Timezone per job** — `timezone = "Asia/Kolkata"`.
11. **Dependency chains** — `after = "backup-db"`.
12. **Web dashboard** — lightweight, optional, read-only view of all jobs + history.

---

## CLI Design

```bash
# Job management
kron add "every day at 2am" ./backup.sh          # quick add
kron add --file backup.toml                       # from definition file
kron list                                         # list all jobs
kron edit backup                                  # edit job definition
kron remove backup                                # remove a job
kron enable/disable backup                        # toggle without removing

# Execution
kron test backup                                  # dry-run now
kron run backup                                   # force run now

# Observability
kron status                                       # all jobs + last run status
kron history backup                               # run history
kron logs backup                                  # output from last run
kron logs backup --run 3                          # output from specific run
kron logs backup --tail                           # follow live output

# Import/Export
kron import                                       # import from system crontab
kron export                                       # export to crontab format

# Alerts
kron alert add telegram --token xxx --chat-id yyy
kron alert add webhook --url https://...
kron alert test                                   # send test notification
```

---

## Job Definition Format (TOML)

```toml
[job]
name = "backup-db"
command = "pg_dump mydb > /backups/mydb.sql"
schedule = "every day at 2am"
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
on_silence = "1h"           # dead-man switch: alert if no run in 1h

[job.depends]
after = ["backup-files"]    # run after backup-files succeeds
```

---

## What NOT to Do

- Don't require a database for single-machine use (SQLite at most)
- Don't require 2 files per job (systemd's mistake)
- Don't make the primary interface a GUI — CLI first
- Don't replace the OS scheduler — wrap it
- Don't require accounts, cloud services, or SaaS for basic functionality

---

## Tech Stack

- **Language:** Rust
- **CLI framework:** clap
- **Storage:** SQLite (embedded, zero-config) for run history
- **Scheduler:** tokio-cron-scheduler or custom, wrapping system crontab
- **Notifications:** reqwest for webhooks/Telegram/Slack
- **Config:** TOML (native Rust ecosystem support)

---

## Competitive Position

```
                    Simple ←──────────────→ Complex
                    │                            │
         CLI-first  │   kron ★                   │  Dkron
                    │                            │
                    │                            │
                    │   Supercronic               │  Windmill
      Runner-only  │   Ofelia                    │  Temporal
                    │                            │
                    │                            │
   Monitor-only    │   Healthchecks.io           │  Cronitor
                    │                            │
```

kron lives in the "simple + CLI-first + full-featured" quadrant that nobody occupies.
