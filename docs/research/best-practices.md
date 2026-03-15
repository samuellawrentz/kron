# Rust CLI Best Practices for kron

Synthesized from research on ripgrep, bat, fd, just, watchexec, and the broader cron replacement landscape.

---

## Project Structure

### Workspace Layout

Mature Rust CLIs (ripgrep, watchexec) use Cargo workspaces to separate concerns. For kron:

```
kron/
  Cargo.toml              # workspace root
  crates/
    kron-cli/             # Binary crate — clap, command dispatch, main.rs
      src/
        main.rs           # Thin entry (~30 lines), calls lib
        lib.rs            # CLI logic
        commands/         # One module per subcommand
    kron-core/            # Library crate — scheduler, runner, parser, notify
      src/
        lib.rs
        scheduler/        # Job scheduling engine
        runner/           # Job execution, output capture
        parser/           # Human-readable + cron expression parsing
        notify/           # Notification dispatch
        config/           # TOML job definition structs
        error.rs          # thiserror-based error types
    kron-store/           # Library crate — SQLite storage
      src/
        lib.rs
        migrations/
        models.rs
        error.rs
```

**Key principle:** `main.rs` is a thin entry point (<30 lines). All logic lives in `lib.rs` or library crates. This enables integration testing without subprocess spawning. Every examined tool (5/5) follows this pattern.

### Cargo.toml Conventions

```toml
[workspace]
members = ["crates/*"]
resolver = "2"

[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
pedantic = "warn"
unwrap_used = "warn"
expect_used = "warn"

[profile.release]
lto = true
codegen-units = 1
strip = true
```

---

## Dependency Choices

| Area | Crate | Why |
|------|-------|-----|
| CLI parsing | `clap` (derive) | Universal standard, derive API is unanimous over builder |
| Error (library) | `thiserror` | Typed, matchable error enums for library crates |
| Error (binary) | `anyhow` | Ergonomic error propagation with `.context()` |
| Async runtime | `tokio` (multi-thread) | Industry standard for async Rust |
| SQLite | `rusqlite` + `bundled` | Compiles SQLite into binary — zero system deps |
| Migrations | `rusqlite_migration` | Lightweight, uses `user_version` pragma |
| Config | `toml` + `serde` | Standard for Rust ecosystem |
| Config paths | `dirs` | XDG-compliant, cross-platform |
| Logging | `tracing` + `tracing-subscriber` | Structured logging with async span correlation |
| Schedule parsing | `croner` | Best-in-class: 5-field + extended syntax + timezone |
| English input | `english-to-cron` | "every day at 2am" → cron expression |
| HTTP | `reqwest` | Notifications (webhook, Slack, Telegram) |
| CLI testing | `assert_cmd` + `predicates` | Standard for Rust CLI integration tests |
| Temp files | `assert_fs` | Test fixtures |

---

## Error Handling

### Library Crates (kron-core, kron-store)

Use `thiserror` for typed, matchable errors:

```rust
#[derive(Debug, thiserror::Error)]
pub enum KronError {
    #[error("job not found: {name}")]
    JobNotFound { name: String },

    #[error("invalid schedule: {0}")]
    ScheduleParse(String),

    #[error("store error: {0}")]
    Store(#[from] StoreError),

    #[error("notification failed: {0}")]
    Notify(String),
}
```

### Binary Crate (kron-cli)

Use `anyhow` for ergonomic propagation:

```rust
fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    run(cli).context("kron failed")?;
    Ok(())
}
```

**Rule:** Never `.unwrap()` in production paths. Use `?` with context.

---

## clap Patterns

### Top-level Struct (not enum)

```rust
#[derive(Parser)]
#[command(name = "kron", version, about = "Cron that tells you what happened")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Enable verbose output (-v, -vv, -vvv)
    #[arg(short, long, action = ArgAction::Count, global = true)]
    pub verbose: u8,

    /// Disable color output
    #[arg(long, global = true, env = "NO_COLOR")]
    pub no_color: bool,
}
```

Rain's Rust CLI Recommendations specifically warns: always use a struct at the top level, never an enum — "it always comes back to bite me."

### Subcommands

```rust
#[derive(Subcommand)]
pub enum Command {
    /// Add a new job
    Add {
        /// Schedule in human-readable or cron format
        schedule: String,
        /// Command to execute
        command: Vec<String>,
    },
    /// List all jobs
    List,
    /// Show job run history
    History {
        /// Job name
        job: String,
        /// Number of recent runs
        #[arg(short, long, default_value = "10")]
        count: usize,
    },
    // ...
}
```

**Key patterns:**
- `///` doc comments become `--help` text automatically
- `#[arg(env = "KRON_VAR")]` for env var integration
- `#[arg(global = true)]` for flags available on all subcommands
- `#[derive(Args)]` + `#[command(flatten)]` for reusable argument groups

---

## Async / tokio

### Blocking Work

All blocking operations (SQLite, subprocess execution, file I/O) must use `spawn_blocking`:

```rust
let result = tokio::task::spawn_blocking(move || {
    conn.execute("INSERT INTO runs ...", params![...])
}).await??;
```

**Rule:** Async tasks must not block for more than 10-100 microseconds between `.await` points.

### Graceful Shutdown

Use the official Tokio pattern (no extra deps):

```rust
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

let token = CancellationToken::new();
let tracker = TaskTracker::new();

// On SIGINT/SIGTERM:
token.cancel();
tracker.close();
tracker.wait().await;
```

### Mutex Rule

Use `std::sync::Mutex` (not `tokio::sync::Mutex`) unless the lock guard is held across an `.await` point.

---

## SQLite Storage

### Setup

```toml
[dependencies]
rusqlite = { version = "0.32", features = ["bundled"] }
```

`bundled` compiles SQLite into the binary — critical for kron's zero-dependency goal.

**Warning:** Do not use rusqlite + sqlx together — both link `libsqlite3-sys`, causing semver hazards.

### WAL Mode

Enable immediately on connection:

```rust
conn.pragma_update(None, "journal_mode", "WAL")?;
```

### Migrations

Use `rusqlite_migration` — lightweight, uses `user_version` pragma, SQL inline in Rust:

```rust
use rusqlite_migration::{Migrations, M};

let migrations = Migrations::new(vec![
    M::up("CREATE TABLE jobs (...)"),
    M::up("CREATE TABLE runs (...)"),
]);
migrations.to_latest(&mut conn)?;
```

---

## Configuration

### Precedence (highest wins)

1. CLI flags
2. `KRON_*` environment variables
3. `~/.config/kron/config.toml` (user global)
4. Built-in defaults

### Job Definitions

Single TOML file per job at `~/.config/kron/jobs/<name>.toml`:

```toml
[job]
name = "backup"
schedule = "0 2 * * *"
command = "/home/user/backup.sh"
timeout = "1h"

[notify]
on_failure = true
on_success = false
webhook = "https://hooks.slack.com/..."
```

Use `dirs` crate for XDG-compliant path resolution:

```rust
let config_dir = dirs::config_dir()
    .unwrap_or_else(|| PathBuf::from("."))
    .join("kron");
```

---

## Logging / Observability

### Use tracing, not log

For a concurrent job scheduler, `tracing` spans are essential — traditional log lines interleave incorrectly across async task boundaries.

```rust
use tracing::{info, info_span, Instrument};

let span = info_span!("job_run", job = %name, run_id = %id);
async {
    info!("starting job");
    // all events inside carry job + run_id automatically
}.instrument(span).await;
```

### Verbosity Mapping

| Flag | Level |
|------|-------|
| (none) | `warn` |
| `-v` | `info` |
| `-vv` | `debug` |
| `-vvv` | `trace` |

### Output Discipline

- **stdout:** User-facing data (job list, history, logs) — must be pipeable
- **stderr:** Errors, progress, diagnostics
- Respect `NO_COLOR` env var and `--color=auto/always/never`

---

## Testing Strategy

### Three-Layer Approach

All examined tools (5/5) use this pattern:

```
Layer 1: #[test] in src/ modules        — pure unit tests on functions
Layer 2: tests/*.rs against lib API      — no subprocess spawning
Layer 3: tests/cli_tests.rs              — full CLI integration with assert_cmd
```

### CLI Integration Tests

```rust
use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn test_add_job() {
    Command::cargo_bin("kron")
        .unwrap()
        .args(["add", "every day at 2am", "./backup.sh"])
        .assert()
        .success()
        .stdout(predicate::str::contains("added"));
}

#[test]
fn test_unknown_job() {
    Command::cargo_bin("kron")
        .unwrap()
        .args(["history", "nonexistent"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}
```

Use `assert_fs` for temporary config/job file fixtures in tests.

---

## Schedule Parsing

### Dual Input Strategy

1. **Input:** Accept both cron expressions (`0 2 * * *`) and English (`every day at 2am`)
2. **Storage:** Store canonical cron expressions internally
3. **Display:** Show human-readable descriptions via `croner`

### Crontab Edge Cases (Critical)

These are real production-incident-level issues:

| Edge Case | Detail |
|-----------|--------|
| **DOW encoding** | 0 and 7 both mean Sunday in vixie-cron. Choose 0=Sunday, document it, test both. Cloudflare hit this bug post-launch. |
| **DOM+DOW union** | vixie-cron runs a job when EITHER day-of-month OR day-of-week matches (not AND). Counterintuitive and widely misunderstood. |
| **`%` expansion** | In crontab, `%` becomes newline; first line is stdin. Must handle during import. |
| **`@` shortcuts** | `@daily`, `@hourly`, `@reboot`, etc. Map to standard expressions. |
| **`MAILTO=""`** | Suppresses email in crontab. Map to notification config during import. |

---

## Notification Design

### Keep It Lean

- Use `reqwest` directly for webhook/Slack/Telegram — avoid heavy bot frameworks
- Feature-gate notification backends to keep binary small
- `pling` crate is a lightweight alternative for multi-provider notifications

### Notification Payload

```rust
pub struct JobNotification {
    pub job_name: String,
    pub status: RunStatus,  // Success, Failure, Timeout
    pub exit_code: Option<i32>,
    pub duration: Duration,
    pub stdout_tail: String, // last N lines
    pub stderr_tail: String,
}
```

---

## Competitive Gaps kron Can Fill

| Gap | Status Quo | kron Opportunity |
|-----|-----------|-----------------|
| Output capture | Cron discards silently (no MTA) | Every run logged to SQLite automatically |
| Run history | None in standard cron | `kron history` with exit codes, duration, output |
| Failure alerts | Email only (broken by default) | Slack, Telegram, webhook — zero-config |
| Overlap prevention | Manual flock hacks | Built-in job locking |
| Human syntax | `0 2 * * 1-5` | `every weekday at 2am` |
| Dry run | Not possible | `kron test <job>` |
| Import/export | N/A | `kron import` / `kron export` for migration |

**Market note:** jobber (Go) was the closest competitor with run history + notifications, but is explicitly unmaintained — the maintainer invited a takeover in the README. Direct opportunity for kron.

---

## Sources

- Rain's Rust CLI Recommendations (sunshowers.io)
- Rust CLI Book (rust-cli.github.io)
- Tokio official docs — Graceful Shutdown, Tracing
- Alice Ryhl — "Async: What is blocking?"
- Cloudflare Engineering Blog — Saffron cron parser post-mortem
- ripgrep, bat, fd, just, watchexec GitHub repositories
- rusqlite_migration, croner, english-to-cron docs
- POSIX crontab(5) specification
