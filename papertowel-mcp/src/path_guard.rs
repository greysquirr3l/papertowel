use std::path::PathBuf;

fn normalize_component(component: &str) -> String {
    #[cfg(windows)]
    {
        component.to_ascii_lowercase()
    }
    #[cfg(not(windows))]
    {
        component.to_owned()
    }
}

fn path_components(path: &std::path::Path) -> Vec<String> {
    path.components()
        .map(|component| normalize_component(&component.as_os_str().to_string_lossy()))
        .collect()
}

fn parse_subpath_components(subpath: &str) -> Vec<String> {
    subpath
        .split(['/', '\\'])
        .filter(|segment| !segment.is_empty())
        .map(normalize_component)
        .collect()
}

fn contains_component_sequence(haystack: &[String], needle: &[String]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

#[cfg(windows)]
fn denied_prefix_match(canonical: &std::path::Path, denied: &str) -> bool {
    canonical
        .to_string_lossy()
        .to_ascii_lowercase()
        .starts_with(&denied.to_ascii_lowercase())
}

#[cfg(not(windows))]
fn denied_prefix_match(canonical: &std::path::Path, denied: &str) -> bool {
    canonical.starts_with(denied)
}

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
        if denied_prefix_match(&canonical, denied) {
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

    // Check multi-component sub-paths via component windows to avoid
    // false positives from substring matches.
    let canonical_components = path_components(&canonical);
    for subpath in DENIED_SUBPATHS {
        let denied_components = parse_subpath_components(subpath);
        if contains_component_sequence(&canonical_components, &denied_components) {
            return Err(format!(
                "path contains sensitive segment '{subpath}'; scanning is not permitted"
            ));
        }
    }

    Ok(canonical)
}
