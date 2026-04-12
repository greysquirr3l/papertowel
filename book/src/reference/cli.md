# CLI Command Reference

This section provides a detailed reference for the `papertowel` command-line interface.

## Scrubber Commands

### `papertowel scan <path>`

Reports AI fingerprints in the specified path. This is a read-only operation.

**Options:**

- `--format <json|text|sarif|github-actions>`: The output format. `sarif` emits [SARIF 2.1.0](https://sarifweb.azurewebsites.net/) for integration with VS Code SARIF Viewer and GitHub Code Scanning. (Default: `text`)
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

### `papertowel grade <path>`

Assigns a letter grade (A+ to F) based on the project's overall AI fingerprint level. Lower slop scores yield better grades.

**Options:**

- `--format <json|text>`: Output format. JSON includes detailed category breakdowns. (Default: `text`)
- `--min-grade <grade>`: Exit with code 1 if the grade is below this threshold. Useful for CI gating. Valid values: `A+`, `A`, `A-`, `B+`, `B`, `B-`, `C+`, `C`, `C-`, `D+`, `D`, `D-`, `F`.
- `--ci`: Shorthand for `--min-grade C`. Fails the build if the project scores C- or below.

**Grade calculation:**

Grades are based on a weighted "slop score" across categories:

| Category | Weight |
|----------|--------|
| Lexical (slop vocabulary) | 20% |
| Architecture | 20% |
| Comments | 15% |
| Structure | 15% |
| Metadata | 10% |
| Testing | 10% |
| History | 10% |
| Workflow | 5% |

**Example:**

```bash
papertowel grade .
papertowel grade . --min-grade B --ci
papertowel grade . --format json
```

---

## Recipe Commands

See [Recipes](../scrubber/recipes.md) for the full recipe TOML format and how to write custom recipes.

### `papertowel recipe list`

Lists all available recipes from all sources (built-in, user-global, and repo-local).

**Options:**

- `--source <builtin|user|repo>`: Filter results to a specific source.

**Example:**

```bash
papertowel recipe list
papertowel recipe list --source builtin
```

### `papertowel recipe show <name>`

Displays details of a specific recipe including its patterns and scoring config.

**Options:**

- `--raw`: For file-backed recipes, output the raw TOML instead of the parsed summary.

**Example:**

```bash
papertowel recipe show slop-vocabulary
```

### `papertowel recipe validate <path>`

Validates the syntax and structure of a recipe TOML file without scanning any code.

**Example:**

```bash
papertowel recipe validate .papertowel/recipes/my-recipe.toml
```

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
- `--profile <name>`: The persona profile to use when scheduling the replay plan.

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

### `papertowel wring unlock-stale`

Removes a stale drip lock file left behind by a previously crashed drip session. Safe to run while another drip is active — it only removes locks whose owning process is no longer running.

---

## Profile Commands

### `papertowel profile create <name>`

Launches an interactive builder to create a new persona profile.

### `papertowel profile list`

Lists all available persona profiles (built-in and custom).

### `papertowel profile show <name>`

Dumps the TOML configuration of a specific persona profile.

---

## Hook Commands

### `papertowel hook install`

Installs a pre-commit hook that scans staged files for AI fingerprints. The hook copies staged file contents to a temp directory (so it scans what's being committed, not working-tree state) and runs `papertowel scan --fail-on medium`. If findings at medium severity or above are found, the commit is blocked.

**Options:**

- `--force`: Overwrite an existing pre-commit hook, even if it wasn't installed by papertowel.

The hook is idempotent — running `install` when the hook is already present is a no-op.

**Example:**

```bash
papertowel hook install
papertowel hook install --force  # overwrite a foreign hook
```

### `papertowel hook uninstall`

Removes the papertowel pre-commit hook. Refuses to remove hooks that weren't installed by papertowel.

**Example:**

```bash
papertowel hook uninstall
```

### `papertowel hook status`

Shows whether a papertowel pre-commit hook is installed, and whether the installed hook was created by papertowel or is a foreign hook.

---

## Learn Commands

### `papertowel learn repo <path>`

Analyses the git history and source files in `<path>` to produce a **Style Baseline** — a statistical model of your coding habits. The baseline is saved to `.papertowel/baseline.json` in the repo root and can be passed to `wring drip` to ensure humanized history mirrors your real style.

**Arguments:**

- `<path>`: Path to the repository root to analyse.

**Example:**

```bash
papertowel learn repo .
```

### `papertowel learn show <path>`

Displays the Style Baseline previously generated for the repository at `<path>`.

**Example:**

```bash
papertowel learn show .
```
