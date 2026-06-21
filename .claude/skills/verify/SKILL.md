---
name: verify
description: Pre-commit verification gate — runs check, test, clippy, and format checks across the Rust workspace. Use before committing or when asked to verify changes.
disable-model-invocation: true
---

## Steps

Run these commands in order, stopping on first failure:

```bash
# 1. Fast compilation check
cargo check --workspace

# 2. All tests
cargo test --release --workspace

# 3. Strict linting
cargo clippy --workspace -- -D warnings

# 4. Format check
cargo fmt --all -- --check
```

If any command fails, report the failure with the exact output and do NOT proceed to the next step.
If all pass, confirm: "All checks passed — ready to commit."

## When the gate is RED: distinguish new vs pre-existing

"The gate is red, must be my fault" is the wrong default. CI warnings, lint exceptions, and even compile errors can predate the current change. Before treating a failure as yours to fix:

1. Note the exact failure (file:line, warning code, message).
2. Stash the working tree: `git stash push -u -m "verify-isolate"`.
3. Re-run the same failing command on the pristine tree (typically `git checkout origin/main -- <path>` or just `cargo check` from a clean checkout of `origin/main`).
4. If the failure reproduces on `origin/main`, it is **pre-existing** — report it as such and continue with the other gates; do not let it block.
5. Only treat the failure as "must fix in this change" when it disappears on the clean tree.

This is especially important for `clippy` warnings that surface after a refactor — a method the refactor stopped calling may have been used elsewhere on a different platform backend, in which case the warning is the only signal that the contract changed.
