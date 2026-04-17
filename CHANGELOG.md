# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.7] — 2026-04-16

### Fixed

- **Windows CI: normalize path separators for git2 index** — `Path::strip_prefix()` returns backslash-separated paths on Windows; `index.add_path` (libgit2) requires POSIX forward-slash paths on all platforms. Added `#[cfg(windows)]` normalization in `stage_and_commit`.
- **Windows CI: gate `std::os::unix` APIs behind `#[cfg(unix)]`** — `hook.rs` and `learner.rs` used `PermissionsExt` unconditionally, causing compile errors on Windows. Entire permission-mode blocks and related tests now gated.
- **Windows CI: handle `ERROR_LOCK_VIOLATION` (OS error 33) in `fs2` lock** — Windows returns OS error 33 instead of `WouldBlock` when re-locking an already-locked file or reading bytes from a range locked by another handle. Added `is_already_locked()` helper that matches both kinds; all lock probes and tests updated.
- **Clippy: collapse nested `if let` / `if` blocks** — `collapsible_if` lint in `lock.rs` introduced by the OS error 33 fix; collapsed to `if let Err(e) = … && !is_already_locked(&e)` form.

### Fixed

- **CLI handlers no longer call `std::process::exit(1)` directly** — CI gate failures in `scan` and `grade`, and recipe validation errors, were bypassing Rust's `Result` chain. Replaced with `anyhow::bail!()` so errors surface through `dispatch()` → `run()` → `main()` and carry proper context.
- **`clean` command now propagates CI mode to post-scrub scan** — `Command::Clean` was hardcoding `ci: false, fail_on: None` for the scan phase after scrub, so `--fail-on` and the `CI` environment variable were silently ignored. Now calls `scan::effective_ci_settings()` and threads the derived values through.
- **`InitArgs.branch` uses `String` instead of `Option<String>`** — clap always fills `default_value`, so the `Option` wrapper was misleading and required an unnecessary `unwrap_or_else`. Changed to `String`.
- **Structured tracing fields in scrub comment-transform warning** — replaced ad-hoc string interpolation with `error = %e` structured field form.
- **MCP handler return types are `Result<()>`** — `handle_tools` and `handle_version` previously returned `()`, forcing the dispatch site to manually wrap in `Ok(())`. Made consistent with all other CLI handlers.
- **Preserve TOML error chain in `recipe validate`** — recipe parse errors now use `anyhow::Error::from(e).context(...)` instead of `bail!("...: {e}")`, so the full `toml::de::Error` source chain is visible in output.

## [0.3.5] — 2026-04-13

### Added

- **`papertowel mcp` subcommand group** — the main CLI now exposes MCP integration directly:
  - `papertowel mcp serve` — runs the `papertowel-mcp` stdio server from the same binary directory
  - `papertowel mcp tools` — lists available MCP tool names
  - `papertowel mcp version` — prints MCP protocol and server build version

### Fixed

- **Comment detector false positive on API documentation** — `///` and `//!` doc-comment lines are now excluded from inline-comment density checks and tutorial-phrase scoring, so well-documented libraries no longer trigger `comments.over_documented`. `TUTORIAL_PHRASES` also drops the overly broad `"to"` and `"for"` entries.
- **`grade` table column misalignment** — `Grade::Display` now calls `f.pad(s)` instead of `write!(f, "{s}")`, so format specifiers like `{grade:>5}` correctly right-align grades within the box-drawing table.
- **MCP scan parity with CLI detector coverage** — `papertowel-mcp` now runs repository-level detectors (`commit_pattern`, `architecture`, `workflow`, `promotion`, `metadata`, `maintenance`, `name_credibility`) when scanning a git repo, includes the security detector path, and aligns prompt-detector extension coverage with the CLI (`zig`, `cpp`, `cc`, `cxx`, `hpp`, `hxx`).

### Changed

- **Release automation wiring** — auto-tag now dispatches `release.yml` on the created version tag ref. `.github/workflows/auto-tag.yml` now requests `actions: write` and invokes `gh workflow run release.yml --ref <tag>`, while `.github/workflows/release.yml` now supports `workflow_dispatch` in addition to tag-push triggers.

## [0.3.4] — 2026-04-12

### Changed

- Added `rustfmt.toml` (`edition = "2024"`, `max_width = 100`) — project-wide formatter config.
- Build metadata now falls back to `crates.io` (or `source`) when git SHA is unavailable, instead of showing `unknown` in `--version` output.
- Sorry for the version alignment bumps in recent patch releases; this release keeps `papertowel` and `papertowel-mcp` aligned at `0.3.4`.

## [0.3.3] — 2026-04-12

### Fixed

- **`#[error(...)]` and other Rust attributes stripped by scrub** ([#4](https://github.com/greysquirr3l/papertowel/pull/4)) — `is_comment_line` was treating any line starting with `#` as a comment. Rust inner (`#![...]`) and outer (`#[...]`) attributes are now excluded, so `#[derive(...)]`, `#[error(...)]`, `#[from]`, and any other attribute are preserved intact by both `transform_text` and `analyze_comments`.

## [0.3.2] — 2026-04-12

### Added

- **`papertowel_grade` MCP tool** — grade a file or directory A+–F for overall AI fingerprint presence. Returns overall grade, score, file count, finding count, and optional per-category breakdown (`explain: true`).
- `papertowel::cli::scan` is now a public module so external crates can reuse `collect_findings_for_root` and `ScanCollection`.
- Protocol regression test extended to cover `papertowel_grade` annotations.

## [0.3.1] — 2026-04-12

### Changed

- Version housekeeping: bumped crate versions to `0.3.1` for crates.io publish cleanup.

## [0.3.0] — 2026-04-12

### Added

- **MCP protocol regression coverage** in `papertowel-mcp` to lock down initialization and tool-surface fields:
  - `initialize` response assertions for `protocolVersion`, `capabilities.tools.listChanged`, and `serverInfo.description`
  - `tools/list` assertions for `papertowel_scan` and `papertowel_scrub` annotations (`readOnlyHint`, `destructiveHint`, `idempotentHint`, `openWorldHint`)
- README and book MCP setup docs now include a copy-paste server block using stdio command mode:
  - `"type": "stdio"`
  - `"command": "papertowel-mcp"`
  - `"env": { "RUST_LOG": "info" }`
- **Explainability output** for scan and grade workflows:
  - `--explain` support surfaces per-category contribution details
  - report output includes explainability attribution and confidence contributions
- **Detector calibration workflow** via `papertowel calibrate`:
  - computes project-specific recommendations from observed findings and optional learning baseline
  - optional recommendation file output for tuning follow-up
- **Mixed-content analysis mode** for hybrid human+AI files:
  - conservative segment-based aggregation/deduplication to reduce over-counting clustered findings
  - shared collection path reused across scan/grade/calibrate/eval commands
- **Evaluation harness** via `papertowel eval`:
  - fixture-based confusion matrix output with precision, recall, and accuracy
  - supports mixed-content mode during evaluation for like-for-like detector comparisons

### Changed

- `papertowel-mcp` initialization metadata aligned with MCP `2025-11-25` guidance:
  - explicit `capabilities.tools.listChanged`
  - `serverInfo.description` included in initialize result
- `tools/list` now emits explicit tool safety annotations for both MCP tools.
- CLI top-level help now shows descriptions for all subcommands (`scan`, `calibrate`, `eval`, `scrub`, `grade`, `wring`, `clean`, `learn`, `profile`, `recipe`, `hook`) for better discoverability.
- CLI surface expanded with `calibrate` and `eval` subcommands; top-level help now documents these workflows alongside existing commands.

### Documentation

- Added MCP setup examples in both README and the book using stdio command mode with logging env:
  - `"type": "stdio"`
  - `"command": "papertowel-mcp"`
  - `"env": { "RUST_LOG": "info" }`
- Updated the book MCP setup to use command-based execution (`papertowel-mcp`) instead of a hardcoded absolute path example.
- Clarified CLI discoverability by documenting/reflecting descriptive subcommand help output.

### Fixed

- Tool input validation failures in MCP tool calls are now returned as tool execution errors (`isError: true`) instead of protocol-level parameter errors, improving host/model self-correction behavior.

## [0.2.0] — 2026-04-12

### Added

- **Security vulnerability detector**: New detector category identifying insecure patterns frequently generated by AI:
  - **SEC001–SEC015**: 15 rules covering OWASP Top 10 categories (SQL/shell injection, weak cryptography, TLS bypass, JWT flaws, XSS, path traversal, credential logging, debug mode, weak RNG, unsafe deserialization, SSRF, hardcoded IV/nonce, hardcoded secrets, unsafe eval/exec, auth TODOs)
  - Cross-language support: Rust, Go, TypeScript/TSX, JavaScript/JSX, Python, C#
  - Regex-based detection with per-rule confidence scoring
  - Performance optimization: regexes compiled once at startup and cached in `LazyLock`
- Security category added to `FindingCategory`; enabled by default, disable via `[detectors] security = false` in `.papertowel.toml`
- **`papertowel grade` command**: Get a letter grade (A+ to F) for your project's AI fingerprint level. Lower slop = better grade. Supports `--min-grade` for CI gating and `--format json` for automation. Inspired by [vibescore](https://github.com/chand1012/vibescore).
- **Architecture detector**: New detector category analyzing code organization patterns:
  - **ARCH001**: Flat module structure (no meaningful subdirectories)
  - **ARCH002**: Missing architectural layers (no domain/, ports/, etc.)
  - **ARCH003**: God files (>800 lines mixing responsibilities)
  - **ARCH004**: Low trait ratio (<2% abstractions)
  - **ARCH005**: Anemic domain models (structs with no impl blocks)
- Architecture category added to `FindingCategory` and weighted at 20% in grade calculation.

### Changed

- Upgraded `rand` from 0.8 to 0.9 to resolve RUSTSEC-2026-0097 (potential unsoundness in `ThreadRng` with custom loggers)
- Upgraded `dirs` from 5 to 6 (routine semver bump; no API changes used by this project)

## [0.1.5] — 2026-04-10

### Added

- **MCP recipe integration**: `papertowel_scan` and `papertowel_scrub` MCP tools now run the recipe-based detector alongside structural detectors. The recipe matcher is loaded from the scanned path's project root (best-effort; falls back to structural-only on failure).

### Fixed

- MCP `call_scan` refactored into `scan_file_into` helper to satisfy `clippy::too_many_lines`; `map_or_else` used in `load_recipe_matcher` to fix `clippy::map_unwrap_or`.

## [0.1.4] — 2026-04-10

### Added

- **Recipe system**: pluggable TOML-based detection and scrub recipes. Built-in recipes cover slop vocabulary, phrase patterns, and comment patterns. Custom recipes can be placed in `.papertowel/recipes/` (repo-local) or `~/.config/papertowel/recipes/` (user-global).
- **`papertowel recipe` commands**: `recipe list` (with `--source` filter), `recipe show <name>` (with `--raw` flag), `recipe validate <path>`.
- **Recipe scrubber**: word-level replacement and regex transforms driven by recipe TOML, wired into both `scan` and `scrub`.
- Fixture-driven integration tests for the `scan` command (`tests/integration.rs`).

### Changed

- `scrub --detectors`: accepts `recipe` (and the legacy alias `lexical`) to select the recipe-based detector.
- `scan`: skips files larger than 2 MiB before attempting `read_to_string` to avoid I/O waste on binaries.
- `.papertowelignore`: recipe and scrubber source files excluded from self-scan.

### Fixed

- Glob matching in recipe matcher now falls back to the bare filename component so patterns like `README.md` match regardless of path form.
- Divide-by-zero guard added for `cluster_range_lines = 0` in cluster scoring.
- Whole-word boundary check now treats `_` as a word character, preventing false positives inside snake_case identifiers.
- `hot_buckets` changed from `Vec` to `HashSet` for O(1) membership test.
- Regex patterns with `applies_to`/`excludes` constraints are skipped in the text-only transform path (constraint cannot be enforced without a file path).
- Non-UTF-8 scrub errors downgraded from `warn` to `debug` level.
- `#[ignore]` replaces the unreliable `CI` env-var guard on wring queue tests.
- Integration tests now strip `CI` from the binary environment to prevent auto-`--fail-on medium` from breaking assertions.

## [0.1.3] — 2026-04-09

### Fixed

- `papertowel-mcp` dependency changed to `path + version` form for workspace compatibility.

## [0.1.2] — 2026-04-09

### Fixed

- CI cache busted after corrupt macOS artifacts caused stale rustdoc failures.

## [0.1.1] — 2026-04-09

### Fixed

- Wring queue integration tests marked `#[ignore]` to prevent flaky failures on shallow CI checkouts.

## [0.1.0] — 2026-04-09

### Added

- **Scrubber**: lexical slop detector (aho-corasick multi-pattern), comment density detector, structure uniformity detector, README/metadata boilerplate detector, promotion pattern detector, maintenance credibility detector, name credibility detector, idiom mismatch detector, prompt/test/workflow detectors.
- **Scrubber transforms**: lexical vocabulary replacement, comment thinning, README rewriting.
- **Wringer**: git worktree lifecycle (`wring init`), commit queue and replay planner (`wring queue`), drip-feed daemon (`wring drip`), commit message humanizer, archaeology injection (TODOs, dead code, rename chains).
- **Wringer utilities**: `wring status`, `wring unlock-stale` for lock management.
- **Persona profiles**: `night-owl` and `nine-to-five` built-in profiles, `profile create`/`list`/`show` commands.
- **Learning mode**: `learn repo` analyzes a codebase to build a style baseline, `learn show` displays it.
- **CI integration**: `--ci` flag auto-detects CI environments, GitHub Actions output format, `--fail-on` severity gating.
- **Configuration**: `.papertowel.toml` repo config, `.papertowelignore` path exclusions (gitignore syntax), inline `papertowel:ignore-file` and `papertowel:ignore-next-line` directives, project root discovery, global `~/.config/papertowel/config.toml` support.
- **Output formats**: text, JSON, and SARIF 2.1.0 for integration with VS Code SARIF Viewer, GitHub Code Scanning, and other static analysis tooling.
- **Pre-commit hook**: `papertowel hook install/uninstall/status` — scans staged files and blocks commits with findings at medium severity or above.
- **MCP server**: `papertowel-mcp` crate exposing `papertowel_scan` and `papertowel_scrub` tools (read-only).
- **Security**: gitleaks pre-commit hook integration, safe path handling, input validation.
- Multi-language support: Rust, Go, TypeScript, Python, Zig, C++.
- Git SHA embedded in `--version` output via `build.rs`.
