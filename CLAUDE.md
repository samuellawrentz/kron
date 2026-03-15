# CLAUDE.md — kron

## What is kron?

A modern cron replacement written in Rust. CLI-first, single binary. Cron but it actually tells you what happened.

**Core thesis:** Cron's problem isn't syntax — it's the observability black hole. kron captures everything, alerts on anything unexpected, zero config to get started.

## Tech Stack

- **Language:** Rust
- **CLI:** clap
- **Storage:** SQLite (embedded) for run history
- **Config:** TOML job definitions
- **Async:** tokio
- **HTTP:** reqwest (notifications)

## Architecture Principles

1. Single binary, zero dependencies — `cargo install kron` and done
2. Zero-config observability — every run logged automatically
3. Cron-compatible — import/export crontab format
4. Single file per job (TOML)
5. CLI-first, optional web dashboard

## Key Commands

```
kron add "every day at 2am" ./backup.sh
kron list
kron status
kron history <job>
kron logs <job>
kron test <job>          # dry-run
kron run <job>           # force run
kron remove <job>
kron import              # from system crontab (interactive selection)
kron import --all        # import all crontab entries
kron export              # to crontab format (planned)
kron alert add-telegram  # configure alerts
kron daemon start        # run scheduler
kron daemon install      # install as system service
kron update              # self-update
```

## Project Structure

```
crates/
  kron-cli/              # CLI entry point (clap) + command handlers
  kron-core/             # Core logic: config, scheduler, runner, crontab parser, notifications
  kron-store/            # SQLite storage layer
```

## Development

```bash
cargo build
cargo test
cargo run -- list
```

## Research

See `docs/research/` for competitive landscape, pain points analysis, and product spec.
