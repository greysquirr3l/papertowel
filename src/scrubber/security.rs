//! Detects common security mistakes AI models make when generating code.
//!
//! Covers Rust, Go, Zig, TypeScript, C#, Python, and React (JSX/TSX).
//!
//! # Rule reference
//!
//! | ID      | Category         | What it catches                                      |
//! |---------|------------------|------------------------------------------------------|
//! | SEC001  | Injection        | String-interpolated SQL / shell commands             |
//! | SEC002  | Injection        | `eval()` / `exec()` with dynamic input               |
//! | SEC003  | Secrets          | Hardcoded secrets, API keys, or passwords            |
//! | SEC005  | Auth             | Disabled TLS verification                            |
//! | SEC006  | Auth             | JWT `alg: none` / signature not verified             |
//! | SEC007  | Input            | `dangerouslySetInnerHTML` (XSS in React)             |
//! | SEC008  | Input            | Path traversal without sanitisation                  |
//! | SEC009  | Logging          | Logging credentials or secrets                       |
//! | SEC010  | Misconfiguration | Debug mode / verbose errors in prod                  |
//! | SEC011  | Randomness       | Non-CSPRNG used for security-sensitive values        |
//! | SEC012  | Deserialisation  | Unsafe deserialisation (pickle, yaml.load, etc.)     |
//! | SEC013  | SSRF             | Raw user-controlled URL fetched without allow-list   |
//! | SEC015  | Auth             | TODO/FIXME in authentication or authorisation logic  |

use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::detection::finding::{Finding, FindingCategory, LineRange, Severity};
use crate::domain::errors::PapertowelError;

pub const DETECTOR_NAME: &str = "security";
pub const SUPPORTED_SOURCE_EXTENSIONS: &[&str] =
    &["rs", "go", "zig", "py", "ts", "tsx", "js", "jsx", "cs"];

#[must_use]
pub fn is_supported_source_extension(ext: &str) -> bool {
    let normalized = ext.to_ascii_lowercase();
    SUPPORTED_SOURCE_EXTENSIONS.contains(&normalized.as_str())
}

// ─── Rule definitions ─────────────────────────────────────────────────────────

struct Rule {
    id: &'static str,
    severity: Severity,
    confidence: f32,
    description: &'static str,
    suggestion: &'static str,
    extensions: &'static [&'static str],
    pattern: &'static str,
    /// If true the pattern is matched case-insensitively.
    ignore_case: bool,
}

static RULES: &[Rule] = &[
    // ── SEC001 · SQL / shell injection ───────────────────────────────────────
    Rule {
        id: "SEC001",
        severity: Severity::High,
        confidence: 0.80,
        description: "Possible injection: user input interpolated directly into SQL or shell command. \
                       AI-generated code frequently builds queries or commands with raw string concatenation.",
        suggestion: "Use parameterised queries / prepared statements for SQL. \
                     For shell commands use an argument list API (e.g. std::process::Command, subprocess.run([...])). \
                     Never interpolate untrusted data into a command string.",
        extensions: &["rs", "go", "ts", "tsx", "cs", "py"],
        pattern: r#"(?i)(?:format!|fmt\.Sprintf|string\.Format|(?:f"|f'))\s*\(?\s*["']?\s*(?:SELECT|INSERT|UPDATE|DELETE|DROP|EXEC|EXECUTE|CALL)\s[^"'\n]*(?:\{\d*\}|%[sdvf]|\{[a-z_]+\})[^"'\n]*["']?"#,
        ignore_case: true,
    },
    // ── SEC002 · eval / exec with dynamic data ───────────────────────────────
    Rule {
        id: "SEC002",
        severity: Severity::High,
        confidence: 0.85,
        description: "Dynamic code execution via eval() or exec() with a non-literal argument. \
                       AI often reaches for eval or exec as the simplest dynamic-dispatch solution.",
        suggestion: "Replace with a safe alternative (a dispatch table, match statement, \
                     or a proper plugin API) that does not execute arbitrary code.",
        extensions: &["ts", "tsx", "py", "js"],
        pattern: r"\b(?:eval|exec)\s*\(\s*(?:[a-zA-Z_$][a-zA-Z0-9_$]*|`[^`]*\$\{)",
        ignore_case: false,
    },
    // ── SEC003 · hardcoded secrets ────────────────────────────────────────────
    Rule {
        id: "SEC003",
        severity: Severity::High,
        confidence: 0.75,
        description: "Hardcoded secret, API key, or password detected. \
                       AI assistants routinely embed literal credentials in source code.",
        suggestion: "Load secrets from environment variables or a secrets manager. \
                     Rotate any committed credentials immediately.",
        extensions: &[],
        pattern: r#"(?i)(?:password|passwd|secret|api_key|apikey|auth_token|access_token|private_key|client_secret)\s*(?::=|[:=])\s*["'][^"'\s]{6,}["']"#,
        ignore_case: true,
    },
    Rule {
        id: "SEC004",
        severity: Severity::High,
        confidence: 0.85,
        description: "Weak or broken cryptographic algorithm in use (MD5, SHA-1, DES, 3DES, RC4, Blowfish, ECB mode). \
                       AI models frequently suggest these because they appear in older training examples.",
        suggestion: "Use a modern algorithm: SHA-256/SHA-3 for hashing, AES-256-GCM or \
                     ChaCha20-Poly1305 for encryption, Argon2id/bcrypt/scrypt for passwords.",
        extensions: &[],
        pattern: r#"\b(?:MD5|SHA1\b|SHA-1|DES\b|3DES|TripleDES|RC4|Blowfish|AES[_-]?ECB|ECB[_-]?mode|new\s+MD5|hashlib\.(?:md5|sha1)\b|MessageDigest\.getInstance\s*\(\s*["']MD5|createHash\s*\(\s*["'](?:md5|sha1)["'])"#,
        ignore_case: false,
    },
    // ── SEC005 · TLS verification disabled ────────────────────────────────────
    Rule {
        id: "SEC005",
        severity: Severity::High,
        confidence: 0.90,
        description: "TLS certificate verification disabled. This is a critical MITM vulnerability. \
                       AI-generated code disables verification to 'fix' handshake errors during development.",
        suggestion: "Never disable TLS verification in production. Fix root certificate issues \
                     instead of bypassing verification.",
        extensions: &["rs", "go", "ts", "tsx", "cs", "py"],
        pattern: r"(?i)(?:InsecureSkipVerify|verify\s*=\s*False|verify_ssl\s*=\s*False|checkCertificate\s*=\s*false|ServerCertificateValidationCallback\s*=.*?true|rejectUnauthorized\s*:\s*false|danger.*?disable.*?cert)",
        ignore_case: true,
    },
    // ── SEC006 · JWT algorithm confusion / none ───────────────────────────────
    Rule {
        id: "SEC006",
        severity: Severity::High,
        confidence: 0.88,
        description: "JWT 'alg: none' or algorithm confusion vulnerability. \
                       AI often copies JWT examples that accept any or no signing algorithm.",
        suggestion: "Always specify and pin the expected algorithm. Reject tokens with alg=none. \
                     Use a well-audited library and verify the signature before trusting any claim.",
        extensions: &["ts", "tsx", "js", "go", "py", "cs"],
        pattern: r#"(?i)(?:algorithm\s*[:=]\s*["']none["']|alg\s*[:=]\s*["']none["']|algorithms\s*[:=]\s*\[\s*["']none["']|\.decode\s*\([^)]*verify\s*=\s*False)"#,
        ignore_case: true,
    },
    // ── SEC007 · dangerouslySetInnerHTML (XSS) ────────────────────────────────
    Rule {
        id: "SEC007",
        severity: Severity::High,
        confidence: 0.90,
        description: "dangerouslySetInnerHTML used with a non-constant value. \
                       AI-generated React code uses this as a shortcut for rendering HTML, \
                       opening XSS attack vectors.",
        suggestion: "Use a sanitisation library (DOMPurify) before passing any HTML, or \
                     redesign to avoid raw HTML injection entirely.",
        extensions: &["ts", "tsx", "js", "jsx"],
        // Match dangerouslySetInnerHTML where __html value starts with a variable/expression (not a static string).
        // We flag any occurrence and let humans review; static strings are rare there anyway.
        pattern: r"dangerouslySetInnerHTML\s*=\s*\{\s*\{?\s*__html\s*:",
        ignore_case: false,
    },
    // ── SEC008 · path traversal ────────────────────────────────────────────────
    Rule {
        id: "SEC008",
        severity: Severity::High,
        confidence: 0.75,
        description: "Possible path traversal: user-supplied input used in file path construction \
                       without visible sanitisation. AI omits canonicalisation and boundary checks.",
        suggestion: "Canonicalise the resolved path and assert it remains within an allowed base \
                     directory before opening the file. Reject paths containing '..' components.",
        extensions: &["rs", "go", "ts", "tsx", "cs", "py"],
        pattern: r"(?:open|read_to_string|File::open|os\.Open|fs\.readFile|File\.Open)\s*\([^)]*(?:req\.|request\.|params\.|query\.|body\.)[a-z_]+[^)]*\)",
        ignore_case: false,
    },
    // ── SEC009 · logging credentials ──────────────────────────────────────────
    Rule {
        id: "SEC009",
        severity: Severity::Medium,
        confidence: 0.70,
        description: "Possible credential or token written to logs. \
                       AI models frequently log entire request/response objects that may contain secrets.",
        suggestion: "Redact sensitive fields before logging. Use structured logging with \
                     explicit field allowlists rather than logging entire objects.",
        extensions: &[],
        pattern: r"(?i)(?:log|println|console\.log|fmt\.Print|logging\.|logger\.)\s*[!(]?[^;\n]*(?:password|token|secret|credential|api_key|apikey|private_key)[^;\n]*",
        ignore_case: true,
    },
    // ── SEC010 · debug mode / verbose errors ──────────────────────────────────
    Rule {
        id: "SEC010",
        severity: Severity::Medium,
        confidence: 0.72,
        description: "Debug mode or verbose error detail enabled. AI-generated server code often \
                       leaves debug flags on, leaking stack traces and internal state to clients.",
        suggestion: "Set debug=False and use generic error messages in production. \
                     Log detail server-side; never expose stack traces to HTTP clients.",
        extensions: &["py", "ts", "tsx", "cs", "go"],
        pattern: r"(?i)(?:DEBUG\s*=\s*True|debug\s*:\s*true|app\.run\s*\([^)]*debug\s*=\s*True|\.UseDeveloperExceptionPage\(\)|gin\.SetMode\s*\(\s*gin\.DebugMode)",
        ignore_case: true,
    },
    // ── SEC011 · non-CSPRNG for security values ────────────────────────────────
    Rule {
        id: "SEC011",
        severity: Severity::High,
        confidence: 0.78,
        description: "Non-cryptographic RNG used in a security-sensitive context (token, nonce, key, salt, OTP). \
                       AI defaults to Math.random() / random.random() because they are simpler to use.",
        suggestion: "Use a CSPRNG: crypto.randomBytes() in Node, secrets module in Python, \
                     rand::rngs::OsRng in Rust, crypto/rand in Go, RandomNumberGenerator in C#.",
        extensions: &["ts", "tsx", "js", "py", "go", "cs"],
        pattern: r"(?i)(?:token|secret|nonce|salt|otp|session|password)[^;\n]*(?:Math\.random\(\)|random\.random\(\)|rand\.Intn|rand\.Float)|(?:Math\.random\(\)|random\.random\(\)|rand\.Intn|rand\.Float)[^;\n]*(?:token|secret|nonce|salt|otp|session|password)",
        ignore_case: true,
    },
    // ── SEC012 · unsafe deserialisation ───────────────────────────────────────
    Rule {
        id: "SEC012",
        severity: Severity::High,
        confidence: 0.85,
        description: "Unsafe deserialisation detected (pickle.loads, yaml.load without Loader, \
                       BinaryFormatter, Java ObjectInputStream on untrusted data). AI copies these \
                       patterns verbatim from documentation examples.",
        suggestion: "For YAML use yaml.safe_load(). Replace pickle with JSON or MessagePack for \
                     untrusted data. Use allow-lists for C# and Java deserialisation.",
        extensions: &["py", "cs", "ts", "tsx"],
        // yaml because regex crate does not support lookahead; false positives (yaml.safe_load) are
        // avoided by not matching that call name at all.
        pattern: r"(?i)(?:pickle\.loads?\s*\(|yaml\.load\s*\(|BinaryFormatter\(\)\.Deserialize|new\s+BinaryFormatter)",
        ignore_case: true,
    },
    // ── SEC013 · SSRF: raw user URL fetched ───────────────────────────────────
    Rule {
        id: "SEC013",
        severity: Severity::High,
        confidence: 0.72,
        description: "Possible SSRF: a URL sourced from user input is fetched without an allow-list check. \
                       AI-generated proxy or webhook handlers frequently forget this step.",
        suggestion: "Validate the host against a strict allow-list, block private/link-local ranges \
                     (169.254.*, 10.*, 172.16-31.*, 192.168.*), and use a dedicated HTTP client \
                     that does not follow redirects by default.",
        extensions: &["ts", "tsx", "go", "py", "cs", "rs"],
        pattern: r"(?i)(?:fetch|axios\.get|http\.Get|requests\.get|HttpClient\s*\.\s*GetAsync|reqwest::get)\s*\([^)]*(?:req\.|request\.|params\.|query\.|body\.)[a-z_]",
        ignore_case: true,
    },
    // ── SEC014 · hardcoded IV / nonce ─────────────────────────────────────────
    Rule {
        id: "SEC014",
        severity: Severity::High,
        confidence: 0.82,
        description: "Hardcoded initialisation vector (IV) or nonce detected. \
                       Reusing a static IV with symmetric encryption destroys semantic security.",
        suggestion: "Generate a fresh random IV/nonce for every encryption operation using a CSPRNG \
                     and prepend it to the ciphertext so it can be recovered for decryption.",
        extensions: &[],
        pattern: r#"(?i)(?:iv\s*=\s*b?["'][0-9a-f]{16,32}["']|nonce\s*=\s*b?["'][0-9a-f]{12,32}["']|iv\s*[:=]\s*\[\s*0(?:\s*,\s*0)+\s*\]|nonce\s*[:=]\s*\[\s*0(?:\s*,\s*0)+\s*\])"#,
        ignore_case: true,
    },
    // ── SEC015 · TODO in auth/authz code ─────────────────────────────────────
    Rule {
        id: "SEC015",
        severity: Severity::Medium,
        confidence: 0.68,
        description: "TODO or FIXME comment inside authentication or authorisation logic. \
                       AI frequently stubs out security checks and marks them for later — \
                       which in practice means never.",
        suggestion: "Implement the security check now; never ship a TODO inside auth/authz code. \
                     If this is intentional, track it in the issue tracker and add a test that \
                     will fail until it is addressed.",
        extensions: &[],
        pattern: r"(?i)//\s*(?:TODO|FIXME|HACK|XXX)[^\n]*(?:auth|permission|role|token|jwt|validate|verify|check|access|privilege)",
        ignore_case: true,
    },
];

// ─── Compiled rules (built once, reused across all files) ───────────────────

static COMPILED_RULES: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    RULES
        .iter()
        .map(|r| {
            regex::RegexBuilder::new(r.pattern)
                .case_insensitive(r.ignore_case)
                .multi_line(true)
                .build()
                .unwrap_or_else(|e| {
                    // SAFETY: all patterns are static literals validated by tests;
                    // a compile failure here is an unrecoverable programming error.
                    #[expect(
                        clippy::panic,
                        reason = "static regex literals; failure is a programming error"
                    )]
                    {
                        panic!("SEC regex compile error [{}]: {e}", r.id)
                    }
                })
        })
        .collect()
});

// ─── Public API ───────────────────────────────────────────────────────────────

/// Scan a single file for security findings.
pub fn detect_file(path: &Path) -> Result<Vec<Finding>, PapertowelError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_lowercase();

    if !is_supported_source_extension(&ext) {
        return Ok(Vec::new());
    }

    let skip_dirs = ["target", "vendor", "node_modules", ".git"];
    if path.components().any(|c| {
        c.as_os_str()
            .to_str()
            .is_some_and(|s| skip_dirs.contains(&s))
    }) || path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e == "lock")
        || path
            .file_name()
            .and_then(|f| f.to_str())
            .is_some_and(|f| f.ends_with(".min.js") || f.ends_with(".min.css"))
    {
        return Ok(Vec::new());
    }

    let content = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("security: could not read {}: {e}", path.display());
            return Ok(Vec::new());
        }
    };

    let mut findings = Vec::new();

    for (rule, re) in RULES.iter().zip(COMPILED_RULES.iter()) {
        let rule_extensions = if rule.extensions.is_empty() {
            SUPPORTED_SOURCE_EXTENSIONS
        } else {
            rule.extensions
        };
        if !rule_extensions.contains(&ext.as_str()) {
            continue;
        }

        for (line_idx, line) in content.lines().enumerate() {
            if re.is_match(line) {
                let line_no = line_idx + 1;
                let mut finding = Finding::new(
                    rule.id,
                    FindingCategory::Security,
                    rule.severity,
                    rule.confidence,
                    path,
                    rule.description,
                )?;
                finding.line_range = Some(LineRange::new(line_no, line_no)?);
                finding.suggestion = Some(rule.suggestion.to_owned());
                findings.push(finding);
            }
        }
    }

    findings.dedup_by(|a, b| a.id == b.id && a.line_range == b.line_range);

    Ok(findings)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use std::io::Write;

    use tempfile::NamedTempFile;

    use super::*;

    fn check(code: &str, ext: &str, expected_id: &str) {
        let mut f = NamedTempFile::with_suffix(format!(".{ext}")).unwrap();
        f.write_all(code.as_bytes()).unwrap();
        let findings = detect_file(f.path()).unwrap();
        let ids: Vec<&str> = findings.iter().map(|f| f.id.as_str()).collect();
        assert!(
            ids.contains(&expected_id),
            "expected {expected_id} in {ids:?} for code:\n{code}"
        );
    }

    fn check_no_finding(code: &str, ext: &str, forbidden_id: &str) {
        let mut f = NamedTempFile::with_suffix(format!(".{ext}")).unwrap();
        f.write_all(code.as_bytes()).unwrap();
        let findings = detect_file(f.path()).unwrap();
        let ids: Vec<&str> = findings.iter().map(|f| f.id.as_str()).collect();
        assert!(
            !ids.contains(&forbidden_id),
            "expected no {forbidden_id} but got it in {ids:?}"
        );
    }

    #[test]
    fn sec001_sql_injection_go() {
        check(
            "query := fmt.Sprintf(\"SELECT * FROM users WHERE id = %s\", userInput)",
            "go",
            "SEC001",
        );
    }

    #[test]
    fn sec001_sql_injection_python() {
        check(
            "cursor.execute(f\"SELECT * FROM users WHERE name = {name}\")",
            "py",
            "SEC001",
        );
    }

    #[test]
    fn sec003_hardcoded_password() {
        check("password = \"s3cr3tP@ss!\"", "py", "SEC003");
    }

    #[test]
    fn sec003_hardcoded_api_key() {
        check("const api_key = \"sk-abc123longkeyvalue\"", "ts", "SEC003");
    }

    #[test]
    fn sec003_no_false_positive_on_placeholder() {
        // Very short or obviously placeholder values should ideally not trigger
        // (our threshold is 6 chars; "xxx" is 3 chars)
        check_no_finding("password = \"xxx\"", "py", "SEC003");
    }

    #[test]
    fn sec004_md5_typescript() {
        check(
            "const hash = createHash('md5').update(data).digest('hex');",
            "ts",
            "SEC004",
        );
    }

    #[test]
    fn sec004_sha1_python() {
        check("h = hashlib.sha1(data).hexdigest()", "py", "SEC004");
    }

    #[test]
    fn sec005_tls_skip_verify_go() {
        check(
            "TLSClientConfig: &tls.Config{InsecureSkipVerify: true}",
            "go",
            "SEC005",
        );
    }

    #[test]
    fn sec005_tls_skip_verify_python() {
        check("requests.get(url, verify=False)", "py", "SEC005");
    }

    #[test]
    fn sec006_jwt_alg_none() {
        check("algorithm: \"none\"", "ts", "SEC006");
    }

    #[test]
    fn sec007_dangerous_inner_html() {
        check(
            "<div dangerouslySetInnerHTML={{ __html: userContent }} />",
            "tsx",
            "SEC007",
        );
    }

    #[test]
    fn sec009_logging_password() {
        check("console.log(\"password:\", password)", "ts", "SEC009");
    }

    #[test]
    fn sec010_debug_true_python() {
        check("app.run(host='0.0.0.0', debug=True)", "py", "SEC010");
    }

    #[test]
    fn sec011_math_random_token() {
        check(
            "const token = Math.random().toString(36).slice(2);",
            "ts",
            "SEC011",
        );
    }

    #[test]
    fn sec012_pickle_loads() {
        check("data = pickle.loads(user_bytes)", "py", "SEC012");
    }

    #[test]
    fn sec012_yaml_load_unsafe() {
        check("config = yaml.load(stream)", "py", "SEC012");
    }

    #[test]
    fn sec014_hardcoded_iv_zero() {
        check("iv = b\"0000000000000000\"", "py", "SEC014");
    }

    #[test]
    fn sec008_path_traversal_request_param() {
        check(
            "let file = std::fs::File::open(req.params.filename)?;",
            "rs",
            "SEC008",
        );
    }

    #[test]
    fn sec013_ssrf_user_url() {
        check("const resp = await fetch(req.query.url);", "ts", "SEC013");
    }

    #[test]
    fn sec001_sql_injection_python_single_quote() {
        check(
            "cursor.execute(f'SELECT * FROM users WHERE name = {name}')",
            "py",
            "SEC001",
        );
    }

    #[test]
    fn sec002_exec_dynamic() {
        check("exec(user_code)", "py", "SEC002");
    }

    #[test]
    fn sec003_go_short_decl() {
        check("password := \"s3cr3tP@ss!\"", "go", "SEC003");
    }

    #[test]
    fn sec008_no_path_traversal_fixed_path() {
        check_no_finding(
            "let file = std::fs::File::open(\"/fixed/path/file.txt\")?;",
            "rs",
            "SEC008",
        );
    }

    #[test]
    fn sec015_todo_in_auth() {
        check(
            "// TODO: validate the JWT token before proceeding",
            "ts",
            "SEC015",
        );
    }
}
