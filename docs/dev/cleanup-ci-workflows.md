# Cleanup CI Rollout Guide

This guide documents an advisory-first rollout for the cleanup engine, with optional policy-gated blocking after signal quality is stable.

## Rollout Phases

1. Advisory only: run `cleanup assess`, publish JSON artifacts, review deferred/evidence gaps.
2. Optional blocking: enable `cleanup apply --dry-run --ci` on selected branches or paths.
3. Trend reporting: compare deferred/evidence-gap counts across runs and surface drift in CI summary.

## Recommended Jobs

The snippet below is designed to fit the existing Rust CI shape in this repository.

```yaml
name: Cleanup CI

on:
  pull_request:
  workflow_dispatch:

jobs:
  cleanup-assess:
    name: Cleanup Assess (advisory)
    runs-on: ubuntu-latest
    continue-on-error: true
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable

      - name: Run cleanup assess
        run: |
          set -euo pipefail
          mkdir -p .papertowel/cleanup
          cargo run -- cleanup assess . \
            --format json \
            --out .papertowel/cleanup/latest.json \
            --ci

      - name: Upload cleanup assess artifact
        uses: actions/upload-artifact@v4
        with:
          name: cleanup-assess-report
          path: .papertowel/cleanup/latest.json

  cleanup-policy-gate:
    name: Cleanup Policy Gate (optional blocking)
    runs-on: ubuntu-latest
    needs: [cleanup-assess]
    if: ${{ github.ref == 'refs/heads/main' || github.event_name == 'workflow_dispatch' }}
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable

      - name: Download cleanup assess artifact
        uses: actions/download-artifact@v4
        with:
          name: cleanup-assess-report
          path: .papertowel/cleanup

      - name: Run cleanup policy gate (dry-run apply)
        run: |
          set -euo pipefail
          cargo run -- cleanup apply .papertowel/cleanup/latest.json \
            --format json \
            --dry-run \
            --ci

  cleanup-trend:
    name: Cleanup Trend (advisory)
    runs-on: ubuntu-latest
    needs: [cleanup-assess]
    continue-on-error: true
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable

      - name: Run cleanup status and append to job summary
        run: |
          set -euo pipefail
          report=$(cargo run -- cleanup status . --format json)
          echo "## Cleanup Trend" >> "$GITHUB_STEP_SUMMARY"
          echo '```json' >> "$GITHUB_STEP_SUMMARY"
          echo "$report" >> "$GITHUB_STEP_SUMMARY"
          echo '```' >> "$GITHUB_STEP_SUMMARY"
```

## Local Reproduction Commands

Run exactly what CI runs before changing policy behavior:

```bash
cargo run -- cleanup assess . --format json --out .papertowel/cleanup/latest.json --ci
cargo run -- cleanup apply .papertowel/cleanup/latest.json --format json --dry-run --ci
cargo run -- cleanup status . --format json
```

Recommended full verification before enabling blocking mode:

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Troubleshooting

### Evidence gaps block apply candidates

Symptoms:

- `cleanup apply` reports many blocked findings.
- Reasons include `missing_mandatory_evidence` or `not_marked_apply`.

Actions:

1. Start with advisory-only mode until evidence quality improves.
2. Restrict apply scope with `--allow-tracks` to low-risk tracks first.
3. Keep `--dry-run` in CI until trend data is stable for several runs.

### False positives in cleanup assess

Symptoms:

- Findings are consistently deferred on known-safe code.

Actions:

1. Review track/evidence reasoning in the assess report JSON.
2. Re-run assess on a narrow path to isolate noisy patterns.
3. Prefer documenting and deferring over forcing broader apply thresholds.

### Policy gate too strict for early rollout

Actions:

1. Gate only selected branches first.
2. Use `--allow-tracks` to pilot one track at a time.
3. Keep `cleanup-policy-gate` disabled or advisory until confidence stabilizes.

## Notes

- `cleanup-assess` and `cleanup-trend` are best-effort advisory jobs.
- `cleanup-policy-gate` should be enabled as blocking only after teams are comfortable with deferred/evidence-gap behavior.
- Keep artifacts (`latest.json`) for auditability when introducing stricter policy gates.
