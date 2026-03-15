# AGENTS.md

## Project Overview

kron is a modern cron replacement in Rust — CLI-first, single binary. It solves cron's observability black hole by capturing every run's output, exit code, and duration to a local SQLite database.

## Architecture

Three-crate workspace:

- `kron-store` — SQLite storage layer (rusqlite, bundled). Handles job records and run history.
- `kron-core` — Core engine: TOML config parsing, async command runner with timeout, scheduler daemon with overlap prevention.
- `kron-cli` — clap-based CLI binary with 8 subcommands: `add`, `list`, `status`, `history`, `logs`, `run`, `remove`, `daemon`.

Dependency direction: `kron-cli` → `kron-core` → `kron-store`

## Key Design Decisions

- **TOML is the single source of truth** for job definitions (`~/.config/kron/jobs/*.toml`). SQLite stores only run history.
- **Own daemon** (not wrapping system crontab) — full control over scheduling, output capture, timeout, overlap prevention.
- **XDG directories** — config in `~/.config/kron/`, data in `~/.local/share/kron/`.
- **No unsafe code** — `unsafe_code = "forbid"` at workspace level.

## Development Commands

```bash
cargo build                 # Build all crates
cargo test                  # Run all tests
cargo clippy --all-targets  # Lint (pedantic, zero warnings)
cargo fmt --check           # Format check
cargo run -- <cmd>          # Run CLI in dev mode
```

## Testing

- Pre-commit hook runs fmt + clippy + tests
- `kron-store`: 7 tests (CRUD, duplicates, run history, update_run errors)
- `kron-core`: 12 tests (config serialization, command execution, timeout, scheduler shutdown, duration parsing)
- `kron-cli`: 0 integration tests (planned with `assert_cmd`)

## Error Handling

- Library crates (`kron-store`, `kron-core`): `thiserror` with typed error enums
- Binary crate (`kron-cli`): `anyhow` with `.context()` for user-facing messages
- No `.unwrap()` in production code (clippy enforced)

## Async Patterns

- `tokio` runtime with `spawn_blocking` for all SQLite and filesystem I/O
- `std::sync::Mutex` (not tokio) since locks are never held across `.await`
- `CancellationToken` for graceful daemon shutdown (SIGINT + SIGTERM)
- `tracing` with `Instrument` spans for async job execution

## Current Limitations / TODO

- No human-readable schedule syntax ("every day at 2am") — planned via `english-to-cron` crate
- No notifications (webhook/Slack/Telegram) — planned
- No crontab import/export — planned
- No CLI integration tests — planned with `assert_cmd`
- No web dashboard — planned as optional feature
