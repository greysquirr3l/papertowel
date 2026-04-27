# Dead Code Verification Workflow

This project runs dead-code signal checks in CI, but findings are advisory until manually verified.
Do not delete code solely because a tool flagged it.

## CI checks

- `cargo machete` — reports dependencies that appear unused by source imports.
- `cargo +nightly udeps --workspace --all-targets` — reports unused direct dependencies per target.

## Run locally

```bash
cargo install cargo-machete --locked --force
cargo +nightly install cargo-udeps --locked --force

cargo machete
cargo +nightly udeps --workspace --all-targets
```

## Manual verification checklist before deletion

1. Runtime wiring: confirm no dynamic dispatch, plugin registration, reflection-style lookups, or CLI subcommand wiring rely on the symbol.
2. Configuration references: check config keys, env vars, and string-based hooks that may reference the code path indirectly.
3. Generated code paths: check build scripts, proc-macro output, or generated modules that may use the dependency.
4. Convention-based entrypoints: check file naming and framework conventions (tests, docs examples, benches, binaries).
5. Cross-crate usage: verify the item is unused in all workspace crates and targets (lib/bin/test/bench/examples).

If any checklist item is uncertain, keep the code and open a follow-up issue with evidence.
