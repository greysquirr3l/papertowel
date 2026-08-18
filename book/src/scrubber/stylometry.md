# Statistical Stylometry

Zero-LLM composite scorer that finds AI-telltale text along three dimensions: sentence-length burstiness, lexical diversity, and weighted AI phrase n-gram density. The detector returns a score, not a verdict - and never claims a piece of text is human or AI.

## The Three Sub-Scores

| Sub-metric | Formula | LLM signal threshold |
|---|---|---|
| **Burstiness CV** | `stdev(sentence_word_counts) / mean(sentence_word_counts)` | CV < 0.35 = high signal; > 0.55 = human |
| **MATTR-50** | Moving-average type-token ratio over 50-word sliding windows | LLMs cluster 0.68 - 0.76 |
| **AI n-gram density** | Σ(count × weight) per 100 words across the 24 weighted patterns | ≥ 2.0 weighted / 100w = high signal |

The three subscores combine with weights **0.45 / 0.45 / 0.10**, are dampened below 100 words to scale 0.4x to 1.0x, and produce a final score clamped to `0.0..1.0`.

## Confidence Tiers

The final score maps to one of four tiers, used for both reporting and (in Phase 6) grade-weighting:

| Score | Tier | Grade multiplier |
|---|---|---|
| < 0.30 | Clean | 0.0x |
| 0.30 - 0.55 | Low | 0.5x |
| 0.55 - 0.75 | Medium | 1.0x |
| ≥ 0.75 | High | 1.5x |

The grade multiplier is a Phase-6 concept: today the detector only emits the tier label and the score; the grade weight comes later.

## Composite Scoring Pipeline

1. `extract_sentences(text)` skips fenced code blocks (triple-backtick fenced), splits on `[.!?] + whitespace + capital` boundaries.
2. `extract_words(text)` lowercases and tokenizes on alphanumeric / `'` / `-`.
3. `burstiness(sentences) -> (mean, stddev, cv)`. Returns `(0, 0, 0)` for fewer than two non-empty sentences.
4. `mattr(words, 50) -> f32`. Sliding-window type-token ratio updated incrementally with a `HashMap<&str, u32>` of counts.
5. `matched_phrase_markers(text) -> Vec<MatchedMarker>`. Sweeps all 24 weighted patterns over the text; returns one entry per pattern that matched.
6. `ai_ngram_density(text, word_count) -> f32`. Σ(count × weight) per 100 words.
7. `sub_scores(cv, ngram_density, mattr_value) -> (burst, ngram, diversity)`. Per-axis subscore in 0..1.
8. `raw_composite = 0.45*burst + 0.45*ngram + 0.10*diversity`.
9. `dampener(word_count, full_weight_words)` scales linearly from 0.4 at MIN_SAMPLE_WORDS to 1.0 at FULL_WEIGHT_WORDS.
10. `final_score = clamp(raw_composite * dampener, 0.0, 1.0)`.

## Sample-Size Guard

Samples below `min_words` (default 30) are returned as `StylometryStatus::InsufficientLength` with `confidence_tier = Clean` and `final_score = 0.0`. The phrase-marker pass still runs (the catalog can fire on as little as one phrase) and the findings are surfaced, but no statistical composite is emitted. This keeps the detector from overclaiming on short text - short snippets look AI-ish by default because there is no signal to discriminate.

## Configuration

| Setting | Default | Source line |
|---|---|---|
| `min_words` | 30 | `MIN_SAMPLE_WORDS` |
| `full_weight_words` | 100 | `FULL_WEIGHT_WORDS` |
| `mattr_window` | 50 | `MATTR_WINDOW` |
| `composite_weight_burst` | 0.45 | upstream |
| `composite_weight_ngram` | 0.45 | upstream |
| `composite_weight_diversity` | 0.10 | upstream |

## Pattern Catalog

The 24 weighted patterns come from `watermarks-remover`'s `score_stylometry.py` `AI_PHRASE_PATTERNS`, mapped by weight to severity:

- **High tier** (weight ≥ 1.0): "delve into", "as an AI", "rich tapestry", "I hope this helps", "a testament to", "today's fast-paced world", "serves as a beacon/catalyst", "plays a pivotal/crucial role", "seamlessly integrates/blends", "navigating the complexities/intricacies", "multifaceted nature/approach", "harnessing the power of".
- **Medium tier** (0.8 - 1.0): "underscores the importance", "fosters a sense/culture", "paradigm shift", "holistic approach", "not only ... but also serves", "a myriad of", "in conclusion", "to summarize", "it is important to note/remember".
- **Low tier** (< 0.8): "ultimately,", "furthermore,", "moreover,".

These patterns are also exposed as `[[patterns.regex]]` entries in `src/recipes/phrase-patterns.toml` so the recipe matcher surfaces them in addition to the composite scorer.

## Honesty

The detector returns a **score**, not a verdict. The `Suggestion` field on emitted `Finding`s reads:

> Statistical stylometry signals AI-typical cadence (tier). This is a score, not a proof - no LLM call required.

The detector cannot prove AI authorship. It surfaces statistical regularity that **correlates** with LLM output across the training data of `watermarks-remover`'s authors; it does not establish identity. The composite scorer is zero-LLM - the same metrics would have flagged the same patterns in any sufficiently uniform text - so the cost of a false positive is a confidence-tier label, not a fingerprint claim.

## Sources

- `watermarks-remover` `service/scripts/score_stylometry.py`
- `watermarks-remover` `skills/remove-ai-marks/references/mark-classes.md` §2 "Generative / statistical text"
- Stanford NLP: `Lexical Diversity & Type-Token Ratio` (MATTR definition)
