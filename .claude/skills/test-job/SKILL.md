---
name: test-job
description: Test a kron job end-to-end through the full CLI lifecycle (add, run, history, logs, remove).
argument-hint: [job-name]
disable-model-invocation: true
allowed-tools: Bash(cargo:*)
---

# Test Job E2E

Run a full end-to-end test of kron's CLI for job "$ARGUMENTS".

If no job name is provided, use "test-e2e".

## Steps

1. **Add** a test job:
   ```bash
   cargo run -- add --name $ARGUMENTS "* * * * *" echo "hello from $ARGUMENTS"
   ```

2. **List** jobs and verify it appears:
   ```bash
   cargo run -- list
   ```

3. **Force-run** the job:
   ```bash
   cargo run -- run $ARGUMENTS
   ```

4. **Check status**:
   ```bash
   cargo run -- status
   ```

5. **Check history**:
   ```bash
   cargo run -- history $ARGUMENTS
   ```

6. **Check logs** and verify output contains "hello from $ARGUMENTS":
   ```bash
   cargo run -- logs $ARGUMENTS
   ```

7. **Remove** the job:
   ```bash
   cargo run -- remove $ARGUMENTS
   ```

8. **Confirm removal** — list should not contain the job:
   ```bash
   cargo run -- list
   ```

Report pass/fail for each step. If any step fails, stop and report the error.
