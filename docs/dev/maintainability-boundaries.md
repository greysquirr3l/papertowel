# Maintainability Boundaries

This document defines safe module boundaries for the largest files so future refactors do not change user-facing behavior.

## Goals

- Keep CLI behavior, output formats, and exit codes unchanged.
- Reduce review and regression risk by splitting by responsibility.
- Make security-sensitive logic easier to audit.

## Priority 1: `papertowel-mcp/src/main.rs`

Current issue: transport, protocol, tool schemas, tool execution, and tests all live in one file.

Target split:

- `papertowel-mcp/src/protocol.rs`
  - JSON-RPC/MCP message types
  - error codes
  - protocol constants
- `papertowel-mcp/src/transport.rs`
  - `read_message`
  - `write_response`
  - request dispatch entrypoint
- `papertowel-mcp/src/tools.rs`
  - `handle_tools_list`
  - `handle_tools_call`
  - scan/scrub/grade adapters
- `papertowel-mcp/src/path_guard.rs`
  - `validate_mcp_path`

Acceptance checks:

- existing protocol surface test remains green
- no output/schema changes for `tools/list` and `tools/call`
- no behavior changes in path validation rejection cases

## Priority 2: `src/cli/report.rs`

Current issue: formatting, conversion, and summary logic are mixed.

Target split:

- `src/cli/report/summary.rs`
- `src/cli/report/text.rs`
- `src/cli/report/json.rs`
- `src/cli/report/sarif.rs`
- `src/cli/report/github_actions.rs`

Acceptance checks:

- all current report tests pass unchanged
- no changes to emitted fields for JSON/SARIF/GHA reports

## Priority 3: `src/scrubber/structure.rs`

Current issue: language-specific parsing and scoring logic are tightly coupled.

Target split:

- `src/scrubber/structure/metrics.rs`
- `src/scrubber/structure/parsers/`
- `src/scrubber/structure/detector.rs`

Acceptance checks:

- detector severity outcomes remain stable for existing fixtures
- cross-language behavior remains unchanged

## Safety Rules

- Move code without changing semantics first.
- Keep old tests while moving; add boundary tests before deleting old helpers.
- For each split, run:
  - `cargo check --workspace --all-targets`
  - `cargo test --workspace`
  - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
