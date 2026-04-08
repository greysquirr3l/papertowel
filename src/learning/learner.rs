use std::path::Path;

use chrono::{Datelike, TimeZone, Timelike, Utc};
use git2::Repository;
use tracing::debug;
use walkdir::WalkDir;

use crate::config::{build_ignore_matcher, is_ignored, load_config};
use crate::detection::language::LanguageKind;
use crate::domain::errors::PapertowelError;
use crate::scrubber::comments::analyze_comments;
use crate::scrubber::lexical::SLOP_PATTERNS;

use super::baseline::{CommitStats, StyleBaseline, now_unix_secs};

/// Analyse all source files under `root` and derive a [`StyleBaseline`].
///
/// Files matching `.papertowelignore` and the repo's config exclusions are
/// skipped.  Files shorter than 8 non-empty lines are skipped to avoid
/// skewing averages with trivial files.
#[expect(
    clippy::cast_precision_loss,
    reason = "usize line/file counts: no meaningful precision loss at these scales"
)]
pub fn extract_baseline(root: &Path) -> Result<StyleBaseline, PapertowelError> {
    let config = load_config(root).unwrap_or_default();
    let ignore = build_ignore_matcher(root, &config)?;

    let mut total_comment_density = 0.0_f32;
    let mut total_doc_ratio = 0.0_f32;
    let mut total_slop_hits: usize = 0;
    let mut total_lines: usize = 0;
    let mut files_counted: usize = 0;

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            !ignore
                .as_ref()
                .is_some_and(|m| is_ignored(m, root, e.path(), false))
        })
    {
        let path = entry.path();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default();
        let lang = LanguageKind::from_extension(ext);
        if !lang.is_analysable() {
            continue;
        }

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                debug!(path = %path.display(), error = %e, "skipping unreadable file");
                continue;
            }
        };

        let metrics = analyze_comments(&content);
        if metrics.non_empty_lines < 8 {
            continue;
        }

        total_comment_density += metrics.density;
        total_doc_ratio += if metrics.comment_lines > 0 {
            // doc ratio: proportion of comment lines starting with `///` or `//!`
            let doc_lines = content
                .lines()
                .filter(|l| {
                    let t = l.trim_start();
                    t.starts_with("///") || t.starts_with("//!")
                })
                .count();
            doc_lines as f32 / metrics.comment_lines as f32
        } else {
            0.0
        };

        let slop_hits = count_slop_hits(&content);
        total_slop_hits += slop_hits;
        total_lines += metrics.non_empty_lines;
        files_counted += 1;
    }

    if files_counted == 0 {
        return Err(PapertowelError::Validation(
            "no analysable source files found under the given path".to_owned(),
        ));
    }

    let avg_comment_density = total_comment_density / files_counted as f32;
    let avg_doc_ratio = total_doc_ratio / files_counted as f32;
    let slop_rate_per_hundred = if total_lines > 0 {
        (total_slop_hits as f32 / total_lines as f32) * 100.0
    } else {
        0.0
    };

    Ok(StyleBaseline {
        avg_comment_density,
        avg_doc_ratio,
        slop_rate_per_hundred,
        files_analyzed: files_counted,
        lines_analyzed: total_lines,
        created_at: now_unix_secs(),
        commit_stats: extract_commit_stats(root),
    })
}

/// Analyse git history under `root` and derive [`CommitStats`].
///
/// Returns `None` when the path is not a git repository or has no commits.
#[expect(
    clippy::cast_precision_loss,
    reason = "commit counts: no meaningful precision loss at these scales"
)]
fn extract_commit_stats(root: &Path) -> Option<CommitStats> {
    let repo = Repository::discover(root)
        .map_err(|e| debug!(error = %e, "not a git repo; skipping commit stats"))
        .ok()?;

    let mut revwalk = repo.revwalk().ok()?;
    revwalk.push_head().ok()?;

    let mut hour_sum: u32 = 0;
    let mut weekday_counts = [0u32; 7];
    let mut msg_len_sum: usize = 0;
    let mut conventional_count: usize = 0;
    let mut wip_count: usize = 0;
    let mut total: usize = 0;

    for oid in revwalk.flatten() {
        let Ok(commit) = repo.find_commit(oid) else {
            continue;
        };
        let time = commit.author().when();
        // git2 gives offset in minutes; apply to get local author time.
        let offset_secs = i64::from(time.offset_minutes()) * 60;
        let unix = time.seconds() + offset_secs;
        if let Some(dt) = Utc.timestamp_opt(unix, 0).single() {
            hour_sum += dt.hour();
            // chrono weekday: Mon=0 … Sun=6
            let wd = dt.weekday().num_days_from_monday() as usize;
            if let Some(slot) = weekday_counts.get_mut(wd) {
                *slot += 1;
            }
        }

        let msg = commit.message().unwrap_or("").trim().to_owned();
        msg_len_sum += msg.len();
        if is_conventional_commit(&msg) {
            conventional_count += 1;
        }
        if is_wip_message(&msg) {
            wip_count += 1;
        }
        total += 1;
    }

    if total == 0 {
        return None;
    }

    let weekday_distribution = {
        let mut dist = [0.0_f32; 7];
        for (slot, &count) in dist.iter_mut().zip(weekday_counts.iter()) {
            *slot = count as f32 / total as f32;
        }
        dist
    };

    Some(CommitStats {
        commits_analysed: total,
        avg_commit_hour: hour_sum as f32 / total as f32,
        weekday_distribution,
        avg_message_length: msg_len_sum as f32 / total as f32,
        conventional_commit_rate: conventional_count as f32 / total as f32,
        wip_message_rate: wip_count as f32 / total as f32,
    })
}

/// Returns `true` for Conventional Commits style: `type(scope)?: message`.
#[expect(
    clippy::panic,
    reason = "static regex is a compile-time constant; panic is unreachable in practice"
)]
fn is_conventional_commit(msg: &str) -> bool {
    static CONVENTIONAL_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"^(?:feat|fix|refactor|test|docs|chore|style|perf|ci|build|revert)(?:\([^)]+\))?!?:",
        )
        .unwrap_or_else(|e| panic!("conventional commit regex is valid: {e}"))
    });
    CONVENTIONAL_RE.is_match(msg)
}

/// Returns `true` for WIP / fixup messages.
fn is_wip_message(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    lower.starts_with("wip")
        || lower.starts_with("fixup!")
        || lower.starts_with("squash!")
        || lower.contains("work in progress")
        || lower == "."
        || lower == ".."
}

/// Count occurrences of slop vocabulary words in `content`.
fn count_slop_hits(content: &str) -> usize {
    let lower = content.to_lowercase();
    SLOP_PATTERNS
        .iter()
        .map(|pattern| {
            let needle = pattern.to_lowercase();
            let mut count = 0;
            let mut start = 0;
            while let Some(pos) = lower[start..].find(needle.as_str()) {
                count += 1;
                start += pos + needle.len();
            }
            count
        })
        .sum()
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::{extract_baseline, extract_commit_stats, is_conventional_commit, is_wip_message};
    use crate::learning::baseline::CommitStats;

    fn make_repo(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().expect("tempdir");
        for (name, content) in files {
            let path = dir.path().join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("mkdir");
            }
            fs::write(path, content).expect("write");
        }
        dir
    }

    #[test]
    fn baseline_reflects_comment_density() {
        let src = "fn main() {\n// comment\n// comment\n// comment\nlet x = 1;\nlet y = 2;\nlet z = 3;\nlet w = 4;\nlet v = 5;\nlet u = x + y + z + w + v;\nprintln!(\"{u}\");\n}\n";
        let dir = make_repo(&[("src/main.rs", src)]);
        let baseline = extract_baseline(dir.path()).expect("baseline");
        assert!(baseline.files_analyzed >= 1);
        assert!(baseline.avg_comment_density > 0.0);
    }

    #[test]
    fn error_on_empty_dir() {
        let dir = TempDir::new().expect("tempdir");
        let result = extract_baseline(dir.path());
        assert!(result.is_err(), "should error with no analysable files");
    }

    #[test]
    fn commit_stats_default_is_zero() {
        let cs = CommitStats::default();
        assert_eq!(cs.commits_analysed, 0);
        assert!(
            cs.weekday_distribution
                .iter()
                .all(|&f| f.abs() < f32::EPSILON)
        );
        assert!((cs.conventional_commit_rate - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn is_conventional_commit_recognises_types() {
        assert!(is_conventional_commit("feat: add thing"));
        assert!(is_conventional_commit("fix(scope): oops"));
        assert!(is_conventional_commit("chore!: breaking"));
        assert!(!is_conventional_commit("add thing"));
        assert!(!is_conventional_commit("WIP stuff"));
    }

    #[test]
    fn is_wip_message_recognises_wip() {
        assert!(is_wip_message("wip: half done"));
        assert!(is_wip_message("WIP half done"));
        assert!(is_wip_message("fixup! previous commit"));
        assert!(is_wip_message("."));
        assert!(!is_wip_message("feat: complete thing"));
    }

    #[test]
    fn extract_commit_stats_on_repo_with_commits() {
        use git2::{Repository, Signature, Time};
        let dir = TempDir::new().expect("tempdir");
        let repo = Repository::init(dir.path()).expect("init");
        let sig = Signature::new("Test", "t@t.com", &Time::new(1_700_000_000, 0)).expect("sig");
        let tree_oid = {
            let mut idx = repo.index().expect("index");
            idx.write_tree().expect("tree")
        };
        let tree = repo.find_tree(tree_oid).expect("find tree");
        repo.commit(Some("HEAD"), &sig, &sig, "feat: initial", &tree, &[])
            .expect("commit");

        let stats = extract_commit_stats(dir.path()).expect("stats");
        assert_eq!(stats.commits_analysed, 1);
        assert!((stats.conventional_commit_rate - 1.0).abs() < f32::EPSILON);
        assert!((stats.wip_message_rate - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn non_analysable_files_are_skipped() {
        // Covers the `continue` branch when lang is not analysable (line 52).
        // A Makefile has no extension → LanguageKind::Unknown → not analysable.
        // But we also include a real Rust file so the baseline succeeds (no error).
        let src = "fn main() {}\n// one\n// two\n// three\nlet a = 1;\nlet b = 2;\nlet c = 3;\nlet d = 4;\nlet e = 5;\n";
        let dir = make_repo(&[
            ("src/main.rs", src),
            ("Makefile", "all:\n\t@echo ok\n"),
            ("random.xyz", "binary data here\n"),
        ]);
        let baseline = extract_baseline(dir.path()).expect("baseline");
        // Only main.rs is analysable — files_analyzed should be 1.
        assert_eq!(baseline.files_analyzed, 1);
    }

    #[test]
    fn short_files_are_skipped_for_density_averaging() {
        // Files with < 8 non-empty lines are excluded from density averaging
        // (line 65: `continue`).  Pair a short file with a qualifying one.
        let long_src = "fn main() {}\n// one\n// two\n// three\nlet a = 1;\nlet b = 2;\nlet c = 3;\nlet d = 4;\nlet e = 5;\n";
        let short_src = "fn a() {}\n// hi\n"; // only 2 non-empty lines
        let dir = make_repo(&[("src/main.rs", long_src), ("src/tiny.rs", short_src)]);
        let baseline = extract_baseline(dir.path()).expect("baseline");
        // short file is skipped so only main.rs contributes.
        assert_eq!(baseline.files_analyzed, 1);
    }

    #[test]
    fn zero_comment_lines_produces_zero_doc_ratio() {
        // When a file has no comment lines, doc_ratio branch returns 0.0 (line 80).
        let src = "fn add(a: i32, b: i32) -> i32 { a + b }\nfn sub(a: i32, b: i32) -> i32 { a - b }\nfn mul(a: i32, b: i32) -> i32 { a * b }\nfn div(a: i32, b: i32) -> i32 { a / b }\nfn rem(a: i32, b: i32) -> i32 { a % b }\nfn neg(a: i32) -> i32 { -a }\nfn zero() -> i32 { 0 }\nfn one() -> i32 { 1 }\nfn two() -> i32 { 2 }\n";
        let dir = make_repo(&[("src/math.rs", src)]);
        let baseline = extract_baseline(dir.path()).expect("baseline");
        assert!((baseline.avg_comment_density - 0.0).abs() < f32::EPSILON);
        assert!((baseline.avg_doc_ratio - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn slop_hits_are_counted() {
        // Content with known slop vocabulary (lines 222-223: inner while loop in count_slop_hits).
        let src = "// This function is comprehensive and robust.\n// It leverages seamless integration.\n// Utilize this helper to facilitate streamlined processing.\nfn robust_helper() {}\nfn seamless_util() {}\nfn leverage_this() {}\nfn facilitate_that() {}\nfn streamline_more() {}\nfn utilize_all() {}\n";
        let dir = make_repo(&[("src/slop.rs", src)]);
        let baseline = extract_baseline(dir.path()).expect("baseline");
        assert!(
            baseline.slop_rate_per_hundred > 0.0,
            "slop patterns should be counted: rate={}",
            baseline.slop_rate_per_hundred
        );
    }

    #[test]
    fn extract_commit_stats_returns_none_for_non_git_dir() {
        // Non-git directory → extract_commit_stats returns None (line 165).
        let dir = TempDir::new().expect("tempdir");
        let result = extract_commit_stats(dir.path());
        assert!(result.is_none(), "non-git dir should produce None");
    }

    #[test]
    fn extract_commit_stats_counts_wip_messages() {
        // Covers wip_count += 1 (line 159).
        use git2::{Repository, Signature, Time};
        let dir = TempDir::new().expect("tempdir");
        let repo = Repository::init(dir.path()).expect("init");
        let sig = Signature::new("Test", "t@t.com", &Time::new(1_700_000_000, 0)).expect("sig");
        let tree_oid = {
            let mut idx = repo.index().expect("index");
            idx.write_tree().expect("tree")
        };
        let tree = repo.find_tree(tree_oid).expect("find tree");
        // Commit a WIP message to trigger the wip_count branch.
        let parent_oid = repo
            .commit(Some("HEAD"), &sig, &sig, "feat: initial", &tree, &[])
            .expect("commit 1");
        let parent = repo.find_commit(parent_oid).expect("find parent");
        repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            "wip: half done",
            &tree,
            &[&parent],
        )
        .expect("commit 2");

        let stats = extract_commit_stats(dir.path()).expect("stats");
        assert_eq!(stats.commits_analysed, 2);
        assert!(
            stats.wip_message_rate > 0.0,
            "wip_message_rate should be > 0 for wip commit"
        );
    }

    #[test]
    fn extract_baseline_skips_unreadable_file_and_continues() {
        // Covers lines 57-59: Err branch when read_to_string fails.
        // Creates a .rs file with no read permission alongside a readable 8-line file.
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().expect("tempdir");
        // Readable 8-line file so extract_baseline doesn't fail entirely.
        fs::write(
            dir.path().join("main.rs"),
            "pub fn a(){}\npub fn b(){}\npub fn c(){}\npub fn d(){}\npub fn e(){}\npub fn f(){}\npub fn g(){}\npub fn h(){}\n",
        ).expect("write main");
        // Unreadable .rs file → exercises the Err(e) → continue branch.
        let no_read = dir.path().join("secret.rs");
        fs::write(&no_read, "fn secret() {}\n").expect("write secret");
        fs::set_permissions(&no_read, std::fs::Permissions::from_mode(0o000)).expect("chmod 000");
        // Should succeed, skipping the unreadable file.
        let result = extract_baseline(dir.path());
        // Restore for cleanup.
        fs::set_permissions(&no_read, std::fs::Permissions::from_mode(0o644)).expect("chmod 644");
        assert!(
            result.is_ok(),
            "baseline should succeed despite unreadable file"
        );
    }

    #[test]
    fn extract_baseline_zero_total_lines_produces_zero_slop_rate() {
        // Covers line 100: the `else { 0.0 }` branch when total_lines == 0.
        // A file with only whitespace/blank lines has 0 counted lines but may still
        // pass the non_empty_lines >= 8 check if we craft the content to have 8
        // non-empty structural chars but 0 for the slop_rate denominator path.
        // Actually total_lines counts non-blank lines; a file with 8+ non-blank lines
        // always has total_lines > 0. This branch is unreachable in practice.
        // We verify the normal path instead as a sanity check.
        let dir = make_repo(&[(
            "src/lib.rs",
            "fn a(){}\nfn b(){}\nfn c(){}\nfn d(){}\nfn e(){}\nfn f(){}\nfn g(){}\nfn h(){}\n",
        )]);
        let baseline = extract_baseline(dir.path()).expect("baseline");
        // slop_rate is 0 since the file has no slop words.
        assert!((baseline.slop_rate_per_hundred - 0.0_f32).abs() < 0.01);
    }
}
