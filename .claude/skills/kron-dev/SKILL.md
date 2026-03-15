---
name: kron-dev
description: kron project context and development conventions. Use when writing code for kron, adding features, fixing bugs, or making architectural decisions.
user-invocable: false
---

# kron Development Guide

kron is a modern cron replacement in Rust. CLI-first, single binary, zero config to get started.

## Architecture

Three-crate workspace with strict dependency direction: `kron-cli` → `kron-core` → `kron-store`.

- **kron-store**: SQLite storage (rusqlite bundled, WAL mode). Run history only — jobs table exists but is not the source of truth.
- **kron-core**: Config (TOML serde), async runner (tokio::process with timeout), scheduler daemon (cached cron parsing, overlap prevention, CancellationToken shutdown).
- **kron-cli**: clap derive API, 8 subcommands. anyhow for errors.

## Key Design Decisions

- **TOML is the single source of truth** for job definitions at `~/.config/kron/jobs/*.toml`. SQLite stores only run history.
- **Own daemon** — not wrapping system crontab. Full control over scheduling, output capture, timeout, overlap.
- **XDG directories** — config `~/.config/kron/`, data `~/.local/share/kron/`.
- `unsafe_code = "forbid"` at workspace level.

## Error Handling

- Library crates: `thiserror` with typed enums (`CoreError`, `StoreError`)
- Binary crate: `anyhow` with `.context()` for user-facing messages
- No `.unwrap()` in production code (clippy enforced)
- In tests: `#[allow(clippy::unwrap_used)]` on test modules

## Async Patterns

- `tokio::task::spawn_blocking` for ALL SQLite and filesystem I/O
- `std::sync::Mutex` (not tokio) — locks never held across `.await`
- `CancellationToken` + SIGINT/SIGTERM for graceful shutdown
- `tracing` spans with `Instrument` for async job execution

## Adding a New CLI Command

1. Create `crates/kron-cli/src/commands/<name>.rs`
2. Add variant to `Command` enum in `commands/mod.rs`
3. Add match arm in `commands::run()`
4. For sync commands: `pub fn execute(...) -> Result<()>`
5. For async commands: `pub async fn execute(...) -> Result<()>`
6. Use `config::load_all_jobs()` for job data, `Store::open()` only for run history

## Adding a New Core Feature

1. Add types/functions to appropriate module in `kron-core`
2. If it needs storage: add methods to `Store` in `kron-store`, add migration
3. Add `#[cfg(test)]` unit tests in the same module
4. Run `cargo test && cargo clippy --all-targets` before committing

## Job Definition Format

```toml
[job]
name = "backup"
command = "pg_dump mydb > /backups/mydb.sql"
schedule = "0 2 * * *"
working_dir = "/app"
enabled = true
timeout = "30m"
```

## Pre-commit Hook

Runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` before every commit.
