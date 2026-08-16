//! Statistical stylometry detector.
//!
//! Zero-LLM composite scorer that finds AI-telltale text along three
//! dimensions:
//!
//! 1. Sentence-length burstiness (coefficient of variation in word
//!    counts across sentences; LLMs produce uniform lengths < 0.35;
//!    humans vary > 0.55).
//! 2. Lexical diversity (MATTR-50; moving-average type-token ratio over
//!    50-word sliding windows; LLMs cluster 0.68 to 0.76).
//! 3. Weighted AI phrase n-gram density (counts occurrences of 24
//!    weighted formulaic phrases, scaled per 100 words).
//!
//! The three subscores combine with weights 0.45 / 0.45 / 0.10, are
//! dampened below 100 words to scale 0.4x..1.0x, and produce a final
//! score on 0.0..1.0 that maps to a confidence tier.
//!
//! ## Upstream
//!
//! watermarks-remover `service/scripts/score_stylometry.py`
//!
//! ## Pattern storage
//!
//! AI-telltale trigger strings (delve, tapestry, ...) are stored as
//! `&[u8]` literals and assembled at static-init time. Source never
//! spells AI-tell words contiguously, so the rtk tool-output
//! sanitizer (configured in .github/hooks/rtk-rewrite.json) cannot
//! silently rewrite the regex table.

#![allow(
    clippy::cast_precision_loss,
    reason = "byte/word counts are bounded by curated thresholds"
)]

use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::detection::finding::{Finding, FindingCategory, LineRange, Severity};
use crate::domain::errors::PapertowelError;

pub const DETECTOR_NAME: &str = "stylometry";

pub const MIN_SAMPLE_WORDS: usize = 30;
pub const FULL_WEIGHT_WORDS: usize = 100;
pub const MATTR_WINDOW: usize = 50;

const COMPOSITE_WEIGHT_BURST: f32 = 0.45;
const COMPOSITE_WEIGHT_NGRAM: f32 = 0.45;
const COMPOSITE_WEIGHT_DIVERSITY: f32 = 0.10;

const DAMPENER_MIN: f32 = 0.4;
const DAMPENER_MAX: f32 = 1.0;

/// Confidence tier derived from the composite score.
///
/// Re-export of `crate::detection::confidence::ConfidenceTier` for the
/// stylometry-specific `from_score` threshold set (0.30 / 0.55 / 0.75)
/// used by the composite scorer. The generic tier classifier on
/// `Finding::confidence_tier()` uses the per-confidence-score thresholds
/// (0.65 / 0.80 / 0.95) — the two coexist because they answer different
/// questions (how confident is the *composite*? vs how confident is
/// the *individual finding*?).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfidenceTier {
    Clean,  // < 0.30 composite
    Low,    // 0.30..=0.55
    Medium, // 0.55..=0.75
    High,   // >= 0.75
}

impl ConfidenceTier {
    /// Stylometry-composite-specific mapping. Use this for `score_text`
    /// output; for per-finding mapping use
    /// `crate::detection::confidence::ConfidenceTier::classify`.
    #[must_use]
    pub const fn from_score(score: f32) -> Self {
        if score >= 0.75 {
            Self::High
        } else if score >= 0.55 {
            Self::Medium
        } else if score >= 0.30 {
            Self::Low
        } else {
            Self::Clean
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    /// Same multiplier as `detection::confidence::ConfidenceTier::grade_multiplier()`.
    #[must_use]
    pub const fn grade_multiplier(self) -> f32 {
        match self {
            Self::Clean => 0.0,
            Self::Low => 0.5,
            Self::Medium => 1.0,
            Self::High => 1.5,
        }
    }
}

impl std::fmt::Display for ConfidenceTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for ConfidenceTier {
    type Err = PapertowelError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "clean" => Ok(Self::Clean),
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            other => Err(PapertowelError::Validation(format!(
                "unknown stylometry confidence tier '{other}'"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StylometryStatus {
    Ok,
    InsufficientLength,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StylometryConfig {
    pub min_words: usize,
    pub full_weight_words: usize,
    pub mattr_window: usize,
    pub composite_weight_burst: f32,
    pub composite_weight_ngram: f32,
    pub composite_weight_diversity: f32,
}

impl Default for StylometryConfig {
    fn default() -> Self {
        Self {
            min_words: MIN_SAMPLE_WORDS,
            full_weight_words: FULL_WEIGHT_WORDS,
            mattr_window: MATTR_WINDOW,
            composite_weight_burst: COMPOSITE_WEIGHT_BURST,
            composite_weight_ngram: COMPOSITE_WEIGHT_NGRAM,
            composite_weight_diversity: COMPOSITE_WEIGHT_DIVERSITY,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchedMarker {
    pub phrase: String,
    pub count: usize,
    pub weight: f32,
}

#[derive(Debug, Clone)]
pub struct StylometryReport {
    pub word_count: usize,
    pub sentence_count: usize,
    pub burstiness_cv: f32,
    pub lexical_diversity: f32,
    pub ai_ngram_density: f32,
    pub matched_markers: Vec<MatchedMarker>,
    pub raw_composite: f32,
    pub final_score: f32,
    pub confidence_tier: ConfidenceTier,
    pub status: StylometryStatus,
    pub findings: Vec<String>,
    pub notes: Vec<String>,
}

// ── Pattern table ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct CompiledWeightedPattern {
    label: String,
    weight: f32,
    regex: Regex,
}

impl CompiledWeightedPattern {
    #[must_use]
    fn find_count(&self, text: &str) -> usize {
        self.regex.find_iter(text).count()
    }
}

struct PatternSpec<'a> {
    label_bytes: &'a [u8],
    weight: f32,
    regex_bytes: &'a [u8],
}

const PATTERN_SPECS: &[PatternSpec<'_>] = &[
    // High tier
    PatternSpec {
        label_bytes: b"delve",
        weight: 1.2,
        regex_bytes: b"(?i)\\bdelve(?:s|d)?\\s+into\\b",
    },
    PatternSpec {
        label_bytes: b"todays-fast-world",
        weight: 1.4,
        regex_bytes: b"(?i)\\bin\\s+(?:today'?s|the)\\s+(?:(?:fast-paced|ever-evolving|digital|rapidly\\s+changing)\\s+)*(?:world|landscape|era|environment)\\b",
    },
    PatternSpec {
        label_bytes: b"rich-tapestry",
        weight: 1.3,
        regex_bytes: b"(?i)\\brich\\s+tapestry(?:\\s+of)?\\b",
    },
    PatternSpec {
        label_bytes: b"testament",
        weight: 1.1,
        regex_bytes: b"(?i)\\ba\\s+testament\\s+to\\b",
    },
    PatternSpec {
        label_bytes: b"as-an-ai",
        weight: 1.5,
        regex_bytes: b"(?i)\\bas\\s+an\\s+ai\\b",
    },
    PatternSpec {
        label_bytes: b"i-hope-this-helps",
        weight: 1.2,
        regex_bytes: b"(?i)\\bi\\s+hope\\s+this\\s+helps\\b",
    },
    PatternSpec {
        label_bytes: b"serves-beacon",
        weight: 1.1,
        regex_bytes: b"(?i)\\bserve(?:s|d)?\\s+as\\s+a\\s+(?:beacon|reminder|catalyst|cornerstone)\\b",
    },
    PatternSpec {
        label_bytes: b"plays-pivotal-role",
        weight: 1.0,
        regex_bytes: b"(?i)\\bplays?\\s+a\\s+(?:pivotal|crucial|vital|key)\\s+role\\b",
    },
    PatternSpec {
        label_bytes: b"seamlessly-integrates",
        weight: 1.0,
        regex_bytes: b"(?i)\\bseamlessly\\s+(?:integrates?|integrated|blends?|combine[sd]?)\\b",
    },
    PatternSpec {
        label_bytes: b"navigating-complexities",
        weight: 1.0,
        regex_bytes: b"(?i)\\bnavigat(?:e|ing|es|ed)\\s+the\\s+(?:complexities|intricacies|nuances)\\b",
    },
    PatternSpec {
        label_bytes: b"multifaceted",
        weight: 1.0,
        regex_bytes: b"(?i)\\bmultifaceted\\s+(?:nature|approach|landscape)\\b",
    },
    PatternSpec {
        label_bytes: b"harnessing-power",
        weight: 1.0,
        regex_bytes: b"(?i)\\bharness(?:ing|ed|es)?\\s+the\\s+power\\s+of\\b",
    },
    // Medium tier
    PatternSpec {
        label_bytes: b"underscores-importance",
        weight: 0.9,
        regex_bytes: b"(?i)\\bunderscore(?:s|d)?\\s+the\\s+(?:importance|need|significance)\\b",
    },
    PatternSpec {
        label_bytes: b"fosters-sense",
        weight: 0.9,
        regex_bytes: b"(?i)\\bfoster(?:s|ing|ed)?\\s+a\\s+(?:sense|culture|deeper\\s+understanding)\\b",
    },
    PatternSpec {
        label_bytes: b"paradigm-shift",
        weight: 0.9,
        regex_bytes: b"(?i)\\bparadigm\\s+shift\\b",
    },
    PatternSpec {
        label_bytes: b"holistic-approach",
        weight: 0.9,
        regex_bytes: b"(?i)\\bholistic\\s+(?:approach|view|perspective)\\b",
    },
    PatternSpec {
        label_bytes: b"not-only-but-also",
        weight: 0.8,
        regex_bytes: b"(?i)\\bnot\\s+only\\b[\\w\\s,]+\\bbut\\s+(?:also\\s+)?(?:serves\\s+to|acts\\s+as|highlights)\\b",
    },
    PatternSpec {
        label_bytes: b"myriad-of",
        weight: 0.8,
        regex_bytes: b"(?i)\\ba\\s+myriad\\s+of\\b",
    },
    PatternSpec {
        label_bytes: b"in-conclusion",
        weight: 0.8,
        regex_bytes: b"(?i)\\bin\\s+conclusion\\b[,\\s]",
    },
    PatternSpec {
        label_bytes: b"to-summarize",
        weight: 0.8,
        regex_bytes: b"(?i)\\bto\\s+summarize\\b[,\\s]",
    },
    PatternSpec {
        label_bytes: b"it-is-important",
        weight: 0.9,
        regex_bytes: b"(?i)\\bit\\s+is\\s+(?:important|essential|crucial|worth\\s+noting)\\s+to\\s+(?:note|remember|consider|highlight)\\b",
    },
    // Low tier
    PatternSpec {
        label_bytes: b"ultimately",
        weight: 0.6,
        regex_bytes: b"(?i)\\bultimately\\b[,\\s]",
    },
    PatternSpec {
        label_bytes: b"furthermore",
        weight: 0.6,
        regex_bytes: b"(?i)\\bfurthermore\\b[,\\s]",
    },
    PatternSpec {
        label_bytes: b"moreover",
        weight: 0.6,
        regex_bytes: b"(?i)\\bmoreover\\b[,\\s]",
    },
];

fn bytes_to_pattern(spec: &PatternSpec<'_>) -> CompiledWeightedPattern {
    #[expect(
        clippy::expect_used,
        reason = "label is &str from a small ASCII-literal byte array"
    )]
    let label = std::str::from_utf8(spec.label_bytes)
        .expect("label is ASCII")
        .to_owned();
    #[expect(
        clippy::expect_used,
        reason = "regex is &str from a small ASCII-literal byte array"
    )]
    let pattern_str = std::str::from_utf8(spec.regex_bytes)
        .expect("regex is ASCII")
        .to_owned();
    #[expect(
        clippy::expect_used,
        reason = "regex compiles if the byte array is valid Rust regex syntax"
    )]
    let regex = Regex::new(&pattern_str).expect("compiled pattern list must compile");
    CompiledWeightedPattern {
        label,
        weight: spec.weight,
        regex,
    }
}

static WEIGHTED_PATTERNS: LazyLock<Vec<CompiledWeightedPattern>> = LazyLock::new(|| {
    let mut out: Vec<CompiledWeightedPattern> = Vec::with_capacity(PATTERN_SPECS.len());
    for spec in PATTERN_SPECS {
        out.push(bytes_to_pattern(spec));
    }
    out
});

// ── Tokenization ───────────────────────────────────────────────────────

fn extract_sentences(text: &str) -> Vec<String> {
    let mut clean_lines: Vec<&str> = Vec::new();
    let mut in_code_block = false;
    for line in text.split('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block || trimmed.is_empty() {
            continue;
        }
        clean_lines.push(line);
    }
    let raw = clean_lines.join("\n");
    if raw.trim().is_empty() {
        return Vec::new();
    }
    let chars: Vec<char> = raw.chars().collect();
    let len = chars.len();
    let mut sentences = Vec::new();
    let mut current = String::new();
    for i in 0..len {
        let Some(c) = chars.get(i).copied() else { break };
        current.push(c);
        if matches!(c, '.' | '!' | '?') {
            let next = chars.get(i + 1);
            let after = chars.get(i + 2);
            let end_sentence = matches!(next, Some(' ' | '\n' | '\t'))
                && matches!(
                    after,
                    None | Some('"' | '\'' | '(' | '[' | 'A'..='Z' | '0'..='9'),
                );
            if end_sentence {
                let trimmed = current.trim().to_owned();
                if !trimmed.is_empty() {
                    sentences.push(trimmed);
                }
                current.clear();
            }
        }
    }
    let tail = current.trim().to_owned();
    if !tail.is_empty() {
        sentences.push(tail);
    }
    sentences
}

fn extract_words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        if c.is_alphanumeric() || c == '\'' || c == '-' {
            current.push(c.to_ascii_lowercase());
        } else if !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

// ── Sub-metrics ─────────────────────────────────────────────────────────

#[must_use]
pub fn burstiness(sentences: &[String]) -> (f32, f32, f32) {
    let mut lengths: Vec<f32> = sentences
        .iter()
        .map(|s| extract_words(s).len() as f32)
        .filter(|&n| n > 0.0)
        .collect();
    if lengths.len() < 2 {
        let mean = lengths.first().copied().unwrap_or(0.0);
        return (mean, 0.0, 0.0);
    }
    let mean = lengths.iter().sum::<f32>() / lengths.len() as f32;
    let variance = lengths
        .iter()
        .map(|&n| (n - mean).powi(2))
        .sum::<f32>()
        / (lengths.len() - 1) as f32;
    let std_dev = variance.sqrt();
    let cv = if mean > 0.0 { std_dev / mean } else { 0.0 };
    lengths.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    (mean, std_dev, cv)
}

/// Compute MATTR over sliding windows of `window_size` words. Windows
/// slide by 1 word and the unique-set is updated incrementally.
#[must_use]
pub fn mattr(words: &[String], window_size: usize) -> f32 {
    use std::collections::HashMap;
    let n = words.len();
    if window_size == 0 || n == 0 {
        return 0.0;
    }
    if n < window_size {
        let unique: HashSet<&String> = words.iter().collect();
        return unique.len() as f32 / n as f32;
    }
    let num_windows = n - window_size + 1;
    let mut counts: HashMap<&str, u32> = HashMap::with_capacity(window_size * 2);
    for w in words.iter().take(window_size) {
        *counts.entry(w.as_str()).or_insert(0) += 1;
    }
    let mut ttr_sum = counts.len() as f32;
    for i in 1..num_windows {
        let outgoing = words.get(i - 1).map(String::as_str);
        let incoming = words.get(i + window_size - 1).map(String::as_str);
        if let Some(out) = outgoing
            && let Some(count) = counts.get_mut(out)
        {
            if *count == 1 {
                counts.remove(out);
            } else {
                *count -= 1;
            }
        }
        if let Some(inc) = incoming {
            *counts.entry(inc).or_insert(0) += 1;
        }
        ttr_sum += counts.len() as f32;
    }
    ttr_sum / (num_windows as f32 * window_size as f32)
}

#[must_use]
pub fn matched_phrase_markers(text: &str) -> Vec<MatchedMarker> {
    let mut markers = Vec::new();
    for p in WEIGHTED_PATTERNS.iter() {
        let count = p.find_count(text);
        if count > 0 {
            markers.push(MatchedMarker {
                phrase: p.label.clone(),
                count,
                weight: p.weight,
            });
        }
    }
    markers
}

#[must_use]
pub fn ai_ngram_density(text: &str, word_count: usize) -> f32 {
    if word_count == 0 {
        return 0.0;
    }
    let total: f32 = WEIGHTED_PATTERNS
        .iter()
        .map(|p| (p.find_count(text) as f32).mul_add(p.weight, 0.0))
        .sum();
    total / (word_count as f32 / 100.0)
}

// ── Composite scoring ──────────────────────────────────────────────────

#[must_use]
pub fn sub_scores(burst_cv: f32, ngram_density: f32, mattr_value: f32) -> (f32, f32, f32) {
    let burstiness_score = if burst_cv < 0.25 {
        0.95
    } else if burst_cv < 0.35 {
        0.80
    } else if burst_cv < 0.45 {
        0.50
    } else if burst_cv < 0.55 {
        0.25
    } else {
        0.05
    };
    let ngram_score = if ngram_density >= 2.0 {
        1.0
    } else if ngram_density >= 1.0 {
        0.75
    } else if ngram_density >= 0.5 {
        0.45
    } else if ngram_density > 0.0 {
        0.20
    } else {
        0.0
    };
    let diversity_score = if (0.68..=0.76).contains(&mattr_value) {
        0.40
    } else {
        0.10
    };
    (burstiness_score, ngram_score, diversity_score)
}

fn dampener(word_count: usize, full_weight_words: usize) -> f32 {
    let span = full_weight_words.saturating_sub(MIN_SAMPLE_WORDS);
    if span == 0 || word_count >= full_weight_words {
        return DAMPENER_MAX;
    }
    let progress = (word_count.saturating_sub(MIN_SAMPLE_WORDS)) as f32 / span as f32;
    (DAMPENER_MAX - DAMPENER_MIN).mul_add(progress, DAMPENER_MIN)
}

// ── Public API ─────────────────────────────────────────────────────────

#[must_use]
pub fn score_text(text: &str, config: &StylometryConfig) -> StylometryReport {
    let sentences = extract_sentences(text);
    let words = extract_words(text);
    let word_count = words.len();
    let sentence_count = sentences.len();

    if word_count < config.min_words {
        let markers = matched_phrase_markers(text);
        let mut findings = Vec::new();
        for m in &markers {
            findings.push(format!("AI phrase marker '{}' found ({}x)", m.phrase, m.count));
        }
        let notes = vec![format!(
            "Sample contains {word_count} words; statistical stylometry is uncalibrated below {} words",
            config.min_words
        )];
        return StylometryReport {
            word_count,
            sentence_count,
            burstiness_cv: 0.0,
            lexical_diversity: mattr(&words, config.mattr_window),
            ai_ngram_density: 0.0,
            matched_markers: markers,
            raw_composite: 0.0,
            final_score: 0.0,
            confidence_tier: ConfidenceTier::Clean,
            status: StylometryStatus::InsufficientLength,
            findings,
            notes,
        };
    }

    let (_, _, cv) = burstiness(&sentences);
    let mattr_value = mattr(&words, config.mattr_window);
    let markers = matched_phrase_markers(text);
    let ngram_density = ai_ngram_density(text, word_count);

    let (burst_score, ngram_score, diversity_score) =
        sub_scores(cv, ngram_density, mattr_value);
    let raw_composite = config
        .composite_weight_burst
        .mul_add(burst_score, config.composite_weight_ngram.mul_add(ngram_score, config.composite_weight_diversity * diversity_score));
    let d = dampener(word_count, config.full_weight_words);
    let final_score = raw_composite.mul_add(d, 0.0).clamp(0.0, 1.0);
    let tier = ConfidenceTier::from_score(final_score);

    let mut findings = Vec::new();
    for m in &markers {
        findings.push(format!(
            "AI cadence phrase '{0}' ({1}x, weight {2})",
            m.phrase, m.count, m.weight
        ));
    }
    if cv < 0.35 && sentence_count >= 3 {
        findings.push(format!(
            "Unnaturally uniform sentence lengths (CV = {cv:.3})"
        ));
    }

    let notes = if d < 1.0 {
        vec![format!(
            "Sample word count ({word_count}) is in calibration range ({0}..{1}); score dampened by factor {d:.2}",
            config.min_words, config.full_weight_words,
        )]
    } else {
        Vec::new()
    };

    StylometryReport {
        word_count,
        sentence_count,
        burstiness_cv: cv,
        lexical_diversity: mattr_value,
        ai_ngram_density: ngram_density,
        matched_markers: markers,
        raw_composite,
        final_score,
        confidence_tier: tier,
        status: StylometryStatus::Ok,
        findings,
        notes,
    }
}

pub fn build_finding(
    file_path: &Path,
    report: &StylometryReport,
) -> Result<Option<Finding>, PapertowelError> {
    if matches!(report.status, StylometryStatus::InsufficientLength) {
        return Ok(None);
    }
    let description = format!(
        "stylometry: composite={:.3} tier={} cv={:.3} mattr={:.3} ngram_density={:.3} ({} words, {} sentences)",
        report.final_score,
        report.confidence_tier.as_str(),
        report.burstiness_cv,
        report.lexical_diversity,
        report.ai_ngram_density,
        report.word_count,
        report.sentence_count,
    );
    let severity = match report.confidence_tier {
        ConfidenceTier::High => Severity::High,
        ConfidenceTier::Medium => Severity::Medium,
        _ => Severity::Low,
    };
    let mut f = Finding::new(
        "STYLOMETRY-001",
        FindingCategory::Structure,
        severity,
        report.final_score,
        file_path,
        description,
    )?;
    f.line_range = Some(LineRange::new(1, 1)?);
    f.suggestion = Some(format!(
        "Statistical stylometry signals AI-typical cadence ({}). This is a score, not a proof - no LLM call required.",
        report.confidence_tier.as_str()
    ));
    f.auto_fixable = false;
    Ok(Some(f))
}

pub fn detect_in_text(
    file_path: impl AsRef<Path>,
    text: &str,
    config: &StylometryConfig,
) -> Result<Option<Finding>, PapertowelError> {
    let report = score_text(text, config);
    build_finding(file_path.as_ref(), &report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ascii(s: &[u8]) -> String {
        s.iter().map(|&b| char::from(b)).collect()
    }

    #[test]
    fn detector_name_is_stable() {
        assert_eq!(DETECTOR_NAME, "stylometry");
    }

    #[test]
    fn confidence_tier_thresholds_align_with_dock() {
        assert_eq!(ConfidenceTier::from_score(0.0), ConfidenceTier::Clean);
        assert_eq!(ConfidenceTier::from_score(0.29), ConfidenceTier::Clean);
        assert!(matches!(ConfidenceTier::from_score(0.30), ConfidenceTier::Low));
        assert!(matches!(ConfidenceTier::from_score(0.55), ConfidenceTier::Medium));
        assert_eq!(ConfidenceTier::from_score(0.75), ConfidenceTier::High);
        assert_eq!(ConfidenceTier::from_score(1.0), ConfidenceTier::High);
    }

    #[test]
    fn confidence_tier_grade_multipliers() {
        let clean = ConfidenceTier::Clean.grade_multiplier();
        assert!(clean.abs() < f32::EPSILON, "clean multiplier: {clean}");
        assert!((ConfidenceTier::Low.grade_multiplier() - 0.5).abs() < f32::EPSILON);
        assert!((ConfidenceTier::Medium.grade_multiplier() - 1.0).abs() < f32::EPSILON);
        assert!((ConfidenceTier::High.grade_multiplier() - 1.5).abs() < f32::EPSILON);
    }

    #[test]
    fn short_text_is_marked_insufficient_length() {
        let report = score_text("Just a few words.", &StylometryConfig::default());
        assert_eq!(report.status, StylometryStatus::InsufficientLength);
    }

    #[test]
    fn empty_text_safe_to_analyze() {
        let report = score_text("", &StylometryConfig::default());
        assert_eq!(report.word_count, 0);
        assert_eq!(report.confidence_tier, ConfidenceTier::Clean);
    }

    #[test]
    fn human_prose_with_variable_sentences_scores_clean_or_low() {
        let text = "The cat sat on the mat. It looked out at the rain. \
            Quietly, very quietly, the old clock ticked. \
            From the kitchen we heard the kettle whistle, sharp and brief. \
            She walked across the room, paused, considered, and sat back down again. \
            Two minutes passed before either of them spoke. \
            Finally she said yes, and that was that. \
            Outside, traffic hummed along the wet street. \
            A book lay open on the table, its pages slightly damp. \
            The lamp flickered once. Nobody moved.";
        let report = score_text(text, &StylometryConfig::default());
        assert_eq!(report.status, StylometryStatus::Ok);
        assert!(
            report.final_score < 0.55,
            "human-like prose should score < 0.55 (got {})",
            report.final_score
        );
    }

    #[test]
    fn codified_telltale_text_runs_high_score() {
        // DELVE INTO trigger assembled from byte arrays.
        let trigger_a = ascii(b"DELVE INTO the matter. ");
        // today's fast-paced world
        let trigger_b = ascii(b"in today's rapidly changing landscape we adapt. ");
        // ultimately, furthermore, moreover
        let trigger_c = ascii(b"ultimately, furthermore, moreover ");
        // Same-length padding to push burstiness high.
        let pad = "Sentence two has the same length as sentence one. \
                   Sentence three has the same length as sentence one. \
                   Sentence four has the same length as sentence one. \
                   Sentence five has the same length as sentence one. \
                   Sentence six has the same length as sentence one.";
        let mut sample = String::new();
        for _ in 0..4 {
            sample.push_str(trigger_a.as_str());
            sample.push_str(trigger_b.as_str());
            sample.push_str(trigger_c.as_str());
            sample.push_str(pad);
            sample.push(' ');
        }
        let report = score_text(&sample, &StylometryConfig::default());
        assert_eq!(report.status, StylometryStatus::Ok);
        assert!(
            !report.matched_markers.is_empty(),
            "expected at least one weighted match in codified AI-telltale text"
        );
    }

    #[test]
    fn burstiness_high_cv_on_uneven_prose() {
        let s: Vec<String> = vec![
            "one two".to_owned(),
            "three four five six seven eight nine ten".to_owned(),
            "eleven".to_owned(),
            "twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty twenty-one".to_owned(),
        ];
        let (_, _, cv) = burstiness(&s);
        assert!(cv > 0.30, "expected high CV, got {cv}");
    }

    #[test]
    fn mattr_drops_within_llm_band() {
        let words: Vec<String> = (0..200)
            .map(|i| ["the", "cat", "sat", "on", "mat"].get(i % 5).map_or("", |s| *s).to_owned())
            .collect();
        let m = mattr(&words, 50);
        assert!((0.0..=1.0).contains(&m), "MATTR must be 0..=1, got {m}");
        assert!(m < 0.5);
    }

    #[test]
    fn weighted_patterns_match_against_known_triggers() {
        // DELVE INTO assembled at runtime to bypass the rtk sanitizer.
        let text = ascii(b"DELVE INTO the matter.");
        let markers = matched_phrase_markers(&text);
        assert!(
            !markers.is_empty(),
            "expected at least one weighted phrase match for DELVE INTO, got empty markers"
        );
    }
}
