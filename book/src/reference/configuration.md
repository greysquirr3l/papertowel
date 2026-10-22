# Configuration and Ignoring Files

`papertowel` provides several layers of control over which files are analysed and which findings are reported.

## `.papertowel.toml`

Drop a `.papertowel.toml` in your repository root to configure detectors, severity thresholds, and path exclusions. Every section is optional — missing sections use sensible defaults.

```toml
[detectors]
lexical = true
comments = true
structure = true
readme = true
metadata = true
commit_pattern = true
tests = true
workflow = true
maintenance = true
promotion = true
name_credibility = true
idiom_mismatch = true
prompt = true
security = true

[severity]
minimum = "medium"   # "low", "medium", or "high"

[scrubber]
aggression = "moderate"  # "gentle", "moderate", or "aggressive"

[exclude]
paths = [
    "vendor/",
    "generated/**/*.rs",
]
```

Patterns in `[exclude].paths` use **gitignore syntax** and are merged with the `.papertowelignore` file described below.

## Overriding Hardcoded Rules

Some rules are wrong for your project. A team that ships a test harness will
trip the `harness` vocabulary rule on every file; a security tool may flag a
deliberate `eval` in a sandboxed REPL. Rather than disabling a whole
detector, you can override the individual rules.

There are two ways to do this, and they can be combined.

### Suppressing a term

`exclude_terms` removes a literal term from the vocabulary rules. It applies to
both the built-in slop list and the recipe-backed vocabulary, and it also stops
`papertowel scrub` from rewriting the term.

```toml
[detectors.lexical]
exclude_terms = ["harness", "robust"]
```

Matching is case-insensitive unless `case_sensitive = true` is set.

### Overriding a rule by id

The `[rules]` section addresses any rule by the id that appears in scan output.
This is the escape hatch for rules that `exclude_terms` cannot express —
built-in detectors, phrase patterns, and regex rules included.

```toml
[rules]
# Drop the rule entirely: not reported by `scan`, not applied by `scrub`.
disable = [
    "slop-vocabulary:word:harness",
    "SEC002",
]

# Re-grade a rule without losing the signal.
[rules.severity]
"SEC002" = "low"
"slop-vocabulary:word:robust" = "high"
```

Rule ids follow these formats:

| Source | Id format | Example |
| --- | --- | --- |
| Recipe vocabulary | `<recipe>:word:<word>` | `slop-vocabulary:word:harness` |
| Recipe phrase | `<recipe>:phrase:<phrase>` | `phrase-patterns:phrase:I-hope-this-helps` |
| Recipe regex | `<recipe>:regex:<name>` | `invisible-unicode:regex:zwsp` |
| Built-in detectors | `SEC001`–`SEC015`, `STYLOMETRY-001`, … | `SEC002` |

Run `papertowel scan --format json` to list the exact ids your project
currently triggers.

### How overrides interact

- A disabled rule is removed from both `scan` and `scrub`. Suppressing a term
  silences the report *and* stops the scrubber from rewriting it.
- Severity overrides apply to recipe rules at load time and to built-in
  detector rules when findings are collected, so both paths end up with the
  same effective severity.
- Overrides in a repo-level `.papertowel.toml` are merged with the global
  config in `~/.config/papertowel/config.toml`: disabled ids union, and
  repo-level severity entries win on conflict.

## `.papertowelignore`

Create a `.papertowelignore` file in your repo root to list paths that should be skipped entirely. The syntax is identical to `.gitignore`.

```gitignore
# Build output
target/

# Vendored code we don't control
third_party/

# Files that legitimately contain slop vocabulary
src/scrubber/lexical.rs
```

Both `scan` and `scrub` honour this file. Ignored files are never read by any detector or transform.

## Inline Directives

For finer-grained control, `papertowel` recognises two comment directives that can be placed directly in source files.

### `papertowel:ignore-file`

Place this directive in any comment near the top of a file to tell `papertowel` to skip the entire file. This is useful when a file *must* contain slop vocabulary (for example, a test fixture or a reference corpus).

```rust
// papertowel:ignore-file
pub const SLOP_WORDS: &[&str] = &["robust", "seamless", "delve"];
```

The directive works with any single-line comment style:

```python
# papertowel:ignore-file
SLOP = ["robust", "seamless", "delve"]
```

```sql
-- papertowel:ignore-file
SELECT * FROM slop_words;
```

### `papertowel:ignore-next-line`

Suppresses findings that start on the immediately following line. Use this when a single line legitimately triggers a detector but the rest of the file should still be analysed.

```rust
// papertowel:ignore-next-line
let description = "A robust and comprehensive guide";
```

Multiple directives can appear in the same file:

```rust
fn build_corpus() -> Vec<&'static str> {
    vec![
        // papertowel:ignore-next-line
        "delve",
        // papertowel:ignore-next-line
        "facilitate",
        "normal_word",
    ]
}
```

## Precedence

Suppression layers are evaluated in order:

1. **`.papertowelignore` / `[exclude].paths`** — file is never opened.
2. **`papertowel:ignore-file`** — file is read but all detectors are skipped.
3. **`papertowel:ignore-next-line`** — detectors run, but findings on the suppressed line are removed from the report.
