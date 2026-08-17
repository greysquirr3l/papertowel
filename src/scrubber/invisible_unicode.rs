//! Invisible Unicode detector.
//!
//! Identifies non-printing or visually-ambiguous Unicode codepoints commonly
//! introduced by AI-edit watermarking (ZWJ family, tag chars, variation
//! selectors) or as supply-chain homograph vectors (bidi controls,
//! private-use area codepoints, exotic spaces).
//!
//! Upstream mapping:
//! [`skills/remove-ai-marks/references/mark-classes.md`](https://github.com/guillaumemeyer/watermarks-remover/blob/main/skills/remove-ai-marks/references/mark-classes.md)
//! §1 "Edit-based text" + [`service/scripts/text_unicode.py`](https://github.com/guillaumemeyer/watermarks-remover/blob/main/service/scripts/text_unicode.py).
//!
//! Load-bearing invisibles are preserved by default to avoid corrupting
//! real text:
//!
//! - emoji ZWJ/VS sequences between emoji bases,
//! - flag-emoji tag sequences (regional-indicator × tag),
//! - script-internal Cf marks (Arabic/Syriac/Hebrew).
//!
//! A `--strip-emoji-glue` / `preserve_emoji_glue = false` override strips
//! them regardless.

use std::fs;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use unicode_general_category::{GeneralCategory, get_general_category};

use crate::detection::binary::{BINARY_SNIFF_BYTES, looks_binary_file};
use crate::detection::finding::{Finding, FindingCategory, LineRange, Severity};
use crate::domain::errors::PapertowelError;

pub const DETECTOR_NAME: &str = "invisible-unicode";

const DETECT_ID_PREFIX: &str = "INVISIBLE-UN";

/// Categories of invisible / visually-ambiguous Unicode characters scanned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvisibleKind {
    /// ZWSP, ZWNJ, ZWJ, WJ, BOM.
    ZwjFamily,
    /// Bidi directional / embedding controls (LRE/RLE/PDF/LRO/RLO/LRI/RLI/FSI/PDI).
    Bidi,
    /// U+E0001–U+E007F (tag characters; flag-emoji tag sequences included).
    TagChar,
    /// Variation Selectors VS1–VS256.
    VariationSelector,
    /// Private-Use Area codepoints (BMP + Supplementary PUA-A + PUA-B).
    PrivateUse,
    /// Exotic spaces (NBSP, em/figure/ideographic, Mongolian vowel separator, …).
    ExoticSpace,
}

impl InvisibleKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ZwjFamily => "zwj_family",
            Self::Bidi => "bidi",
            Self::TagChar => "tag_char",
            Self::VariationSelector => "variation_selector",
            Self::PrivateUse => "private_use",
            Self::ExoticSpace => "exotic_space",
        }
    }
}

/// Detector configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InvisibleUnicodeConfig {
    /// Minimum invisible count in a single file before emitting a finding.
    pub min_invisible_chars: usize,
    /// Preserve ZWJ/VS between emoji bases (default `true`).
    pub preserve_emoji_glue: bool,
    /// Preserve ZWNJ/ZWJ inside complex scripts (default `true`).
    pub preserve_script_joiners: bool,
    /// Preserve flag-emoji tag sequences (default `true`).
    pub preserve_tag_sequences: bool,
    /// Preserve script-internal Cf marks (Arabic/Syriac/Hebrew; default `true`).
    pub preserve_script_cf_marks: bool,
}

impl Default for InvisibleUnicodeConfig {
    fn default() -> Self {
        Self {
            min_invisible_chars: 1,
            preserve_emoji_glue: true,
            preserve_script_joiners: true,
            preserve_tag_sequences: true,
            preserve_script_cf_marks: true,
        }
    }
}

/// A single classified invisible-character occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvisibleMatch {
    /// Byte offset of the character in the source text.
    pub byte_offset: usize,
    /// 1-based line number.
    pub line: usize,
    /// Codepoint of the character.
    pub codepoint: u32,
    /// Classified kind.
    pub kind: InvisibleKind,
    /// True if a load-bearing carve-out marked this character as preserved.
    pub load_bearing: bool,
}

/// Classify a single Unicode codepoint into an `InvisibleKind`, or `None` if
/// it is not considered invisible by this detector.
#[must_use]
pub const fn classify_codepoint(codepoint: u32) -> Option<InvisibleKind> {
    // ZWJ family: ZWSP, ZWNJ, ZWJ, WJ, BOM
    if matches!(codepoint, 0x200B..=0x200D | 0x2060 | 0xFEFF) {
        return Some(InvisibleKind::ZwjFamily);
    }
    // Bidi embedding / override / isolate controls
    if matches!(codepoint, 0x202A..=0x202E | 0x2066..=0x2069) {
        return Some(InvisibleKind::Bidi);
    }
    // Tag characters (incl. flag-emoji tag sequences)
    if matches!(codepoint, 0xE0001..=0xE007F) {
        return Some(InvisibleKind::TagChar);
    }
    // Variation Selectors VS1–VS16 (BMP) + VS17–VS256 (SMP)
    if matches!(codepoint, 0xFE00..=0xFE0F | 0xE0100..=0xE01EF) {
        return Some(InvisibleKind::VariationSelector);
    }
    // Private Use Areas: BMP, PUA-A, PUA-B
    if matches!(
        codepoint,
        0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x0010_0000..=0x0010_FFFD
    ) {
        return Some(InvisibleKind::PrivateUse);
    }
    // Exotic spaces
    if matches!(
        codepoint,
        0x00A0           // NBSP
            | 0x2000..=0x200A // en/em/figure/thin/hair spaces, ZW-like
            | 0x2028..=0x2029 // LSEP, PSEP
            | 0x202F           // NNBSP
            | 0x205F           // MMSP
            | 0x3000           // ideographic space
    ) {
        return Some(InvisibleKind::ExoticSpace);
    }
    None
}

/// Heuristic predicate: is `c` plausibly an emoji base? Used for joiner/VS
/// carve-outs. The ranges below cover the bulk of emoji-bearing blocks; it
/// is intentionally conservative.
#[must_use]
pub const fn is_emoji_base(c: char) -> bool {
    let cp = c as u32;
    matches!(
        cp,
        0x1F000..=0x1FAFF
            | 0x2600..=0x27BF
            | 0xFE0F // emoji presentation VS-16
    )
}

/// Is `c` plausibly a script-internal Cf (Arabic/Syriac/Hebrew etc.)?
#[must_use]
pub fn is_script_internal_cf(c: char) -> bool {
    if get_general_category(c) != GeneralCategory::Format {
        return false;
    }
    let cp = c as u32;
    // Hebrew / Arabic / Syriac blocks + Arabic Supplement + Arabic Extended.
    matches!(cp, 0x0590..=0x05FF | 0x0600..=0x06FF | 0x0700..=0x077F | 0x08A0..=0x08FF)
}

/// Detects regional-indicator adjacency for flag-tag preservation.
const fn is_regional_indicator(c: char) -> bool {
    let cp = c as u32;
    matches!(cp, 0x1F1E6..=0x1F1FF)
}

/// Decide whether a match should be preserved as load-bearing text.
///
/// `kind`: the classified kind of the character (used to gate carve-outs).
/// `c`: the literal character (used to apply format-specific heuristics).
/// `prev` / `next`: adjacent characters in the text, if any.
fn is_load_bearing(
    kind: InvisibleKind,
    c: char,
    prev: Option<char>,
    next: Option<char>,
    config: &InvisibleUnicodeConfig,
) -> bool {
    match kind {
        InvisibleKind::ZwjFamily => {
            if config.preserve_emoji_glue
                && matches!(c, '\u{200C}' | '\u{200D}')
                && (prev.is_some_and(is_emoji_base) || next.is_some_and(is_emoji_base))
            {
                return true;
            }
            if config.preserve_script_joiners
                && matches!(c, '\u{200C}' | '\u{200D}')
                && (prev.is_some_and(is_script_internal_cf) || next.is_some_and(is_script_internal_cf))
            {
                return true;
            }
            false
        }
        InvisibleKind::VariationSelector => {
            // VS1–VS16 attached to emoji presentation (or after regional indicator).
            config.preserve_emoji_glue
                && prev.is_some_and(|p| {
                    is_emoji_base(p) || is_regional_indicator(p) || matches!(p, '\u{FE0F}' | '\u{200D}')
                })
        }
        InvisibleKind::TagChar => {
            // Tag sequences: regional-indicator × N followed by tag × N.
            config.preserve_tag_sequences
                && (prev.is_some_and(is_regional_indicator)
                    || prev.is_some_and(|p| classify_codepoint(p as u32) == Some(InvisibleKind::TagChar)))
        }
        InvisibleKind::Bidi => config.preserve_script_cf_marks
            && (prev.is_some_and(is_script_internal_cf) || next.is_some_and(is_script_internal_cf)),
        InvisibleKind::PrivateUse | InvisibleKind::ExoticSpace => false,
    }
}

/// Walk the text once, returning classified matches per character. Line
/// numbers are 1-based. Carve-outs are applied as configured.
#[must_use]
pub fn find_invisibles(text: &str, config: &InvisibleUnicodeConfig) -> Vec<InvisibleMatch> {
    let mut out = Vec::new();
    let mut line = 1_usize;
    let mut prev: Option<char> = None;

    // Collect (byte_offset, char) pairs to enable neighbor lookups safely.
    let mut iter = text.char_indices().peekable();
    while let Some((offset, c)) = iter.next() {
        if let Some(kind) = classify_codepoint(c as u32) {
            let next = iter.peek().map(|(_, n)| *n);
            let load_bearing = is_load_bearing(kind, c, prev, next, config);
            out.push(InvisibleMatch {
                byte_offset: offset,
                line,
                codepoint: c as u32,
                kind,
                load_bearing,
            });
        }
        if c == '\n' {
            line += 1;
        }
        prev = Some(c);
    }
    out
}

/// Convert raw matches into `Finding` objects, suppressing load-bearing
/// carve-outs and emitting only when at least `min_invisible_chars`
/// are present.
fn build_findings(
    file_path: &Path,
    matches: &[InvisibleMatch],
    config: &InvisibleUnicodeConfig,
) -> Result<Vec<Finding>, PapertowelError> {
    const MAX_FINDINGS_PER_FILE: usize = 50;

    let reportable = matches.iter().filter(|m| !m.load_bearing).count();
    if reportable < config.min_invisible_chars {
        return Ok(Vec::new());
    }

    let mut findings = Vec::with_capacity(reportable.min(MAX_FINDINGS_PER_FILE));
    for (idx, m) in matches
        .iter()
        .filter(|m| !m.load_bearing)
        .take(MAX_FINDINGS_PER_FILE)
        .enumerate()
    {
        let id = format!("{DETECT_ID_PREFIX}-{:04}", idx + 1);
        let mut finding = Finding::new(
            id,
            FindingCategory::InvisibleUnicode,
            Severity::High,
            0.92_f32,
            file_path,
            format!(
                "{} character U+{:04X} on line {} (byte offset {})",
                m.kind.as_str(),
                m.codepoint,
                m.line,
                m.byte_offset,
            ),
        )?;

        finding.line_range = Some(LineRange::new(m.line, m.line)?);
        finding.suggestion = Some(format!(
            "Remove the {} character (U+{:04X}); it is visually invisible and likely AI-edit or homograph cruft.",
            m.kind.as_str(),
            m.codepoint,
        ));
        finding.auto_fixable = false;

        findings.push(finding);
    }

    Ok(findings)
}

/// Detect invisibles in the given text content for a file path.
pub fn detect_in_text(
    file_path: impl Into<PathBuf>,
    text: &str,
    config: &InvisibleUnicodeConfig,
) -> Result<Vec<Finding>, PapertowelError> {
    let file_path = file_path.into();
    let matches = find_invisibles(text, config);
    if matches.is_empty() {
        return Ok(Vec::new());
    }
    build_findings(&file_path, &matches, config)
}

/// Detect invisibles in a single file.
pub fn detect_file(
    file_path: impl AsRef<Path>,
    config: &InvisibleUnicodeConfig,
) -> Result<Vec<Finding>, PapertowelError> {
    let path = file_path.as_ref();
    let content = fs::read_to_string(path)
        .map_err(|error| PapertowelError::io_with_path(path, error))?;
    detect_in_text(path, &content, config)
}

/// Walk a repository and run the detector on each text file.
///
/// Skips build / vendored directories (`target/`, `node_modules/`,
/// `.git/`, `vendor/`, `dist/`, `build/`) and binary-looking files.
///
/// Only the first `BINARY_SNIFF_BYTES` are read for the binary sniff
/// to avoid pulling multi-MiB assets into memory just to skip them.
pub fn detect_repo(
    repo_root: impl AsRef<Path>,
    config: &InvisibleUnicodeConfig,
) -> Result<Vec<Finding>, PapertowelError> {
    const SKIP_DIRS: [&str; 6] = [
        "target", "node_modules", ".git", "vendor", "dist", "build",
    ];

    let mut findings = Vec::new();
    let walker = WalkBuilder::new(repo_root.as_ref())
        .standard_filters(true)
        .build();

    for entry in walker.filter_map(Result::ok) {
        let path = entry.path();
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        if path.components().any(|c| {
            c.as_os_str()
                .to_str()
                .is_some_and(|s| SKIP_DIRS.contains(&s))
        }) {
            continue;
        }
        // Binary sniff on the head only.
        let Ok(head) = looks_binary_file(path, BINARY_SNIFF_BYTES) else {
            continue;
        };
        if head {
            continue;
        }
        // Full read only after the sniff decides "text".
        let Ok(content) = fs::read_to_string(path) else { continue; };
        let local = detect_in_text(path, &content, config)?;
        findings.extend(local);
    }
    Ok(findings)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::indexing_slicing,
        reason = "test assertions on known-populated vecs"
    )]

    use super::*;
    use std::fs;

    use tempfile::TempDir;

    #[test]
    fn detector_name_is_stable() {
        assert_eq!(DETECTOR_NAME, "invisible-unicode");
    }

    #[test]
    fn invisible_kind_labels_are_snake_case() {
        assert_eq!(InvisibleKind::ZwjFamily.as_str(), "zwj_family");
        assert_eq!(InvisibleKind::VariationSelector.as_str(), "variation_selector");
    }

    #[test]
    fn classifier_identifies_all_six_kinds() {
        assert_eq!(classify_codepoint(0x200B), Some(InvisibleKind::ZwjFamily)); // ZWSP
        assert_eq!(classify_codepoint(0x200D), Some(InvisibleKind::ZwjFamily)); // ZWJ
        assert_eq!(classify_codepoint(0xFEFF), Some(InvisibleKind::ZwjFamily)); // BOM
        assert_eq!(classify_codepoint(0x202E), Some(InvisibleKind::Bidi)); // RLO
        assert_eq!(classify_codepoint(0x2069), Some(InvisibleKind::Bidi)); // PDI
        assert_eq!(classify_codepoint(0xE0041), Some(InvisibleKind::TagChar)); // TAG LATIN CAPITAL LETTER A
        assert_eq!(classify_codepoint(0xFE0F), Some(InvisibleKind::VariationSelector));
        assert_eq!(classify_codepoint(0xE01EF), Some(InvisibleKind::VariationSelector));
        assert_eq!(classify_codepoint(0xE001), Some(InvisibleKind::PrivateUse));
        assert_eq!(classify_codepoint(0xF8FF), Some(InvisibleKind::PrivateUse));
        assert_eq!(classify_codepoint(0x00A0), Some(InvisibleKind::ExoticSpace)); // NBSP
        assert_eq!(classify_codepoint(0x3000), Some(InvisibleKind::ExoticSpace)); // IDSP
    }

    #[test]
    fn classifier_rejects_printable_ascii() {
        for cp in b'A'..=b'z' {
            assert!(classify_codepoint(u32::from(cp)).is_none());
        }
    }

    #[test]
    fn ascii_text_yields_no_findings() -> Result<(), Box<dyn std::error::Error>> {
        let findings = detect_in_text(
            "ascii.rs",
            "fn main() { println!(\"hello\"); }\n",
            &InvisibleUnicodeConfig::default(),
        )?;
        assert!(findings.is_empty());
        Ok(())
    }

    #[test]
    fn detects_zwsp_in_text() -> Result<(), Box<dyn std::error::Error>> {
        let text = "first\u{200B}second\n";
        let findings = detect_in_text("carrier.txt", text, &InvisibleUnicodeConfig::default())?;
        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        assert_eq!(f.severity, Severity::High);
        assert_eq!(f.category, FindingCategory::InvisibleUnicode);
        assert!(f.description.contains("zwj_family"));
        assert!(f.description.contains("U+200B"));
        Ok(())
    }

    #[test]
    fn zwj_between_ascii_neighbors_is_not_load_bearing() {
        // ZWJ between two ASCII spaces — not an emoji context.
        let zwj = '\u{200D}';
        let text = format!("first {zwj} second");
        let matches = find_invisibles(&text, &InvisibleUnicodeConfig::default());
        let zwj = matches.iter().find(|m| m.codepoint == 0x200D);
        assert!(zwj.is_some(), "ZWJ should be classified");
        assert!(
            !zwj.is_some_and(|m| m.load_bearing),
            "ZWJ between ASCII neighbors should not be load-bearing",
        );
    }

    #[test]
    fn zwj_between_emoji_bases_is_load_bearing() {
        // 🚀 (U+1F680) + ZWJ + 🔥 (U+1F525)
        let text = "\u{1F680}\u{200D}\u{1F525}";
        let matches = find_invisibles(text, &InvisibleUnicodeConfig::default());
        let zwj = matches.iter().find(|m| m.codepoint == 0x200D);
        assert!(zwj.is_some_and(|m| m.load_bearing), "ZWJ between emoji must be load-bearing");
    }

    #[test]
    fn detects_bidi_control_in_text() -> Result<(), Box<dyn std::error::Error>> {
        let text = "innocent\u{202E}evil.exe\n";
        let findings = detect_in_text("tricky.rs", text, &InvisibleUnicodeConfig::default())?;
        assert_eq!(findings.len(), 1);
        assert!(findings[0].description.contains("bidi"));
        Ok(())
    }

    #[test]
    fn detects_tag_char_in_text() -> Result<(), Box<dyn std::error::Error>> {
        let text = "hidden\u{E0041}tag\n";
        let findings = detect_in_text("flag.txt", text, &InvisibleUnicodeConfig::default())?;
        assert!(!findings.is_empty());
        Ok(())
    }

    #[test]
    fn detects_nbsp_in_text() -> Result<(), Box<dyn std::error::Error>> {
        let text = "name\u{00A0}=\"value\"\n";
        let findings = detect_in_text("props.rs", text, &InvisibleUnicodeConfig::default())?;
        assert_eq!(findings.len(), 1);
        assert!(findings[0].description.contains("exotic_space"));
        Ok(())
    }

    #[test]
    fn detects_repo_walks_text_files_and_skips_build_dirs() -> Result<(), Box<dyn std::error::Error>> {
        let temp = TempDir::new()?;
        // A source file with a ZWSP — should produce a finding
        fs::write(temp.path().join("lib.rs"), "x\u{200B}y\n")?;
        // A build artifact we'd want skipped
        fs::create_dir(temp.path().join("target"))?;
        fs::write(temp.path().join("target").join("garbage.rs"), "x\u{200B}y\n")?;

        let findings = detect_repo(temp.path(), &InvisibleUnicodeConfig::default())?;
        // Only `lib.rs` should appear; `target/` is skipped.
        assert_eq!(findings.len(), 1);
        assert!(findings[0].file_path.ends_with("lib.rs"));
        Ok(())
    }

    #[test]
    fn min_invisible_chars_threshold_emits_nothing_below_threshold(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let text = "x\u{200B}y\n";
        let cfg = InvisibleUnicodeConfig {
            min_invisible_chars: 5,
            ..InvisibleUnicodeConfig::default()
        };
        let findings = detect_in_text("one_zwsp.txt", text, &cfg)?;
        assert!(findings.is_empty());
        Ok(())
    }
}
