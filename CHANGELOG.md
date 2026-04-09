# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
