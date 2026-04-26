use std::path::PathBuf;

/// Validate that `raw_path` is safe for the MCP server to operate on.
///
/// Rejects:
/// - Paths containing null bytes (potential injection).
/// - Paths that canonicalize to well-known sensitive system directories or
///   secret-bearing home sub-directories.
///
/// Returns the canonicalized [`PathBuf`] on success.
pub fn validate_mcp_path(raw_path: &str) -> Result<PathBuf, String> {
    #[cfg(not(windows))]
    const DENIED_PREFIXES: &[&str] = &[
        "/etc",
        "/private/etc", // macOS: /etc is a symlink to /private/etc
        "/proc",
        "/sys",
        "/dev",
    ];

    #[cfg(windows)]
    const DENIED_PREFIXES: &[&str] = &[
        r"C:\Windows\System32",
        r"C:\Windows\SysWOW64",
        r"C:\Windows\System",
    ];

    // Single path components that indicate a secret-bearing directory.
    // Checked via component iteration to work correctly on both Unix and Windows.
    const DENIED_COMPONENTS: &[&str] = &[".ssh", ".gnupg", ".pgp", ".aws", ".azure", ".kube"];

    // Multi-component sensitive sub-paths (must appear as consecutive components).
    // Each entry is a slash-separated list of components.
    const DENIED_SUBPATHS: &[&str] = &[
        ".config/gcloud",
        "Library/Keychains",
        "Library/Credentials",
        #[cfg(windows)]
        r"AppData\Roaming\.aws",
        #[cfg(windows)]
        r"AppData\Roaming\.azure",
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

    // Check single-component sensitive names via the component iterator so
    // the check works correctly regardless of path separator.
    for component in canonical.components() {
        let name = component.as_os_str().to_string_lossy();
        for denied in DENIED_COMPONENTS {
            if name.as_ref() == *denied {
                return Err(format!(
                    "path contains sensitive directory '{denied}'; scanning is not permitted"
                ));
            }
        }
    }

    // Check multi-component sub-paths by joining canonical components into a
    // normalized forward-slash string and searching for the sub-path.
    let normalized: String = canonical
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/");

    for subpath in DENIED_SUBPATHS {
        // Normalize the denied sub-path to forward slashes for comparison.
        let needle = subpath.replace('\\', "/");
        if normalized.contains(needle.as_str()) {
            return Err(format!(
                "path contains sensitive segment '{subpath}'; scanning is not permitted"
            ));
        }
    }

    Ok(canonical)
}
