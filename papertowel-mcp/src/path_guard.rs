use std::path::PathBuf;

/// Validate that `raw_path` is safe for the MCP server to operate on.
///
/// Rejects:
/// - Paths containing null bytes (potential injection).
/// - Paths that canonicalize to well-known sensitive system directories
///   (`/etc`, `/proc`, `/sys`, `/dev`) or common secret-bearing home
///   sub-directories (`.ssh`, `.gnupg`, `.aws`, `.config/gcloud`).
///
/// Returns the canonicalized [`PathBuf`] on success.
pub fn validate_mcp_path(raw_path: &str) -> Result<PathBuf, String> {
    const DENIED_PREFIXES: &[&str] = &[
        "/etc",
        "/private/etc", // macOS: /etc is a symlink to /private/etc
        "/proc",
        "/sys",
        "/dev",
    ];
    const DENIED_SEGMENTS: &[&str] = &[
        ".ssh",
        ".gnupg",
        ".pgp",
        ".aws",
        ".azure",
        ".config/gcloud",
        ".kube",
        "Library/Keychains",
        "Library/Credentials",
    ];

    // Null-byte check.
    if raw_path.contains('\0') {
        return Err("path contains a null byte".to_owned());
    }

    let path = PathBuf::from(raw_path);

    // Canonicalize to resolve `..` and symlinks before the sensitive-prefix check.
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("path is invalid or does not exist: {e}"))?;

    for denied in DENIED_PREFIXES {
        if canonical.starts_with(denied) {
            return Err(format!(
                "scanning '{denied}' is not permitted by the MCP server"
            ));
        }
    }

    let canonical_str = canonical.to_string_lossy();
    for segment in DENIED_SEGMENTS {
        if canonical_str.contains(segment) {
            return Err(format!(
                "path contains sensitive segment '{segment}'; scanning is not permitted"
            ));
        }
    }

    Ok(canonical)
}
