# Recipes

Recipes are TOML files that define detection patterns and replacement transforms. They are the recommended way to extend `papertowel` without modifying source code. All built-in detectors ship as recipes; you can add your own to tailor the analysis to a specific codebase or house style.

## How Recipes Work

When `papertowel scan` or `papertowel scrub` runs, the recipe engine loads every available recipe and runs the enabled patterns against each scanned file. Findings from recipes appear in the same report as findings from structural detectors.

The recipe engine:

1. Loads built-in recipes (embedded in the binary).
2. Loads user-global recipes from `~/.config/papertowel/recipes/*.toml`.
3. Loads repo-local recipes from `.papertowel/recipes/*.toml`.
4. For each loaded recipe, runs enabled word, phrase, regex, and contextual pattern groups.
5. Applies cluster-scoring: if `cluster_threshold` words appear within `cluster_range_lines`, severity is boosted.
6. Applies `applies_to` / `excludes` glob gating so patterns can target specific file types.

Files larger than 2 MiB are skipped by the recipe scanner to avoid I/O waste on binary files.

## Recipe TOML Format

```toml
[recipe]
name        = "my-recipe"
version     = "1.0.0"
description = "Detects domain-specific slop vocabulary"
author      = "your-name"
category    = "Lexical"          # Lexical | Comment | Structural | Readme | Metadata | Custom
default_severity = "Low"         # Low | Medium | High

[scoring]
cluster_threshold      = 4       # how many hits trigger a severity boost
cluster_range_lines    = 15      # the rolling window (in lines) for clustering
cluster_severity_boost = "High"  # severity assigned to clustered findings
base_confidence        = 0.6     # confidence score injected into each finding (0.0–1.0)

[patterns.words]
enabled        = true
case_sensitive = false
whole_word     = true            # treats _ as word char; won't match inside snake_case
severity       = "Low"
items = [
    { word = "leverage",    replacement = "use"    },
    { word = "utilize",     replacement = "use"    },
    { word = "seamless",    replacement = "smooth" },
    # omit replacement to flag without auto-fixing:
    { word = "delve" },
]

[patterns.phrases]
enabled  = true
severity = "Medium"
items = [
    { phrase = "it's worth noting",  replacement = "" },
    { phrase = "in order to",        replacement = "to" },
    { phrase = "as mentioned above", replacement = "" },
]

[patterns.regex]
enabled  = true
severity = "Medium"
items = [
    # Flag function-level comment blocks that start with "This function ..."
    { pattern = r"//\s*This function \w+", replacement = "" },
    # Limit to Rust source files only
    { pattern = r"//\s*Helper to \w+",     replacement = "", applies_to = ["*.rs"] },
]
```

### `applies_to` and `excludes`

Every item in any pattern group can carry `applies_to` and `excludes` fields, each accepting a list of glob patterns matched against the file path:

```toml
{ word = "robust", replacement = "sturdy", applies_to = ["*.md", "*.txt"] }
{ phrase = "out of the box", excludes = ["tests/**"] }
```

Patterns with both fields: `applies_to` must match **and** `excludes` must not match for the pattern to run.

## Built-in Recipes

| Name | Category | What it targets |
| :--- | :--- | :--- |
| `slop-vocabulary` | Lexical | Word-level overused adjectives, verbs, and transitions |
| `phrase-patterns` | Lexical | Multi-word phrases that read as LLM filler |
| `comment-patterns` | Comment | Doc-comment openers that follow LLM boilerplate templates |

List them with:

```bash
papertowel recipe list
```

Inspect a specific recipe:

```bash
papertowel recipe show slop-vocabulary
```

## Adding Custom Recipes

### Repo-local

Place a TOML file at `.papertowel/recipes/my-recipe.toml`. It will be loaded automatically whenever `papertowel` is run from inside that repository. Commit it alongside your code so the team shares the same detection rules.

### User-global

Place a TOML file at `~/.config/papertowel/recipes/my-recipe.toml`. It applies to every repository on your machine.

## Validating a Recipe

Before adding a recipe to your workflow, validate its syntax:

```bash
papertowel recipe validate .papertowel/recipes/my-recipe.toml
```

This checks structure and required fields without scanning any code.

## Disabling Built-in Recipes

If a built-in recipe produces too many false positives for your project, exclude the files it trips on using `.papertowelignore`, or suppress individual lines with `// papertowel:ignore-next-line`. There is no per-recipe disable flag at this time.

## CI Integration

Recipes are validated as part of the built-in CI workflow if you use the GitHub Actions pipeline from papertowel's own `.github/workflows/ci.yml`. The `recipes` job runs `papertowel recipe validate` against every TOML in `src/recipes/` on every push.
