# Watermarks-Remover Port Review

Status: Draft
Owner: papertowel maintainers
Date: 2026-08-15
Branch: `refactor/stylometry-detectors`

## 1) Purpose

Inventory the capabilities of [`guillaumemeyer/watermarks-remover`](https://github.com/guillaumemeyer/watermarks-remover) and identify which are worth porting into papertowel. This document is a research artifact — not a spec. Each port candidate has a dedicated subsection with the upstream source mapping, design notes, and acceptance criteria so it can be lifted into a focused PR plan without re-reading the upstream code.

## 2) Source Repo Summary

`watermarks-remover` is a Python service + skill bundle that detects and removes AI-generation marks across three surface types:

1. **Edit-based text marks** — invisible Unicode (ZWJ, ZWNJ, BOM, tag chars U+E0001–U+E007F, variation selectors, private-use, NBSP, bidi controls) and homoglyphs (Cyrillic/fullwidth Latin). Removed by **Layer A** scripts (`text_unicode.py`) — deterministic and verifiable.
2. **Generative / statistical text marks** — token-sampling watermarks (Kirchenbauer, SynthID-Text, Tournament sampling). Targeted by **Layer B** rewrite (`rewrite_text.py`) — best-effort paraphrase, no gold certificate.
3. **File provenance metadata** — C2PA Content Credentials, EXIF/XMP/JUMBF, OOXML props, PNG chunks, YAML frontmatter AI keys. Removed by `clean_file.py` / `clean_image.py` / `container_meta.py`.

It also ships a separate **stylometric detector** (`score_stylometry.py`) that is *zero-LLM*: it computes burstiness (sentence-length CV), lexical diversity (MATTR), and weighted phrase-n-gram density to produce a composite AI-likelihood score without any model calls.

The project is opinionated about honesty: its `references/ethics.md` separates **verifiable** removals (Unicode counts, metadata actions) from **best-effort statistical rewrite** and refuses to claim "no AI provenance left" after a clean.

## 3) Coverage Delta

| Capability | papertowel | watermarks-remover | Gap |
|---|---|---|---|
| Slop lexicon (single words) | ✅ `lexical.rs` (~150 terms) | ❌ | — |
| Phrase patterns (multi-word) | ✅ `phrase-patterns.toml` (~50) | ✅ 24 weighted regex | partial — extend |
| Comment over-documentation | ✅ `comments.rs` | ❌ | — |
| Architecture (god files, traits, layers) | ✅ `architecture.rs` | ❌ | — |
| Commit / workflow / metadata artifacts | ✅ `commit_pattern.rs`, `metadata.rs`, `workflow.rs` | partial | — |
| Security (OWASP, SQLi, secrets, weak crypto) | ✅ `security.rs` SEC001–015 | ❌ | — |
| **Invisible Unicode (ZWJ/ZWNJ/BOM/tag/VS/NBSP/bidi)** | ❌ | ✅ Layer A | MISSING |
| **Homoglyph / NFKC normalization** | ❌ | ✅ Layer A | MISSING |
| **Stylometric scoring (burstiness CV / MATTR / n-gram)** | ❌ | ✅ `score_stylometry.py` | MISSING |
| **Magic-byte / binary-container detection** | ❌ | ✅ `common.looks_binary()` | MISSING |
| Container metadata (DOCX/PDF/SVG/PNG/OOXML/XMP/C2PA) | ❌ | ✅ full | out of scope |
| Markdown YAML frontmatter AI keys | partial (file presence only) | ✅ explicit AI-key list | MISSING (key-level) |
| Statistical paraphrase rewrite | ❌ | ✅ Layer B | intentionally absent |
| Confidence tiers (CLEAN/LOW/MED/HIGH) | `Severity` enum only | ✅ `classify_finding_confidence()` | MISSING (tiered) |

The four bolded gaps are the highest-value ports. The remaining gaps (container metadata, paraphrase rewrite) are out of scope for papertowel as a code-focused tool — see §11.

## 4) Priority 1 — Highest Signal, Lowest Cost

Deterministic, stdlib-only in the upstream, and Rust-native. Best return per line of code.

### 4.1 Invisible-Unicode Scrubber

**Upstream source:** [`service/scripts/text_unicode.py`](https://github.com/guillaumemeyer/watermarks-remover/blob/main/service/scripts/text_unicode.py) + `skills/remove-ai-marks/references/mark-classes.md` (§1 "Edit-based text").

**Inspect kinds to port:**

| Kind | Examples |
|---|---|
| `zwj_family` | ZWSP, ZWNJ, ZWJ, WJ, BOM |
| `bidi` | LRE/RLO/LRI/PDI and friends |
| `tag_chars` | U+E0001–U+E007F |
| `variation_selector` | VS1–VS256 |
| `private_use` | U+E000–F8FF, U+F0000–FFFFD, U+100000–10FFFF |
| `space` | NBSP, em space, ideographic space |
| `confusable` | Cyrillic/fullwidth Latin |

**Load-bearing invisibles** must be preserved by default to avoid corrupting real text:

- emoji glue (ZWJ/VS after an emoji base)
- script joiners (ZWNJ/ZWJ inside complex scripts like Persian, Devanagari)
- flag tag-char sequences
- same-script fillers/selectors (Mongolian free variation selectors, Khmer inherent vowels, Hangul jamo fillers)
- orthographic Arabic/Syriac `Cf` marks

Add `--strip-emoji-glue` (paranoid mode) for users who want all of them stripped regardless.

**Design:**

- New module `src/scrubber/invisible_unicode.rs` exporting `DETECTOR_NAME = "invisible-unicode"`.
- `enum InvisibleKind` with `TryFrom<char>` — fits the `parse, don't validate` rule.
- Character-class table built once with `LazyLock<[(UnicodeRange, InvisibleKind); N]>`, mirroring the `LazyLock<AhoCorasick>` pattern already in `src/scrubber/lexical.rs:14`.
- Detect via `char::is_invisible()`-style predicates (`GeneralCategory::Cf` excluding the carve-outs above).
- **Severity:** High. **Confidence:** 0.92+.

**Proposed config:**

| Setting | Default | Notes |
|---|---|---|
| `min_invisible_chars` | 1 | Trigger threshold per file |
| `preserve_emoji_glue` | true | Never strip emoji ZWJ sequences |
| `preserve_script_joiners` | true | Persian/Devanagari ZWNJ/ZWJ |
| `preserve_tag_sequences` | true | Flag emoji tag sequences |
| `preserve_script_cf_marks` | true | Arabic/Syriac orthographic marks |

**Hookup points:**

- Register in `src/scrubber/mod.rs:1-18`.
- Register in `src/cleanup/mod.rs` detector list (alongside `lexical`, `security`, etc.).
- Optional recipe entry alongside `slop-vocabulary.toml` / `phrase-patterns.toml`.

**Acceptance:**

- New `Finding` category (suggest `FindingCategory::InvisibleUnicode`).
- Unit tests cover: ZWSP-only file, ZWJ-emoji-unchanged, bidi-control file, tag-char file, all-ASCII file (no findings), mixed load-bearing + carrier (only carriers stripped).
- Fixture under `tests/fixtures/` for end-to-end scan.

### 4.2 NFKC Normalization Pass

**Upstream source:** `mark-classes.md` "confusable: Cyrillic/fullwidth Latin (aggressive)".

- Apply `unicode-normalization` crate's `UnicodeNormalization::nfkc()` to scrubbed text on `--normalize` flag.
- Detect homoglyphs (Cyrillic `а`/`о`/`р`/`е`/`х`/etc. vs Latin; fullwidth `\u{FF01}`–`\u{FF5E}`) via a `&[(char, char)]` table.
- Emit a `Finding` per homoglyph cluster (severity High — identifier-level homoglyphs are a known supply-chain vector).

**New module:** `src/scrubber/normalize.rs`.

### 4.3 Binary-Container Sniff

**Upstream source:** [`service/scripts/common.py:looks_binary()`](https://github.com/guillaumemeyer/watermarks-remover/blob/main/service/scripts/common.py) — magic bytes + control-byte ratio.

- Helper: `pub fn looks_binary(bytes: &[u8]) -> bool` with the same heuristic: ZIP/PDF/PNG/JPEG/DOCX/ODT magic prefixes + control-byte ratio in the first 8 KiB.
- Use it in `src/scrubber/metadata.rs:detect_repo_with_config` and the new `invisible_unicode` module to short-circuit before treating binaries as text. Today `metadata.rs` would happily try to scan `target/release/foo` as text.

**New module:** `src/detection/binary.rs`.

## 5) Priority 2 — Statistical Stylometry

The biggest *new signal dimension*. papertowel currently counts slop tokens; it does not measure statistical regularity.

### 5.1 Stylometry Scrubber

**Upstream source:** [`service/scripts/score_stylometry.py`](https://github.com/guillaumemeyer/watermarks-remover/blob/main/service/scripts/score_stylometry.py).

**Three sub-metrics to port:**

| Sub-metric | Formula | LLM signal threshold |
|---|---|---|
| **Burstiness CV** | `stdev(sentence_word_counts) / mean(sentence_word_counts)` | CV < 0.35 = high signal; > 0.55 = human |
| **MATTR (window 50)** | Moving-average type-token ratio over 50-word sliding windows | LLMs cluster 0.68–0.76 |
| **AI n-gram density** | `Σ(count × weight) / (word_count / 100)` | ≥ 2.0 weighted/100w = high signal |

**Composite score:**

```text
raw      = 0.45·burstiness + 0.45·ngram + 0.10·diversity
dampener = if words < 100:
              0.4 + 0.6·(words - 30) / (100 - 30)   # 30 ≤ words ≤ 100
           else 1.0
final    = clamp(raw · dampener, 0.0, 1.0)
```

**Confidence tier mapping** (mirrors upstream `classify_finding_confidence()`):

| Score | Tier |
|---|---|
| < 0.30 | CLEAN |
| 0.30–0.55 | LOW |
| 0.55–0.75 | MEDIUM |
| ≥ 0.75 | HIGH |

**Recommended grade weight:** 15% (between lexical 20% and architecture 20%). Apply only after phase 6 lands — see §10.

**Proposed config:**

| Setting | Default | Source line |
|---|---|---|
| `min_words` | 30 | `MIN_SAMPLE_WORDS` |
| `full_weight_words` | 100 | `FULL_WEIGHT_WORDS` |
| `burstiness_thresholds` | 0.25 / 0.35 / 0.45 / 0.55 | `score_text_stylometry()` |
| `ngram_density_thresholds` | 0.5 / 1.0 / 2.0 | same |
| `mattr_llm_band` | 0.68–0.76 | same |
| `composite_weights` | 0.45 / 0.45 / 0.10 | derived from upstream |

**New module:** `src/scrubber/stylometry.rs`.

## 6) Priority 3 — Phrase-Catalog Expansion

papertowel's `src/recipes/phrase-patterns.toml` is well-structured but thinner than upstream. Append the 24 weighted regexes below. These are pure-data changes — no new code, no new detector.

| Pattern (regex) | Label | Weight |
|---|---|---|
| `\bdelve(?:s\|d)?\s+into\b` | delve into | 1.2 |
| `\ba\s+testament\s+to\b` | a testament to | 1.1 |
| `\brich\s+tapestry(?:\s+of)?\b` | rich tapestry | 1.3 |
| `\bplays?\s+a\s+(?:pivotal\|crucial\|vital\|key)\s+role\b` | plays a pivotal/crucial role | 1.0 |
| `\bin\s+(?:today'?s\|the)\s+(?:(?:fast-paced\|ever-evolving\|digital\|rapidly\s+changing)\s+)*(?:world\|landscape\|era\|environment)\b` | in today's fast-paced world/landscape | 1.4 |
| `\bit\s+is\s+(?:important\|essential\|crucial\|worth\s+noting)\s+to\s+(?:note\|remember\|consider\|highlight)\b` | it is important/crucial to note | 0.9 |
| `\bnot\s+only\b[\w\s,]+\bbut\s+(?:also\s+)?(?:serves\s+to\|acts\s+as\|highlights)\b` | not only … but also serves to | 0.8 |
| `\bserve(?:s\|d)?\s+as\s+a\s+(?:beacon\|reminder\|catalyst\|cornerstone)\b` | serves as a beacon/catalyst | 1.1 |
| `\bunderscore(?:s\|d)?\s+the\s+(?:importance\|need\|significance)\b` | underscores the importance | 0.9 |
| `\bfoster(?:s\|ing\|ed)?\s+a\s+(?:sense\|culture\|deeper\s+understanding)\b` | fosters a sense/culture | 0.9 |
| `\bseamlessly\s+(?:integrates?\|integrated\|blends?\|combine[sd]?)\b` | seamlessly integrates/blends | 1.0 |
| `\bnavigat(?:e\|ing\|es\|ed)\s+the\s+(?:complexities\|intricacies\|nuances)\b` | navigating complexities/intricacies | 1.0 |
| `\bmultifaceted\s+(?:nature\|approach\|landscape)\b` | multifaceted nature | 1.0 |
| `\bharness(?:ing\|ed\|es)?\s+the\s+power\s+of\b` | harnessing the power of | 1.0 |
| `\ba\s+myriad\s+of\b` | a myriad of | 0.8 |
| `\bparadigm\s+shift\b` | paradigm shift | 0.9 |
| `\bholistic\s+(?:approach\|view\|perspective)\b` | holistic approach | 0.9 |
| `\bin\s+conclusion\b[,\s]` | in conclusion | 0.8 |
| `\bto\s+summarize\b[,\s]` | to summarize | 0.8 |
| `\bultimately\b[,\s]` | ultimately, | 0.6 |
| `\bfurthermore\b[,\s]` | furthermore, | 0.6 |
| `\bmoreover\b[,\s]` | moreover, | 0.6 |
| `\bas\s+an\s+ai\b` | as an AI | 1.5 |
| `\bi\s+hope\s+this\s+helps\b` | I hope this helps | 1.2 |

**Format note:** papertowel's existing `phrase-patterns.toml` uses literal-string `match` keys, not regex. The new entries need a `[patterns.regex]` section (or a `match_kind = "regex"` flag on each item) plus an extension to the loader at `src/recipe/loader.rs` to compile via the `regex` crate with `case_insensitive(true)`.

**Acceptance:**

- All 24 patterns resolve to a unique `Finding` per match.
- Existing literal-string phrases still match as before (no regression).
- Fixture regression test: `tests/fixtures/stylometry_ai_sample.txt` scores above 0.75, `stylometry_human_sample.txt` scores below 0.30.

## 7) Priority 4 — Confidence-Tier Classification

**Upstream source:** [`common.py: classify_finding_confidence()`](https://github.com/guillaumemeyer/watermarks-remover/blob/main/service/scripts/common.py).

**In papertowel:** `Finding::severity()` and a raw `confidence: f32` already exist on each finding in `src/detection/finding.rs`. Add a `pub fn confidence_tier(&self) -> ConfidenceTier` helper:

| Tier | Confidence range | Grade impact multiplier |
|---|---|---|
| `Clean` | ≥ 0.95 | 0.0× (suppress from grade) |
| `Low` | 0.80–0.95 | 0.5× |
| `Medium` | 0.65–0.80 | 1.0× |
| `High` | < 0.65 or `Severity::High` | 1.5× |

Small change, but lets `grade` output a confidence tier per finding without changing the math. Refines grade math in §6 of this doc.

## 8) Honesty Model — Worth Adopting Verbatim

The honesty framing in [`skills/remove-ai-marks/references/ethics.md`](https://github.com/guillaumemeyer/watermarks-remover/blob/main/skills/remove-ai-marks/references/ethics.md) draws a sharp line worth quoting in papertowel's docs:

> A removed mark does **not** mean the content was never AI-assisted. Use this toolkit honestly.

The upstream report always separates:

1. **Verifiable** removals (Unicode counts, metadata actions)
2. **Best-effort** statistical rewrite (no gold undetection claim)
3. **Optional / out-of-scope** channels (pixel/audio/video watermarks, C2PA soft binding, secret-key detectors)

papertowel should adopt this tri-state framing in `book/src/scrubber/security.md` and/or a new `book/src/scrubber/invisible-unicode.md` page. Specifically:

- Findings from `invisible_unicode` and `security` are **verifiable** (the byte changed).
- Findings from `stylometry` are **best-effort** (a score, not a proof).
- Nothing in papertowel claims "no AI provenance left."

## 9) Out of Scope

| Capability | Why I'm not porting |
|---|---|
| C2PA / XMP / EXIF / OOXML / PDF metadata | papertowel is code-focused; these target documents & media. Different product surface. |
| Layer B paraphrase rewrite | papertowel's stance ("don't pretend it's human") conflicts with watermarks-remover's removal-of-evidence framing. Even their `ethics.md` admits this is best-effort and unverifiable. |
| Pixel-domain image watermarks | Same — code-only tool. |
| SynthID/CTRLRegen/Diffusion backends | Different category entirely — model-side, not file-side. |
| Markdown YAML frontmatter AI keys (full set) | Worth adding later as a sub-feature of `metadata.rs`; defer until 4.1/4.2 land so the metadata path is well-tested. |

## 10) Recommended Sequencing

Following `copilot-instructions.md` "small, testable increments" and "Make changes in small, testable increments":

| Phase | Step | Risk | Validates |
|---|---|---|---|
| 1 | Add `invisible_unicode` scrubber + recipe (§4.1) | Low — pure addition | New finding category; existing detectors untouched |
| 2 | Add `binary` detection helper (§4.3) | Low — internal helper | No false-positives on `target/`, vendored deps |
| 3 | NFKC normalization (§4.2) — gated behind `--normalize` | Low — opt-in | Safe default; flag-gated |
| 4 | Append the 24 weighted regexes to `phrase-patterns.toml` (§6) | Low — pure extension | Existing tests untouched |
| 5 | `stylometry.rs` with burstiness + MATTR + n-gram density (§5.1) — gated behind `--stylometry` initially | Medium — new score axis | Adds 3rd axis to grading without disturbing existing weights |
| 6 | `confidence_tier()` helper + grade weighting multiplier (§7) | Medium — touches grade math | Refines grade output; needs fixture regression |
| 7 | New `book/src/scrubber/invisible-unicode.md` + stylometry chapter | Low — docs | Doc parity |

**Avoid touching `src/detection/grading.rs`'s weight table until phase 6** — that's the highest-blast-radius change and should ride in its own PR after the new detectors are stable.

## 11) Open Questions

1. **Should the stylometry score appear as a separate detector or fold into `lexical.rs`?** Separate is cleaner (single responsibility) but adds another detector slot. Recommend separate.
2. **Do we want a `--report-confidence-tiers` flag, or always emit them?** Recommend always — the data is already in findings.
3. **YAML frontmatter AI keys (§9 deferred item) — should we add a dedicated detector or extend `metadata.rs`?** Recommend extending `metadata.rs` once 4.1 lands and we have the metadata path well-tested.
4. **Honesty-model adoption (§8) — does this conflict with papertowel's existing tone in `book/src/scrubber/security.md`?** Need to review before drafting.

## 12) References

- [guillaumemeyer/watermarks-remover](https://github.com/guillaumemeyer/watermarks-remover)
- [score_stylometry.py](https://github.com/guillaumemeyer/watermarks-remover/blob/main/service/scripts/score_stylometry.py) — composite stylometry engine
- [text_unicode.py](https://github.com/guillaumemeyer/watermarks-remover/blob/main/service/scripts/text_unicode.py) — invisible Unicode handlers
- [common.py](https://github.com/guillaumemeyer/watermarks-remover/blob/main/service/scripts/common.py) — `looks_binary()`, `classify_finding_confidence()`
- [mark-classes.md](https://github.com/guillaumemeyer/watermarks-remover/blob/main/skills/remove-ai-marks/references/mark-classes.md) — mark taxonomy
- [how-claude-marks.md](https://github.com/guillaumemeyer/watermarks-remover/blob/main/skills/remove-ai-marks/references/how-claude-marks.md) — vendor-specific mark context
- [ethics.md](https://github.com/guillaumemeyer/watermarks-remover/blob/main/skills/remove-ai-marks/references/ethics.md) — honesty framing
- [Anthropic "How Claude marks AI-generated content"](https://support.claude.com/en/articles/16266773-how-claude-marks-ai-generated-content) — primary vendor source
