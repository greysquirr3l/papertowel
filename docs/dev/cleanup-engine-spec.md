# Cleanup Engine Specification

Status: Draft
Owner: papertowel maintainers
Date: 2026-04-27

## 1) Problem Statement

The repository already uses a strong cleanup process in prompts, but the process is not yet encoded as a first-class product capability in papertowel CLI/MCP. This creates drift risk across sessions and reviewers.

Goal: implement a conservative, evidence-driven cleanup engine that operationalizes the seven cleanup tracks with reproducible outputs and policy-aware validation.

## 2) Goals

1. Preserve behavior and public API by default.
2. Produce deterministic, reviewable findings with explicit confidence and evidence.
3. Apply only high-confidence, low-risk edits unless policy allows broader changes.
4. Persist deferred items so work is cumulative across runs.
5. Integrate with CI in advisory or blocking mode.

## 3) Non-goals

1. No aggressive auto-refactors by default.
2. No deletion based only on static tool output.
3. No mandatory architecture rewrites.
4. No hidden policy decisions; all gates must be configurable and observable.

## 4) Cleanup Tracks (Product Model)

Each track maps to finding kinds and evidence requirements.

1. deduplication
2. type_consolidation
3. dead_code
4. circular_dependencies
5. type_strengthening
6. error_handling
7. deprecated_and_ai_artifacts

Each finding must include:

- track
- id
- file path and optional symbol span
- description
- risk level
- confidence score and class
- evidence bundle
- suggested action (defer | review | apply)

## 5) CLI Surface

### 5.1 assess command

papertowel cleanup assess <path>

Flags:

- --tracks <csv> (default: all)
- --format text|json (default: text)
- --mixed (reuse existing mixed-content collection rules where relevant)
- --policy <path> (optional policy override)
- --out <path> (write machine-readable report)
- --baseline <path> (compare against previous report)
- --ci (CI mode: no interactive prompts)

Behavior:

- Read-only analysis.
- Generates per-track findings, confidence classes, and required evidence gaps.
- Generates deferred queue for medium/low confidence items.

### 5.2 apply command

papertowel cleanup apply <report.json>

Flags:

- --max-risk low|medium (default: low)
- --min-confidence high (default: high)
- --allow-tracks <csv>
- --dry-run
- --ci

Behavior:

- Only applies candidates marked apply and allowed by policy gates.
- Refuses destructive actions missing required evidence.
- Emits post-apply validation summary.

### 5.3 status command

papertowel cleanup status [--format text|json]

Behavior:

- Shows deferred backlog, evidence gaps, and trend since last successful validation.

## 6) MCP Surface

New tools:

1. papertowel_cleanup_assess

- input: path, tracks, format, ci, baseline
- output: same schema as CLI assess

2. papertowel_cleanup_apply

- input: report path or payload, policy overrides, dry_run
- output: applied changes, skipped candidates, validation block summary

3. papertowel_cleanup_status

- input: none or scope path
- output: deferred queue + evidence-needed summary

Tool safety annotations:

- assess/status: read-only
- apply: destructiveHint true, idempotentHint false

## 7) Confidence and Risk Model

Confidence score range: 0.0 to 1.0

Confidence classes:

- high: >= 0.85 and all mandatory evidence present
- medium: >= 0.60 and < 0.85, or evidence partially present
- low: < 0.60 or critical evidence missing

Risk levels:

- low: no behavior/API impact expected, localized edit
- medium: local behavior or API shape possibly affected
- high: cross-boundary behavior, compatibility path, or broad semantic rewrite

Default policy:

- apply only low-risk + high-confidence
- medium/low confidence items are deferred with explicit evidence requirements

## 8) Evidence Policy (Critical)

Mandatory evidence templates by track:

1. dead_code

- static reference scan result
- configuration/string-hook scan
- convention/entrypoint scan
- cross-target check (lib/bin/test/bench/example)

2. deprecated_and_ai_artifacts

- deprecation source (docs/changelog/version target)
- compatibility matrix impact check
- user-facing path verification (CLI/API)

3. circular_dependencies

- cycle graph evidence before/after
- neutral extraction candidate list

If mandatory evidence is missing:

- candidate action must be defer or review
- candidate cannot be marked apply

## 9) Report Schema (JSON)

Top-level shape:

{
  "version": "1",
  "generated_at": "2026-04-27T00:00:00Z",
  "path": ".",
  "tracks": ["dead_code", "error_handling"],
  "summary": {
    "finding_count": 0,
    "apply_count": 0,
    "review_count": 0,
    "defer_count": 0
  },
  "findings": [],
  "deferred": [],
  "validation_plan": {
    "commands": [
      "cargo build --workspace",
      "cargo test --workspace",
      "cargo clippy --workspace --all-targets -- -D warnings"
    ]
  }
}

Finding shape:

{
  "id": "cleanup.dead_code.001",
  "track": "dead_code",
  "severity": "medium",
  "risk": "low",
  "confidence": {
    "score": 0.91,
    "class": "high",
    "reasons": ["unused in all targets", "no config hook found"]
  },
  "location": {
    "file": "src/foo.rs",
    "line": 42,
    "symbol": "foo::bar"
  },
  "description": "Unused helper function",
  "evidence": {
    "required": ["refs_scan", "entrypoint_scan"],
    "present": ["refs_scan", "entrypoint_scan"],
    "missing": []
  },
  "suggested_action": "apply"
}

## 10) Config Additions (.papertowel.toml)

[cleanup]
enabled = true
persist_reports = true
report_dir = ".papertowel/cleanup"

[cleanup.policy]
min_confidence_to_apply = "high"
max_risk_to_apply = "low"
require_manual_verification_for_delete = true

[cleanup.validation]
commands = [
  "cargo build --workspace",
  "cargo test --workspace",
  "cargo clippy --workspace --all-targets -- -D warnings"
]
fail_on_validation_error = true

[cleanup.tracks.dead_code]
enabled = true
require_cross_target_check = true

[cleanup.tracks.deprecated_and_ai_artifacts]
enabled = true
require_deprecation_evidence = true

## 11) CI Integration

Recommended CI jobs:

1. cleanup-assess (advisory)

- run cleanup assess --format json --out .papertowel/cleanup/latest.json --ci
- upload artifact for review

2. cleanup-policy-gate (optional blocking)

- fail if report contains any candidate marked apply but blocked by missing mandatory evidence
- fail if validation commands fail after cleanup apply in CI mode

3. cleanup-trend

- compare current deferred counts with baseline
- post summary comment in PR

Default recommendation: advisory first, then selective blocking once signal quality is stable.

## 12) Implementation Plan (Incremental)

Phase A: Read-only assess

1. Add cleanup domain types and JSON report writer.
2. Implement assess command with track routing and confidence classification.
3. Persist deferred queue in .papertowel/cleanup/deferred.json.

Phase B: Guarded apply

1. Implement apply command with strict policy gates.
2. Add dry-run and CI mode behavior.
3. Add post-apply validation execution and reporting.

Phase C: MCP parity

1. Add cleanup assess/apply/status tools.
2. Ensure schema parity with CLI JSON.
3. Add protocol tests for tool annotations and response shape.

Phase D: CI templates/docs

1. Add example workflow snippets to docs.
2. Add troubleshooting section for false positives and evidence gaps.

## 13) Testing Strategy

1. Unit tests

- confidence classification
- evidence gate behavior
- policy overrides

2. Integration tests

- assess output snapshots
- apply dry-run vs apply behavior
- deferred queue persistence and status output

3. Regression tests

- no deletions applied when mandatory evidence missing
- no apply outside min-confidence/max-risk policy

4. Validation contract tests

- command execution captures pass/fail states correctly

## 14) Acceptance Criteria

1. cleanup assess returns deterministic report for same input and config.
2. cleanup apply never applies medium/low confidence items under default policy.
3. dead code and deprecated path removals require mandatory evidence.
4. deferred queue persists and is visible via cleanup status.
5. CI advisory mode works without blocking by default.
6. Existing commands and outputs remain backward compatible.

## 15) Open Design Choices

1. Should apply operate directly on report payload, report path, or both?
2. Should confidence scoring weights be fixed or user-configurable?
3. Should cleanup status aggregate per branch or per repo root only?

## 16) Suggested Next Task Cut

1. T46: cleanup domain/report schema + assess skeleton
2. T47: track routers (read-only) + confidence classifier
3. T48: deferred queue persistence + cleanup status
4. T49: guarded cleanup apply + validation runner
5. T50: MCP cleanup tool parity
6. T51: CI docs and workflow examples
