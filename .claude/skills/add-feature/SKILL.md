---
name: add-feature
description: Add a new feature to kron following project conventions. Use when implementing new functionality, commands, or capabilities.
argument-hint: <feature-description>
disable-model-invocation: true
allowed-tools: Read, Write, Edit, Bash(cargo:*), Grep, Glob
---

# Add Feature to kron

Implement the feature described by: $ARGUMENTS

## Before writing code

1. Read the relevant existing code to understand current patterns
2. Check `docs/research/product-spec.md` for the feature's design spec
3. Check `docs/research/best-practices.md` for implementation guidance
4. Identify which crate(s) need changes (`kron-store`, `kron-core`, `kron-cli`)

## Implementation checklist

- [ ] Add types/structs to the appropriate crate
- [ ] Follow existing error handling: `thiserror` in libs, `anyhow` in CLI
- [ ] Use `tokio::task::spawn_blocking` for any SQLite or filesystem I/O in async code
- [ ] Add `#[cfg(test)]` unit tests for new functionality
- [ ] If adding a CLI command: add to `Command` enum, dispatcher, and create handler module
- [ ] If modifying config: update `JobDefinition` struct with `#[serde(default)]`
- [ ] No `.unwrap()` in production code

## After writing code

1. Run `cargo fmt`
2. Run `cargo clippy --all-targets` — fix all warnings
3. Run `cargo test` — all tests must pass
4. Test manually with `cargo run -- <command>` if applicable
5. Create an atomic git commit describing the change
