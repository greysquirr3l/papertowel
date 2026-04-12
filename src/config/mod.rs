use std::fs;
use std::path::{Path, PathBuf};

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use serde::{Deserialize, Serialize};

use crate::domain::errors::PapertowelError;

/// Name of the repo-level configuration file.
pub const CONFIG_FILE_NAME: &str = ".papertowel.toml";
/// Name of the gitignore-syntax path exclusion file.
pub const IGNORE_FILE_NAME: &str = ".papertowelignore";
/// Name of the global configuration file inside the user config directory.
pub const GLOBAL_CONFIG_FILE_NAME: &str = "config.toml";
/// Subdirectory under `$XDG_CONFIG_HOME` or `~/.config` for papertowel.
const CONFIG_DIR_NAME: &str = "papertowel";

// ─── Enums ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ScrubberAggression {
    Gentle,
    #[default]
    Moderate,
    Aggressive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MinimumSeverity {
    Low,
    #[default]
    Medium,
    High,
}

// ─── Sub-configs ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DetectorConfig {
    pub lexical: bool,
    pub comments: bool,
    pub structure: bool,
    pub readme: bool,
    pub metadata: bool,
    pub commit_pattern: bool,
    pub tests: bool,
    pub workflow: bool,
    pub maintenance: bool,
    pub promotion: bool,
    pub name_credibility: bool,
    pub idiom_mismatch: bool,
    pub prompt: bool,
    pub security: bool,
}

impl Default for DetectorConfig {
    fn default() -> Self {
        Self {
            lexical: true,
            comments: true,
            structure: true,
            readme: true,
            metadata: true,
            commit_pattern: true,
            tests: true,
            workflow: true,
            maintenance: true,
            promotion: true,
            name_credibility: true,
            idiom_mismatch: true,
            prompt: true,
            security: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SeverityConfig {
    pub minimum: MinimumSeverity,
}

impl Default for SeverityConfig {
    fn default() -> Self {
        Self {
            minimum: MinimumSeverity::Medium,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScrubberConfig {
    pub aggression: ScrubberAggression,
}

impl Default for ScrubberConfig {
    fn default() -> Self {
        Self {
            aggression: ScrubberAggression::Moderate,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct WringerProjectConfig {
    pub default_persona: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ExcludeConfig {
    pub paths: Vec<String>,
}

///
/// All sections are optional; missing sections use their `Default`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ProjectConfig {
    pub detectors: DetectorConfig,
    pub severity: SeverityConfig,
    pub scrubber: ScrubberConfig,
    pub wringer: WringerProjectConfig,
    pub exclude: ExcludeConfig,
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Walk up the filesystem from `start` looking for project-root markers:
/// `.papertowel.toml`, `.papertowelignore`, or `.git/`.
///
/// Returns the first ancestor (or `start` itself) that contains a marker.
/// Falls back to `start` (canonicalized) if no marker is found.
pub fn discover_project_root(start: &Path) -> PathBuf {
    let canonical = fs::canonicalize(start).unwrap_or_else(|_| start.to_path_buf());
    let search_start = if canonical.is_file() {
        canonical
            .parent()
            .map_or_else(|| canonical.clone(), Path::to_path_buf)
    } else {
        canonical.clone()
    };

    let mut current = search_start.as_path();
    loop {
        if current.join(CONFIG_FILE_NAME).exists()
            || current.join(IGNORE_FILE_NAME).exists()
            || current.join(".git").exists()
        {
            return current.to_path_buf();
        }
        match current.parent() {
            Some(parent) if parent != current => current = parent,
            _ => break,
        }
    }
    canonical
}

/// Return the global papertowel config directory.
///
/// Resolves to `$XDG_CONFIG_HOME/papertowel` (or `~/.config/papertowel`).
/// Returns `None` if the home directory cannot be determined.
pub fn global_config_dir() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        let dir = PathBuf::from(xdg).join(CONFIG_DIR_NAME);
        return Some(dir);
    }
    std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".config").join(CONFIG_DIR_NAME))
}

/// Load the global config from `~/.config/papertowel/config.toml`.
///
/// Returns `ProjectConfig::default()` if the file does not exist.
pub fn load_global_config() -> Result<ProjectConfig, PapertowelError> {
    let Some(dir) = global_config_dir() else {
        return Ok(ProjectConfig::default());
    };
    load_config_from_file(&dir.join(GLOBAL_CONFIG_FILE_NAME))
}

fn load_config_from_file(path: &Path) -> Result<ProjectConfig, PapertowelError> {
    if !path.exists() {
        return Ok(ProjectConfig::default());
    }
    let text =
        fs::read_to_string(path).map_err(|error| PapertowelError::io_with_path(path, error))?;
    toml::from_str(&text).map_err(PapertowelError::TomlDeserialize)
}

/// Discover the project root from `scan_path`, load the global and project
/// configs (project wins where set), and build the ignore matcher.
///
/// Returns `(project_root, config, ignore_matcher)`.
pub fn resolve_config(
    scan_path: &Path,
) -> Result<(PathBuf, ProjectConfig, Option<Gitignore>), PapertowelError> {
    let project_root = discover_project_root(scan_path);
    let global = load_global_config()?;
    let project = load_config(&project_root)?;
    let merged = merge_configs(global, project);
    let ignore = build_ignore_matcher(&project_root, &merged)?;
    Ok((project_root, merged, ignore))
}

fn merge_configs(global: ProjectConfig, project: ProjectConfig) -> ProjectConfig {
    // If the project has a config file, it takes full precedence.
    // Global config acts as the baseline default; any explicitly set
    // project-level values override.  Because serde default-fills every
    // field, we can't distinguish "user set this to the default" from
    // "field was absent".  For now, project config wins wholesale when the
    // project's config file existed (indicated by non-default values).
    // The simplest correct behavior: merge exclude paths.
    ProjectConfig {
        detectors: project.detectors,
        severity: project.severity,
        scrubber: project.scrubber,
        wringer: project.wringer,
        exclude: ExcludeConfig {
            paths: {
                let mut paths = global.exclude.paths;
                paths.extend(project.exclude.paths);
                paths
            },
        },
    }
}

pub fn load_config(repo_root: impl AsRef<Path>) -> Result<ProjectConfig, PapertowelError> {
    let path = repo_root.as_ref().join(CONFIG_FILE_NAME);

    if !path.exists() {
        return Ok(ProjectConfig::default());
    }

    let text =
        fs::read_to_string(&path).map_err(|error| PapertowelError::io_with_path(&path, error))?;
    toml::from_str(&text).map_err(PapertowelError::TomlDeserialize)
}

pub fn save_config(
    repo_root: impl AsRef<Path>,
    config: &ProjectConfig,
) -> Result<(), PapertowelError> {
    let path = repo_root.as_ref().join(CONFIG_FILE_NAME);
    let text = toml::to_string_pretty(config).map_err(PapertowelError::TomlSerialize)?;
    fs::write(&path, text).map_err(|error| PapertowelError::io_with_path(&path, error))
}

/// Build a [`Gitignore`] matcher from:
/// 1. Patterns listed in `config.exclude.paths`
///
/// "nothing is ignored".
pub fn build_ignore_matcher(
    repo_root: impl AsRef<Path>,
    config: &ProjectConfig,
) -> Result<Option<Gitignore>, PapertowelError> {
    let repo_root = repo_root.as_ref();
    let mut builder = GitignoreBuilder::new(repo_root);

    // Inline patterns from [exclude] section
    for pattern in &config.exclude.paths {
        builder
            .add_line(None, pattern)
            .map_err(|e| PapertowelError::Config(e.to_string()))?;
    }

    let ignore_file = repo_root.join(IGNORE_FILE_NAME);
    if ignore_file.exists() {
        builder.add(ignore_file);
    }

    // If nothing was added, skip building the matcher entirely
    if config.exclude.paths.is_empty() && !repo_root.join(IGNORE_FILE_NAME).exists() {
        return Ok(None);
    }

    let gitignore = builder
        .build()
        .map_err(|e| PapertowelError::Config(e.to_string()))?;

    Ok(Some(gitignore))
}

pub fn is_ignored(matcher: &Gitignore, root: &Path, path: &Path, is_dir: bool) -> bool {
    let relative = path.strip_prefix(root).unwrap_or(path);
    matcher
        .matched_path_or_any_parents(relative, is_dir)
        .is_ignore()
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::io::Write;

    use tempfile::TempDir;

    use super::{
        CONFIG_FILE_NAME, DetectorConfig, GLOBAL_CONFIG_FILE_NAME, IGNORE_FILE_NAME, ProjectConfig,
        ScrubberAggression, SeverityConfig, build_ignore_matcher, discover_project_root,
        is_ignored, load_config, load_config_from_file, resolve_config, save_config,
    };

    fn scratch() -> TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    #[test]
    fn missing_config_returns_default() {
        let dir = scratch();
        let config = load_config(dir.path()).expect("load_config");
        assert_eq!(config, ProjectConfig::default());
    }

    #[test]
    fn config_roundtrips_toml() {
        let dir = scratch();
        let mut config = ProjectConfig::default();
        config.detectors.lexical = false;
        config.scrubber.aggression = ScrubberAggression::Aggressive;

        save_config(dir.path(), &config).expect("save_config");
        let loaded = load_config(dir.path()).expect("load_config");
        assert_eq!(loaded, config);
    }

    #[test]
    fn partial_toml_uses_defaults_for_missing_sections() {
        let dir = scratch();
        let partial = r#"
[scrubber]
aggression = "gentle"
"#;
        std::fs::write(dir.path().join(".papertowel.toml"), partial).expect("write");
        let config = load_config(dir.path()).expect("load_config");
        assert_eq!(config.scrubber.aggression, ScrubberAggression::Gentle);
        assert_eq!(config.detectors, DetectorConfig::default());
        assert_eq!(config.severity, SeverityConfig::default());
    }

    #[test]
    fn no_ignore_patterns_returns_none_matcher() {
        let dir = scratch();
        let config = ProjectConfig::default();
        let matcher = build_ignore_matcher(dir.path(), &config).expect("build");
        assert!(matcher.is_none());
    }

    #[test]
    fn inline_exclude_pattern_ignores_vendor() {
        let dir = scratch();
        let mut config = ProjectConfig::default();
        config.exclude.paths.push("vendor/**".to_owned());

        let matcher = build_ignore_matcher(dir.path(), &config)
            .expect("build")
            .expect("should produce a matcher");

        let vendor_path = dir.path().join("vendor").join("lib.rs");
        assert!(
            is_ignored(&matcher, dir.path(), &vendor_path, false),
            "vendor/lib.rs should be ignored"
        );
    }

    #[test]
    fn papertowelignore_file_is_respected() {
        let dir = scratch();
        let mut f =
            std::fs::File::create(dir.path().join(".papertowelignore")).expect("create ignore");
        writeln!(f, "generated/").expect("write");
        drop(f);

        let config = ProjectConfig::default();
        let matcher = build_ignore_matcher(dir.path(), &config)
            .expect("build")
            .expect("should produce a matcher");

        let generated_path = dir.path().join("generated").join("schema.rs");
        assert!(
            is_ignored(&matcher, dir.path(), &generated_path, false),
            "generated/schema.rs should be ignored"
        );
    }

    #[test]
    fn non_ignored_path_passes_through() {
        let dir = scratch();
        let mut config = ProjectConfig::default();
        config.exclude.paths.push("vendor/**".to_owned());

        let matcher = build_ignore_matcher(dir.path(), &config)
            .expect("build")
            .expect("matcher");

        let src_path = dir.path().join("src").join("lib.rs");
        assert!(
            !is_ignored(&matcher, dir.path(), &src_path, false),
            "src/lib.rs should not be ignored"
        );
    }

    // ─── Discovery tests ──────────────────────────────────────────────────

    #[test]
    fn discover_finds_root_by_papertowel_toml() {
        let dir = scratch();
        std::fs::write(dir.path().join(CONFIG_FILE_NAME), "").expect("write");
        let sub = dir.path().join("src").join("deep");
        std::fs::create_dir_all(&sub).expect("mkdir");

        let root = discover_project_root(&sub);
        assert_eq!(root, dir.path().canonicalize().expect("canon"));
    }

    #[test]
    fn discover_finds_root_by_papertowelignore() {
        let dir = scratch();
        std::fs::write(dir.path().join(IGNORE_FILE_NAME), "target/\n").expect("write");
        let sub = dir.path().join("a").join("b");
        std::fs::create_dir_all(&sub).expect("mkdir");

        let root = discover_project_root(&sub);
        assert_eq!(root, dir.path().canonicalize().expect("canon"));
    }

    #[test]
    fn discover_finds_root_by_git_dir() {
        let dir = scratch();
        std::fs::create_dir(dir.path().join(".git")).expect("mkdir");
        let sub = dir.path().join("lib");
        std::fs::create_dir_all(&sub).expect("mkdir");

        let root = discover_project_root(&sub);
        assert_eq!(root, dir.path().canonicalize().expect("canon"));
    }

    #[test]
    fn discover_falls_back_to_start_when_no_marker() {
        let dir = scratch();
        let sub = dir.path().join("nowhere");
        std::fs::create_dir_all(&sub).expect("mkdir");

        let root = discover_project_root(&sub);
        // Falls back to the canonicalized start path (may go further up
        // if the temp dir itself lives inside a git repo, so just check
        // the sub dir is a prefix).
        assert!(
            sub.canonicalize().expect("canon").starts_with(&root),
            "expected {sub:?} to start with {root:?}"
        );
    }

    #[test]
    fn resolve_config_finds_ignore_from_subdirectory() {
        let dir = scratch();
        std::fs::write(dir.path().join(IGNORE_FILE_NAME), "generated/\n").expect("write");
        let sub = dir.path().join("src");
        std::fs::create_dir_all(&sub).expect("mkdir");

        let (project_root, _config, ignore) = resolve_config(&sub).expect("resolve");
        assert_eq!(project_root, dir.path().canonicalize().expect("canon"));

        let canon_dir = dir.path().canonicalize().expect("canon");
        let gen_path = canon_dir.join("generated").join("foo.rs");
        assert!(
            ignore
                .as_ref()
                .is_some_and(|m| is_ignored(m, &project_root, &gen_path, false))
        );
    }

    #[test]
    fn global_config_is_loaded_from_file() {
        let dir = scratch();
        let config_path = dir.path().join(GLOBAL_CONFIG_FILE_NAME);
        std::fs::write(&config_path, "[exclude]\npaths = [\"vendor/**\"]\n").expect("write");

        let global = load_config_from_file(&config_path).expect("load");
        assert_eq!(global.exclude.paths, vec!["vendor/**".to_owned()]);
    }

    #[test]
    fn global_config_missing_file_returns_default() {
        let dir = scratch();
        let config_path = dir.path().join(GLOBAL_CONFIG_FILE_NAME);
        let global = load_config_from_file(&config_path).expect("load");
        assert_eq!(global, ProjectConfig::default());
    }
}
