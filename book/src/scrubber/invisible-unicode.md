# Invisible Unicode Detection

Detects visually-ambiguous or invisible Unicode codepoints commonly introduced by AI-edit watermarking or as supply-chain homograph vectors. The classifier table lives in `src/scrubber/invisible_unicode.rs`; this page documents what is detected, the load-bearing carve-outs, and the detection categories.

## What It Detects

The detector classifies six kinds of invisible / visually-ambiguous characters:

| Kind | Examples |
|---|---|
| `zwj_family` | ZWSP, ZWNJ, ZWJ, WJ, BOM |
| `bidi` | LRE / RLE / PDF / LRO / RLO / LRI / RLI / FSI / PDI |
| `tag_char` | U+E0001 - U+E007F |
| `variation_selector` | VS1 - VS256 (BMP + SMP) |
| `private_use` | BMP, PUA-A (U+F0000..U+FFFFD), PUA-B (U+100000..U+10FFFD) |
| `exotic_space` | NBSP, en/em/figure/thin/hair, IDSP, LSEP, PSEP, NNBSP, MMSP |

These are the same six categories documented in `watermarks-remover`'s `mark-classes.md` §1 "Edit-based text" - Layer A, deterministic and verifiable.

## Load-Bearing Carve-Outs

By default, the detector **preserves** characters that carry real meaning even though they fall in the same Unicode classes. Each carve-out is configurable:

- **Emoji glue** (ZWJ / VS between emoji bases): preserved by default. The string `\u{1F680}\u{200D}\u{1F525}` (rocket + ZWJ + fire) is a legitimate emoji sequence; without the carve-out, the detector would flag the ZWJ as invisible cruft.
- **Script joiners** (ZWNJ / ZWJ inside Persian, Devanagari, Arabic): preserved by default. Removing a Persian ZWNJ would break the script.
- **Flag-emoji tag sequences** (regional-indicator × tag char × tag char × ...): preserved by default.
- **Script-internal Cf marks** in Arabic / Syriac / Hebrew blocks: preserved by default.

Add `--strip-emoji-glue` (or set `preserve_emoji_glue = false`) for paranoid mode that strips every Cf carrier regardless of script context.

## How It Finds Them

The detector iterates the source text character by character, classifying each `char` via the `classify_codepoint` table (a const `matches!` over the relevant Unicode ranges). For each classified codepoint it computes line number, byte offset, and a load-bearing flag by inspecting the previous and next `char` in the text.

Findings are emitted as `FindingCategory::InvisibleUnicode` with **severity High** and **confidence 0.92** when at least `min_invisible_chars` non-load-bearing invisibles are present in the file. `min_invisible_chars` defaults to 1 with a hard cap of 50 findings per file to bound noise on heavily contaminated inputs.

## Recipe Form

`src/recipes/invisible-unicode.toml` exposes the detector under the `papertowel recipe list` and `[include_recipes]` / `[exclude_recipes]` filters. The TOML metadata block is intentionally sparse - the actual classification logic lives in Rust so the regex table cannot be bypassed by user recipe overrides.

## Configuration

| Setting | Default | Notes |
|---|---|---|
| `min_invisible_chars` | 1 | Trigger threshold per file |
| `preserve_emoji_glue` | true | Never strip emoji ZWJ sequences |
| `preserve_script_joiners` | true | Persian / Devanagari ZWNJ / ZWJ |
| `preserve_tag_sequences` | true | Flag emoji tag sequences |
| `preserve_script_cf_marks` | true | Arabic / Syriac / Hebrew orthographic marks |

## Honesty

The detector reports the classification and severity. It does **not** claim "this character was inserted by an LLM" - invisible Unicode is also a long-standing supply-chain vector for non-AI malware. The detector's job is to surface the byte; the interpretation is the user's.

## Sources

- `watermarks-remover` `skills/remove-ai-marks/references/mark-classes.md` §1 "Edit-based text"
- `watermarks-remover` `service/scripts/text_unicode.py`
- Unicode Standard Annex #24 (`General Category = Cf`, `Mn`, `Me`)
