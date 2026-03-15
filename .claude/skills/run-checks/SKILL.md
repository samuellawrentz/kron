---
name: run-checks
description: Run the full CI check suite for kron (format, lint, test).
disable-model-invocation: true
allowed-tools: Bash(cargo:*)
---

# Run Checks

Run the full check suite and report results.

```bash
cargo fmt --check
```

```bash
cargo clippy --all-targets -- -D warnings
```

```bash
cargo test
```

Report a summary table:

| Check | Status |
|-------|--------|
| fmt   | pass/fail |
| clippy | pass/fail (N warnings) |
| test  | pass/fail (N passed, N failed) |

If any check fails, show the relevant error output and suggest a fix.
