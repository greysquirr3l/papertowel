# CLI Command Reference

This section provides a detailed reference for the `papertowel` command-line interface.

## Scrubber Commands

### `papertowel scan <path>`

Reports AI fingerprints in the specified path. This is a read-only operation.

**Options:**

- `--format <json|text>`: The output format. (Default: `text`)
- `--severity <low|medium|high>`: Filter findings by minimum severity.
- `--fail-on <low|medium|high>`: Exit with code 1 if any finding at or above the threshold is found.
- `--ci`: Auto-detected from the `CI` env var. Implies `--fail-on medium` and `--format github-actions` unless overridden.

Files listed in `.papertowelignore` (or `[exclude].paths` in `.papertowel.toml`) are skipped. Files containing a `// papertowel:ignore-file` directive are also skipped. Individual lines can be suppressed with `// papertowel:ignore-next-line`.

**Example:**

```bash
papertowel scan . --severity medium
```

### `papertowel scrub <path>`

Detects and automatically fixes AI fingerprints in the specified path.

**Options:**

- `--dry-run`: Preview changes without applying them to the filesystem.
- `--detectors <list>`: A comma-separated list of detectors to run (e.g., `lexical,comments`).

**Example:**

```bash
papertowel scrub . --dry-run
```

### `papertowel clean <path>`

A convenience command that runs the full pipeline: `scan` followed by `scrub`.

**Options:**

- `--dry-run`: Preview changes.

---

## Wringer Commands

### `papertowel wring init`

Sets up the git worktree for the public branch.

**Options:**

- `--branch <name>`: The name of the public branch to create/use. (Default: `public`)

**Example:**

```bash
papertowel wring init --branch public
```

### `papertowel wring queue`

Analyzes the difference between your development branch and the public branch to build a replay plan.

**Options:**

- `--from <branch>`: The source branch containing your development work.

**Example:**

```bash
papertowel wring queue --from dev
```

### `papertowel wring drip`

Replays commits from the queue into the public worktree on a human schedule.

**Options:**

- `--daemon`: Run in the background and apply commits as their target time arrives.
- `--profile <name>`: The persona profile to use for scheduling and message humanization.

**Example:**

```bash
papertowel wring drip --daemon --profile night-owl
```

### `papertowel wring status`

Shows the current state of the queue and the synchronization position.

---

## Profile Commands

### `papertowel profile create <name>`

Launches an interactive builder to create a new persona profile.

### `papertowel profile list`

Lists all available persona profiles (built-in and custom).

### `papertowel profile show <name>`

Dumps the TOML configuration of a specific persona profile.
