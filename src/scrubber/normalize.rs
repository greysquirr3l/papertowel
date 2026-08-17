//! NFKC normalization + homoglyph detection.
//!
//! Two related transformations for input text. NFKC normalization
//! decomposes compatibility-form codepoints (fullwidth Latin,
//! ligatures, superscripts, subscripts) and recomposes them into
//! canonical form, so downstream detectors can deduplicate the
//! canonical text against homoglyph variants.
//!
//! Homoglyph detection identifies codepoints in two scripts that
//! are visually identical (Cyrillic `a` vs Latin `a`,
//! fullwidth `U+FF41` vs Latin `a`). Emits a `Finding` per
//! homoglyph cluster.
//!
//! ## Upstream mapping
//!
//! [`watermarks-remover` `service/scripts/text_unicode.py:normalize_text`](https://github.com/guillaumemeyer/watermarks-remover/blob/main/service/scripts/text_unicode.py)
//!
//! ## Honesty model
//!
//! tri-state honesty framing - a homoglyph cluster is a byte-level
//! observation. The detector surfaces the bytes; the user interprets.

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

use crate::detection::finding::{Finding, FindingCategory, LineRange, Severity};
use crate::domain::errors::PapertowelError;

pub const DETECTOR_NAME: &str = "normalize";

/// Homoglyph table mapping Cyrillic / Greek / fullwidth Latin
/// confusables to their Latin canonical form. Key is the visual-
/// equivalent codepoint; value is the Latin codepoint we map to.
///
/// Limited to common confusables - not a full table.
pub const HOMOGLYPH_PAIRS: &[(u32, u32)] = &[
    // Cyrillic -> Latin
    (0x0430, 0x0061), // a
    (0x0435, 0x0065), // e
    (0x043E, 0x006F), // o
    (0x0440, 0x0070), // p
    (0x0441, 0x0063), // c
    (0x0445, 0x0078), // x
    (0x0443, 0x0079), // y (visual confusable in some fonts)
    (0x0410, 0x0041), // A
    (0x0412, 0x0042), // B (Cyrillic Ve)
    (0x0415, 0x0045), // E
    (0x041E, 0x004F), // O
    (0x0420, 0x0050), // P
    (0x0421, 0x0043), // C
    (0x0425, 0x0058), // X
    // Greek -> Latin
    (0x03B1, 0x0061), // alpha
    (0x03BF, 0x006F), // omicron
    (0x03C1, 0x0070), // rho
    (0x03BD, 0x0076), // nu (sometimes confused with v)
    // Fullwidth Latin -> Latin (U+FF41..=U+FF5E)
    (0xFF41, 0x0061),
    (0xFF42, 0x0062),
    (0xFF43, 0x0063),
    (0xFF44, 0x0064),
    (0xFF45, 0x0065),
    (0xFF46, 0x0066),
    (0xFF47, 0x0067),
    (0xFF48, 0x0068),
    (0xFF49, 0x0069),
    (0xFF4A, 0x006A),
    (0xFF4B, 0x006B),
    (0xFF4C, 0x006C),
    (0xFF4D, 0x006D),
    (0xFF4E, 0x006E),
    (0xFF4F, 0x006F),
    (0xFF50, 0x0070),
    (0xFF51, 0x0071),
    (0xFF52, 0x0072),
    (0xFF53, 0x0073),
    (0xFF54, 0x0074),
    (0xFF55, 0x0075),
    (0xFF56, 0x0076),
    (0xFF57, 0x0077),
    (0xFF58, 0x0078),
    (0xFF59, 0x0079),
    (0xFF5A, 0x007A),
];

/// Normalization form to apply.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NormalizationForm {
    #[default]
    Nfkc,
    Nfc,
    Nfd,
    Nfkd,
}

/// Configuration for the normalization detector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NormalizeConfig {
    pub form: NormalizationForm,
    pub detect_homoglyphs: bool,
    pub min_homoglyph_chars: usize,
}

impl Default for NormalizeConfig {
    fn default() -> Self {
        Self {
            form: NormalizationForm::Nfkc,
            detect_homoglyphs: true,
            min_homoglyph_chars: 1,
        }
    }
}

/// NFKC-normalize `text`.
#[must_use]
pub fn normalize_text(text: &str, form: NormalizationForm) -> String {
    match form {
        NormalizationForm::Nfc => text.nfc().collect::<String>(),
        NormalizationForm::Nfd => text.nfd().collect::<String>(),
        NormalizationForm::Nfkd => text.nfkd().collect::<String>(),
        NormalizationForm::Nfkc => text.nfkc().collect::<String>(),
    }
}

/// Check whether `c` is in the homoglyph table.
#[must_use]
pub fn canonical_of(c: char) -> Option<char> {
    let cp = c as u32;
    HOMOGLYPH_PAIRS.iter().find_map(|(from, to)| {
        if *from == cp {
            char::from_u32(*to)
        } else {
            None
        }
    })
}

/// Walk `text` and find all homoglyph clusters, grouped by canonical
/// Latin codepoint and contiguous run.
///
/// Cluster boundaries are *characters*, not lines: a homoglyph run
/// ends at the first non-homoglyph char (including `\n`, which is not
/// in the homoglyph table). The `line` field tracks the line where
/// each cluster starts; multi-line clusters are not produced.
#[must_use]
pub fn find_homoglyph_clusters(text: &str) -> Vec<HomoglyphCluster> {
    let mut clusters: Vec<HomoglyphCluster> = Vec::new();
    let mut chars = text.chars().peekable();
    let mut line = 1_usize;

    while let Some(c) = chars.next() {
        if c == '\n' {
            line += 1;
            continue;
        }
        let Some(canonical) = canonical_of(c) else { continue };
        let mut sources: BTreeSet<u32> = BTreeSet::new();
        sources.insert(c as u32);
        let start_line = line;
        while let Some(&nxt) = chars.peek() {
            if canonical_of(nxt).is_some() {
                sources.insert(nxt as u32);
                chars.next();
            } else {
                break;
            }
        }
        clusters.push(HomoglyphCluster {
            canonical: canonical as u32,
            sources,
            start_line,
            end_line: start_line,
        });
        if line != start_line {
            // We never merged across newlines (by construction), but if
            // this changes in the future, end_line needs to track.
            // This branch is intentionally unreachable today; keeping
            // the field avoids a `#[dead_code]` if the cluster span
            // ever loosens.
        }
    }
    clusters
}

/// A contiguous run of homoglyph codepoints that share a canonical Latin
/// counterpart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomoglyphCluster {
    pub canonical: u32,
    pub sources: BTreeSet<u32>,
    pub start_line: usize,
    pub end_line: usize,
}

/// Build a Finding from a homoglyph cluster.
pub fn build_finding(
    file_path: &Path,
    cluster: &HomoglyphCluster,
) -> Result<Finding, PapertowelError> {
    let mut sources_list: Vec<u32> = cluster.sources.iter().copied().collect();
    sources_list.sort_unstable();
    let hex_list: Vec<String> = sources_list
        .iter()
        .map(|cp| format!("U+{cp:04X}"))
        .collect();

    let id = format!("NORMALIZE-HOMOGLYPH-{:04X}", cluster.canonical);
    let description = format!(
        "homoglyph cluster: canonical U+{:04X}, sources [{}] (lines {}-{})",
        cluster.canonical,
        hex_list.join(", "),
        cluster.start_line,
        cluster.end_line,
    );
    let mut finding = Finding::new(
        id,
        FindingCategory::Structure,
        Severity::High,
        0.85,
        file_path,
        description,
    )?;
    finding.line_range = Some(LineRange::new(cluster.start_line, cluster.end_line)?);
    finding.suggestion = Some(format!(
        "Replace the homoglyph codepoints with U+{:04X} or run `papertowel scrub --normalize` to NFKC-normalize the file.",
        cluster.canonical,
    ));
    finding.auto_fixable = false;
    Ok(finding)
}

/// Run normalization + homoglyph detection on `text` and produce a
/// list of Findings (one per homoglyph cluster).
pub fn detect_in_text(
    file_path: impl AsRef<Path>,
    text: &str,
    config: &NormalizeConfig,
) -> Result<Vec<Finding>, PapertowelError> {
    if !config.detect_homoglyphs {
        return Ok(Vec::new());
    }
    let clusters = find_homoglyph_clusters(text);
    let reportable: Vec<&HomoglyphCluster> = clusters
        .iter()
        .filter(|c| c.sources.len() >= config.min_homoglyph_chars.max(1))
        .collect();
    if reportable.is_empty() {
        return Ok(Vec::new());
    }
    let mut findings = Vec::with_capacity(reportable.len());
    for c in reportable {
        findings.push(build_finding(file_path.as_ref(), c)?);
    }
    Ok(findings)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::expect_used,
        reason = "test fixtures use known-valid Unicode codepoints"
    )]
    use super::*;
    #[test]
    fn detector_name_is_stable() {
        assert_eq!(DETECTOR_NAME, "normalize");
    }

    #[test]
    fn nfkc_normalizes_fullwidth_latin() {
        // "Hello" in fullwidth letters.
        let text = "\u{FF28}\u{FF45}\u{FF4C}\u{FF4C}\u{FF4F}";
        let normalized = normalize_text(text, NormalizationForm::Nfkc);
        assert_eq!(normalized, "Hello");
    }

    #[test]
    fn nfkc_collapses_ligatures() {
        // "fi" ligature decomposes under NFKC to "fi".
        let text = "\u{FB01}nd";
        let normalized = normalize_text(text, NormalizationForm::Nfkc);
        assert_eq!(normalized, "find");
    }

    #[test]
    fn nfkc_preserves_ascii() {
        let text = "fn main() {\n    println!(\"hi\");\n}\n";
        let normalized = normalize_text(text, NormalizationForm::Nfkc);
        assert_eq!(normalized, text);
    }

    #[test]
    fn canonical_of_maps_cyrillic_a() {
        let cyrillic_a = char::from_u32(0x0430).expect("0x0430 is a valid char");
        let canon = canonical_of(cyrillic_a).expect("Cyrillic a is in the table");
        assert_eq!(canon, 'a');
    }

    #[test]
    fn canonical_of_returns_none_for_ascii() {
        assert!(canonical_of('z').is_none());
        assert!(canonical_of('A').is_none());
    }

    #[test]
    fn find_homoglyph_clusters_detects_mixed_script() -> Result<(), Box<dyn std::error::Error>> {
        // Cyrillic a is the homoglyph; Latin a is canonical. The
        // cluster groups the Cyrillic source(s) only.
        let text = "p\u{0430}lindromic";
        let clusters = find_homoglyph_clusters(text);
        let cluster = clusters.first().ok_or("expected at least one cluster")?;
        assert_eq!(cluster.canonical, 0x0061);
        assert!(cluster.sources.contains(&0x0430));
        Ok(())
    }

    #[test]
    fn homoglyph_cluster_with_fullwidth_chars() {
        // 3 fullwidth chars in a row
        let text = "\u{FF41}\u{FF42}\u{FF43}";
        let clusters = find_homoglyph_clusters(text);
        assert!(!clusters.is_empty());
        let total_sources: usize = clusters.iter().map(|c| c.sources.len()).sum();
        assert_eq!(total_sources, 3);
    }

    #[test]
    fn threshold_min_homoglyph_chars_suppresses_singles(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let text = "\u{0430}b";
        let cfg = NormalizeConfig {
            min_homoglyph_chars: 2,
            ..NormalizeConfig::default()
        };
        let findings = detect_in_text("single.rs", text, &cfg)?;
        assert!(findings.is_empty(), "single homoglyph should be suppressed");
        Ok(())
    }

    #[test]
    fn detect_in_text_emits_findings_for_clusters(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let text = "\u{0430}\u{0435}\u{0440}";
        let findings = detect_in_text("mixed.rs", text, &NormalizeConfig::default())?;
        assert!(!findings.is_empty());
        Ok(())
    }

    #[test]
    fn detect_in_text_disabled_returns_empty(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let text = "\u{0430}\u{0435}\u{0440}";
        let cfg = NormalizeConfig {
            detect_homoglyphs: false,
            ..NormalizeConfig::default()
        };
        let findings = detect_in_text("off.rs", text, &cfg)?;
        assert!(findings.is_empty());
        Ok(())
    }
}
