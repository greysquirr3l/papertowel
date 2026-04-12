# CLAUDE.md — papertowel

## What is this?

**papertowel** is a Rust CLI tool that cleans up the telltale signs of AI-generated code. It has two major subsystems:

1. **The Scrubber** — static analysis and transformation of source code to detect and remove LLM fingerprints (slop vocabulary, over-documentation, cookie-cutter READMEs, suspiciously uniform code structure, metadata boilerplate).

2. **The Wringer** — git history humanization via worktrees. Instead of rewriting history after the fact, it maintains a public branch in a git worktree and drip-feeds commits on a realistic human schedule driven by configurable persona profiles.

The name is a pun: cleaning up the slop.

## Architecture

**Pattern:** Collapsed DDD+CQRS in a single crate.

This is a mono-crate published to crates.io as `papertowel` with both `[lib]` and `[[bin]]` targets. No workspace, no sub-crates unless an MCP server is added later. Module tree does all the organizational work.

```
src/
├── main.rs              # Binary entry, tracing init, clap dispatch
├── lib.rs               # Public lib API surface
├── cli/                 # Clap subcommand definitions and handlers
├── scrubber/            # Code fingerprint detection and transformation
│   ├── lexical.rs       # Slop vocabulary (aho-corasick multi-pattern)
│   ├── comments.rs      # Over-documentation scoring and thinning
│   ├── structure.rs     # Uniform code pattern detection
│   ├── readme.rs        # Cookie-cutter README detection
│   └── metadata.rs      # CONTRIBUTING/COC/SECURITY boilerplate detection
├── wringer/             # Git history humanization
│   ├── worktree.rs      # Git worktree lifecycle (git2, no CLI shelling)
│   ├── queue.rs         # Commit analysis, squash/split planning
│   ├── drip.rs          # Background daemon, scheduled commit replay
│   ├── messages.rs      # Commit message humanization with entropy
│   └── archaeology.rs   # Synthetic iteration artifacts (TODOs, renames, reverts)
├── profile/
│   └── persona.rs       # Human persona profiles (schedule, style, entropy)
├── detection/
│   └── finding.rs       # Scored findings with category, severity, location
└── domain/
    ├── commands.rs       # CQRS command types
    ├── queries.rs        # CQRS query types
    └── errors.rs         # thiserror hierarchy
```

## Key Design Decisions

### Worktree drip-feed over history rewriting

The wringer does NOT use `git filter-branch`, `git rebase`, or `BFG`. Instead:

- Dev work happens on a private branch with Wiggum committing at machine speed
- `wring init` creates a worktree on a `public` branch
- `wring queue` analyzes pending commits and builds a replay plan
- `wring drip` cherry-picks into the worktree at persona-driven intervals
- Public branch has real commits with real timestamps — no rewriting forensics

### Single crate, module tree

Everything ships as one `cargo install papertowel`. Feature areas are modules, not crates. If an MCP server is needed later, it becomes a thin second crate in a workspace that depends on `papertowel` as a lib.

### Pluggable detectors

Each scrubber module implements a common trait returning `Vec<Finding>`. Transform modules consume findings and apply fixes. New detectors/transforms are added as modules without touching the pipeline.

## Conventions

### Error handling

- `thiserror` for all library error types in `domain/errors.rs`
- `anyhow` only in `main.rs` and CLI handlers
- Never `unwrap()` in library code; `expect()` only with invariant justification

### Logging

- `tracing` for structured logging everywhere
- `tracing-subscriber` with `EnvFilter` in `main.rs`
- Use `#[instrument]` on public functions
- Span hierarchy: command → subsystem → operation

### Git operations

- All git ops via `git2` crate — never shell out to `git` CLI
- Worktree state persisted in `.papertowel/wringer.toml`
- Queue plan persisted in `.papertowel/queue.json`

### Configuration

- Repo-level config: `.papertowel.toml`
- Path exclusions: `.papertowelignore` (gitignore syntax)
- Persona profiles: `~/.config/papertowel/profiles/*.toml`
- Two built-in profiles: `night-owl` and `nine-to-five`

### Testing

- Unit tests in each module with `#[cfg(test)]`
- Integration tests in `tests/` using `tempfile` for scratch repos
- `assert_cmd` for CLI smoke tests
- Test repos with known AI fingerprints as fixtures

### CI

- GitHub Actions: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo deny check`, `cargo test`

## CLI Surface

```bash
papertowel scan <path>           # Report AI fingerprints (read-only)
    --format json|text
    --severity <low|medium|high>

papertowel scrub <path>          # Fix code fingerprints
    --dry-run
    --detectors <comma-separated>

papertowel wring init            # Set up worktree for public branch
    --branch <name>

papertowel wring queue           # Analyze dev commits, build replay plan
    --from <branch>

papertowel wring drip            # Replay commits on human schedule
    --daemon
    --profile <name>

papertowel wring status          # Show queue state and sync position

papertowel clean <path>          # Full pipeline: scan + scrub
    --dry-run

papertowel profile create <name> # Interactive persona builder
papertowel profile list          # Show available personas
papertowel profile show <name>   # Dump persona config
```

## Persona Profile Format

```toml
[persona]
name = "night-owl"
timezone = "America/Detroit"

[persona.schedule]
active_hours = ["10:00-14:00", "21:00-03:00"]
peak_productivity = "22:00-01:00"
avg_commits_per_session = 8
session_variance = 0.4

[persona.messages]
style = "mixed"              # conventional | lazy | mixed
wip_frequency = 0.15
profanity_frequency = 0.05
typo_rate = 0.02
emoji_rate = 0.01

[persona.archaeology]
todo_inject_rate = 0.1
dead_code_rate = 0.05
rename_chains = true
```

## Slop Vocabulary Reference (Partial)

### Single words (high signal when clustered)

robust, comprehensive, streamlined, utilize, facilitate, leverage, seamless, delve, modular, extensible, intuitive, boilerplate, idiomatic, straightforward, trivial, ergonomic, performant, granular, opinionated

### Phrases

"it's worth noting", "as mentioned above", "for the sake of", "in order to", "this ensures that", "helper function to", "this module provides", "we can see that", "under the hood", "out of the box", "at the end of the day"

### Comment patterns

"// This function ...", "// Helper to ...", "// Utility for ...", "/// This struct represents", "/// A comprehensive ...", "/// Provides a streamlined ..."

## Immutable source files

An unknown VS Code extension is replacing slop vocabulary words (e.g. "robust" → "sturdy", "utilize" → "use") and stripping doc comments/attributes in source files on save. The following files contain intentional slop vocabulary in string literals and test fixtures and have been locked with `chflags uchg` to prevent corruption:

- `src/learning/learner.rs`
- `src/scrubber/comments.rs`
- `src/scrubber/structure.rs`
- `src/scrubber/promotion.rs`
- `src/scrubber/lexical.rs`
- `src/cli/hook.rs`
- `src/cli/scrub.rs`
- `src/cli/wring.rs`
- `src/wringer/lock.rs`
- `src/domain/errors.rs`

To edit these files, unlock first: `chflags nouchg <file>`, edit, then re-lock: `chflags uchg <file>`.

## What NOT to touch

- Never modify `.git/` internals directly — always go through `git2`
- Never rewrite history on the dev/source branch
- Never auto-commit to the user's current working branch without explicit command
- The `.papertowel/` directory is papertowel's state — don't pollute the repo root

## Context for AI Agents

This tool exists because AI-generated code has predictable stylistic fingerprints, and some communities/reviewers treat AI origin as a disqualifying factor regardless of code quality. papertowel is the solution: make code that is functionally identical but stylistically indistinguishable from human-written code.

The irony of using AI to build a tool that hides AI involvement is not lost on us. Embrace it.
