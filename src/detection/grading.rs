//! Letter-grade scoring for AI fingerprint detection.
//!
//! Inspired by [vibescore](https://github.com/stef41/vibescore), this module
//! provides a holistic "Slop Score" that grades projects on how clean they are
//! from AI-generated code fingerprints.
//!
//! Lower slop = better grade (A+ means nearly fingerprint-free).

use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::detection::finding::{Finding, FindingCategory, Severity};

/// Letter grade from A+ (best) to F (worst).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Grade {
    APlus,
    A,
    AMinus,
    BPlus,
    B,
    BMinus,
    CPlus,
    C,
    CMinus,
    DPlus,
    D,
    DMinus,
    F,
}

impl Grade {
    /// Convert a slop score (0-100, lower is better) to a letter grade.
    #[must_use]
    pub fn from_slop_score(score: f32) -> Self {
        // Inverted scale: lower slop = better grade
        match score {
            s if s <= 3.0 => Self::APlus,
            s if s <= 7.0 => Self::A,
            s if s <= 10.0 => Self::AMinus,
            s if s <= 13.0 => Self::BPlus,
            s if s <= 17.0 => Self::B,
            s if s <= 20.0 => Self::BMinus,
            s if s <= 23.0 => Self::CPlus,
            s if s <= 27.0 => Self::C,
            s if s <= 30.0 => Self::CMinus,
            s if s <= 33.0 => Self::DPlus,
            s if s <= 37.0 => Self::D,
            s if s <= 40.0 => Self::DMinus,
            _ => Self::F,
        }
    }

    /// Check if this grade meets a minimum threshold.
    #[must_use]
    pub fn meets_minimum(self, minimum: Self) -> bool {
        self <= minimum // Lower enum variant = better grade
    }

    /// ANSI color code for terminal output.
    #[must_use]
    pub const fn ansi_color(&self) -> &'static str {
        match self {
            Self::APlus | Self::A | Self::AMinus => "\x1b[32m", // Green
            Self::BPlus | Self::B | Self::BMinus => "\x1b[36m", // Cyan
            Self::CPlus | Self::C | Self::CMinus => "\x1b[33m", // Yellow
            Self::DPlus | Self::D | Self::DMinus => "\x1b[91m", // Light red
            Self::F => "\x1b[31m",                              // Red
        }
    }
}

impl fmt::Display for Grade {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::APlus => "A+",
            Self::A => "A",
            Self::AMinus => "A-",
            Self::BPlus => "B+",
            Self::B => "B",
            Self::BMinus => "B-",
            Self::CPlus => "C+",
            Self::C => "C",
            Self::CMinus => "C-",
            Self::DPlus => "D+",
            Self::D => "D",
            Self::DMinus => "D-",
            Self::F => "F",
        };
        write!(f, "{s}")
    }
}

impl std::str::FromStr for Grade {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "A+" => Ok(Self::APlus),
            "A" => Ok(Self::A),
            "A-" => Ok(Self::AMinus),
            "B+" => Ok(Self::BPlus),
            "B" => Ok(Self::B),
            "B-" => Ok(Self::BMinus),
            "C+" => Ok(Self::CPlus),
            "C" => Ok(Self::C),
            "C-" => Ok(Self::CMinus),
            "D+" => Ok(Self::DPlus),
            "D" => Ok(Self::D),
            "D-" => Ok(Self::DMinus),
            "F" => Ok(Self::F),
            _ => Err(format!("invalid grade: {s}")),
        }
    }
}

/// Slop score for a single category.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryScore {
    pub category: GradeCategory,
    pub raw_score: f32,
    pub normalized_score: f32,
    pub grade: Grade,
    pub finding_count: usize,
}

/// Categories for grading (mapped from `FindingCategory`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GradeCategory {
    Lexical,
    Comments,
    Structure,
    Architecture,
    Metadata,
    Testing,
    Workflow,
    History,
}

impl GradeCategory {
    /// Weight for overall score calculation.
    #[must_use]
    pub const fn weight(&self) -> f32 {
        match self {
            Self::Lexical | Self::Architecture => 0.20,
            Self::Comments | Self::Structure => 0.15,
            Self::Metadata | Self::Testing | Self::History => 0.10,
            Self::Workflow => 0.05,
        }
    }

    /// Human-readable label.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Lexical => "Lexical (slop words)",
            Self::Comments => "Comments",
            Self::Structure => "Structure",
            Self::Architecture => "Architecture",
            Self::Metadata => "Metadata",
            Self::Testing => "Testing",
            Self::Workflow => "Workflow",
            Self::History => "History",
        }
    }
}

impl fmt::Display for GradeCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// Map `FindingCategory` to `GradeCategory`.
impl From<FindingCategory> for GradeCategory {
    fn from(cat: FindingCategory) -> Self {
        match cat {
            FindingCategory::Lexical => Self::Lexical,
            FindingCategory::Comment => Self::Comments,
            FindingCategory::Structure
            | FindingCategory::IdiomMismatch
            | FindingCategory::PromptLeakage => Self::Structure,
            FindingCategory::Architecture => Self::Architecture,
            FindingCategory::Readme
            | FindingCategory::Metadata
            | FindingCategory::Promotion
            | FindingCategory::NameCredibility => Self::Metadata,
            FindingCategory::TestPattern => Self::Testing,
            FindingCategory::Workflow | FindingCategory::Maintenance => Self::Workflow,
            FindingCategory::CommitPattern => Self::History,
        }
    }
}

/// Complete grade report for a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GradeReport {
    pub overall_score: f32,
    pub overall_grade: Grade,
    pub categories: Vec<CategoryScore>,
    pub total_findings: usize,
    pub files_scanned: usize,
    pub scan_duration_ms: u64,
}

impl GradeReport {
    /// Build a grade report from scan findings.
    #[must_use]
    pub fn from_findings(findings: &[Finding], files_scanned: usize, scan_duration_ms: u64) -> Self {
        let mut category_findings: HashMap<GradeCategory, Vec<&Finding>> = HashMap::new();

        for f in findings {
            let cat = GradeCategory::from(f.category);
            category_findings.entry(cat).or_default().push(f);
        }

        let mut categories = Vec::new();
        let mut weighted_sum = 0.0_f32;
        let mut total_weight = 0.0_f32;

        for grade_cat in [
            GradeCategory::Lexical,
            GradeCategory::Comments,
            GradeCategory::Structure,
            GradeCategory::Architecture,
            GradeCategory::Metadata,
            GradeCategory::Testing,
            GradeCategory::Workflow,
            GradeCategory::History,
        ] {
            let cat_findings = category_findings.get(&grade_cat);
            let finding_count = cat_findings.map_or(0, Vec::len);

            // Calculate raw score based on severity-weighted finding count
            let raw_score = cat_findings.map_or(0.0, |fs| {
                fs.iter()
                    .map(|f| {
                        let severity_weight = match f.severity {
                            Severity::High => 3.0,
                            Severity::Medium => 1.5,
                            Severity::Low => 0.5,
                        };
                        severity_weight * f.confidence_score
                    })
                    .sum::<f32>()
            });

            // Normalize to 0-100 scale (cap at 100)
            // Scale factor: ~10 high-severity findings = score of 100
            let normalized_score = (raw_score * 3.33).min(100.0);
            let grade = Grade::from_slop_score(normalized_score);

            if finding_count > 0 {
                weighted_sum += normalized_score * grade_cat.weight();
                total_weight += grade_cat.weight();
            }

            categories.push(CategoryScore {
                category: grade_cat,
                raw_score,
                normalized_score,
                grade,
                finding_count,
            });
        }

        // Overall score is weighted average of categories with findings
        let overall_score = if total_weight > 0.0 {
            weighted_sum / total_weight
        } else {
            0.0 // No findings = perfect score
        };

        Self {
            overall_score,
            overall_grade: Grade::from_slop_score(overall_score),
            categories,
            total_findings: findings.len(),
            files_scanned,
            scan_duration_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grade_from_slop_score_boundaries() {
        assert_eq!(Grade::from_slop_score(0.0), Grade::APlus);
        assert_eq!(Grade::from_slop_score(3.0), Grade::APlus);
        assert_eq!(Grade::from_slop_score(3.1), Grade::A);
        assert_eq!(Grade::from_slop_score(10.0), Grade::AMinus);
        assert_eq!(Grade::from_slop_score(20.0), Grade::BMinus);
        assert_eq!(Grade::from_slop_score(30.0), Grade::CMinus);
        assert_eq!(Grade::from_slop_score(40.0), Grade::DMinus);
        assert_eq!(Grade::from_slop_score(50.0), Grade::F);
        assert_eq!(Grade::from_slop_score(100.0), Grade::F);
    }

    #[test]
    fn grade_ordering_lower_is_better() {
        assert!(Grade::APlus < Grade::A);
        assert!(Grade::A < Grade::B);
        assert!(Grade::B < Grade::F);
    }

    #[test]
    fn grade_meets_minimum() {
        assert!(Grade::APlus.meets_minimum(Grade::B));
        assert!(Grade::B.meets_minimum(Grade::B));
        assert!(!Grade::C.meets_minimum(Grade::B));
    }

    #[test]
    fn grade_parse_roundtrip() -> Result<(), String> {
        for grade in [
            Grade::APlus,
            Grade::A,
            Grade::AMinus,
            Grade::BPlus,
            Grade::F,
        ] {
            let s = grade.to_string();
            let parsed: Grade = s.parse()?;
            assert_eq!(parsed, grade);
        }
        Ok(())
    }

    #[test]
    fn empty_findings_perfect_score() {
        let report = GradeReport::from_findings(&[], 10, 100);
        assert_eq!(report.overall_grade, Grade::APlus);
        assert!((report.overall_score - 0.0).abs() < f32::EPSILON);
    }
}
