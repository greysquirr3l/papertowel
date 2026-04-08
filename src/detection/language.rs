/// Supported source language families.
///
/// `LanguageKind` drives detector dispatch in the scan pipeline: different
/// languages use different comment markers, function keywords, doc-comment
/// conventions, and test idioms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LanguageKind {
    Rust,
    Python,
    Go,
    TypeScript,
    CSharp,
    /// Not a language papertowel analyses structurally; lexical scan still runs.
    Unknown,
}

impl LanguageKind {
    /// Infer language from a lower-cased file extension.
    #[must_use]
    pub fn from_extension(ext: &str) -> Self {
        match ext {
            "rs" => Self::Rust,
            "py" | "pyw" => Self::Python,
            "go" => Self::Go,
            "ts" | "tsx" | "mts" => Self::TypeScript,
            "cs" => Self::CSharp,
            _ => Self::Unknown,
        }
    }

    /// Returns `true` when papertowel has structural-analysis support for this
    /// language (function-shape and test-pattern detectors).
    #[must_use]
    pub const fn is_analysable(self) -> bool {
        !matches!(self, Self::Unknown)
    }

    /// Regex pattern that matches the start of a function definition in this
    /// language.  The pattern is anchored at the start of a trimmed line.
    #[must_use]
    pub const fn fn_pattern(self) -> &'static str {
        match self {
            // Rust: optional visibility, optional async, `fn` keyword
            Self::Rust | Self::Unknown => {
                r"^\s*(?:pub(?:\s*\([^)]*\))?\s+)?(?:async\s+)?fn\s+\w+"
            }
            // Python: optional async, `def` keyword
            Self::Python => r"^\s*(?:async\s+)?def\s+\w+",
            // Go: `func` keyword, optional receiver in parens
            Self::Go => r"^\s*func\s+(?:\([^)]*\)\s*)?\w+",
            // TypeScript: named `function` keyword (includes `async function`,
            // `export function`, `export default function`)
            Self::TypeScript => r"^\s*(?:export\s+(?:default\s+)?)?(?:async\s+)?function\s+\w+",
            // C#: one or more access/modifier keywords followed by a return
            // type and method name, then `(`
            Self::CSharp => {
                r"^\s*(?:(?:public|private|protected|internal|static|virtual|override|abstract|async|readonly|sealed|extern|new)\s+)+\w[\w<>\[\]]*\s+\w+\s*\("
            }
        }
    }

    /// Regex pattern that matches a doc-comment line in this language.
    #[must_use]
    pub const fn doc_comment_pattern(self) -> &'static str {
        match self {
            // Rust and C#: `///` XML/doc comments; Unknown falls back here too
            Self::Rust | Self::CSharp | Self::Unknown => r"^\s*//[/!]",
            // Python: triple-quote strings used as docstrings (opening line)
            Self::Python => r#"^\s*(?:"{3}|'{3})"#,
            // Go: `//` package/func doc comments (godoc convention)
            Self::Go => r"^\s*//",
            // TypeScript: JSDoc `/** ... */` opening line
            Self::TypeScript => r"^\s*/\*\*",
        }
    }

    /// Returns `true` when the language uses `#` as its single-line comment
    /// prefix.  Relevant for `comments::analyze_comments`.
    #[must_use]
    pub const fn hash_comments(self) -> bool {
        matches!(self, Self::Python)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::LanguageKind;

    #[test]
    fn from_extension_maps_known_extensions() {
        assert_eq!(LanguageKind::from_extension("rs"), LanguageKind::Rust);
        assert_eq!(LanguageKind::from_extension("py"), LanguageKind::Python);
        assert_eq!(LanguageKind::from_extension("go"), LanguageKind::Go);
        assert_eq!(LanguageKind::from_extension("ts"), LanguageKind::TypeScript);
        assert_eq!(
            LanguageKind::from_extension("tsx"),
            LanguageKind::TypeScript
        );
        assert_eq!(LanguageKind::from_extension("cs"), LanguageKind::CSharp);
        assert_eq!(LanguageKind::from_extension("rb"), LanguageKind::Unknown);
        assert_eq!(LanguageKind::from_extension(""), LanguageKind::Unknown);
    }

    #[test]
    fn is_analysable_true_for_known_languages() {
        assert!(LanguageKind::Rust.is_analysable());
        assert!(LanguageKind::Python.is_analysable());
        assert!(LanguageKind::Go.is_analysable());
        assert!(LanguageKind::TypeScript.is_analysable());
        assert!(LanguageKind::CSharp.is_analysable());
        assert!(!LanguageKind::Unknown.is_analysable());
    }

    #[test]
    fn fn_pattern_compiles_for_all_variants() {
        for lang in [
            LanguageKind::Rust,
            LanguageKind::Python,
            LanguageKind::Go,
            LanguageKind::TypeScript,
            LanguageKind::CSharp,
            LanguageKind::Unknown,
        ] {
            let pattern = lang.fn_pattern();
            let re = regex::Regex::new(pattern);
            assert!(
                re.is_ok(),
                "fn_pattern for {lang:?} failed to compile: {pattern}"
            );
        }
    }

    #[test]
    fn doc_comment_pattern_compiles_for_all_variants() {
        for lang in [
            LanguageKind::Rust,
            LanguageKind::Python,
            LanguageKind::Go,
            LanguageKind::TypeScript,
            LanguageKind::CSharp,
            LanguageKind::Unknown,
        ] {
            let pattern = lang.doc_comment_pattern();
            let re = regex::Regex::new(pattern);
            assert!(
                re.is_ok(),
                "doc_comment_pattern for {lang:?} failed to compile: {pattern}"
            );
        }
    }

    #[test]
    fn fn_pattern_matches_expected_syntax() {
        let check = |lang: LanguageKind, line: &str| {
            regex::Regex::new(lang.fn_pattern())
                .expect("valid pattern")
                .is_match(line)
        };

        // Rust
        assert!(check(LanguageKind::Rust, "pub fn compute(x: u32) -> u32 {"));
        assert!(check(LanguageKind::Rust, "async fn fetch() {"));
        assert!(check(LanguageKind::Rust, "fn private() {}"));

        // Python
        assert!(check(LanguageKind::Python, "def compute(x):"));
        assert!(check(LanguageKind::Python, "async def fetch():"));
        assert!(!check(LanguageKind::Python, "x = def_value"));

        // Go
        assert!(check(LanguageKind::Go, "func Compute(x int) int {"));
        assert!(check(LanguageKind::Go, "func (r *Repo) Save() error {"));

        // TypeScript
        assert!(check(
            LanguageKind::TypeScript,
            "export async function fetchData() {"
        ));
        assert!(check(
            LanguageKind::TypeScript,
            "function compute(x: number) {"
        ));

        // C#
        assert!(check(
            LanguageKind::CSharp,
            "public async Task<int> ComputeAsync("
        ));
        assert!(check(LanguageKind::CSharp, "private static string Format("));
    }
}
