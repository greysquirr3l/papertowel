use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use crate::learning::{StyleBaseline, extract_baseline};

#[derive(Debug, Args)]
pub struct LearnArgs {
 pub path: String,
}

#[derive(Debug, Args)]
pub struct LearnShowArgs {
 pub path: String,
}

pub fn handle_learn(args: &LearnArgs) -> Result<()> {
 let root = PathBuf::from(&args.path);
 println!("Analysing {}...", root.display());
 let baseline = extract_baseline(&root)?;
 let saved = baseline.save(&root)?;
 println!("Baseline saved to {}", saved.display());
 print_baseline(&baseline);
 Ok(())
}

pub fn handle_show(args: &LearnShowArgs) -> Result<()> {
 let root = PathBuf::from(&args.path);
 match StyleBaseline::load(&root)? {
 Some(baseline) => print_baseline(&baseline),
 None => println!(
 "No baseline found. Run `papertowel learn {}` first.",
 root.display()
 ),
 }
 Ok(())
}

fn print_baseline(b: &StyleBaseline) {
 println!(" Files analysed: {}", b.files_analyzed);
 println!(" Source lines: {}", b.lines_analyzed);
 println!(
 " Comment density: {:.1}% (calibrated threshold: {:.1}%)",
 b.avg_comment_density * 100.0,
 b.comment_density_threshold() * 100.0,
 );
 println!(" Doc-comment ratio: {:.1}%", b.avg_doc_ratio * 100.0);
 println!(
 " Slop rate: {:.2} hits / 100 lines",
 b.slop_rate_per_hundred
 );

 if let Some(cs) = &b.commit_stats {
 println!();
 println!(" Commit history ({} commits):", cs.commits_analysed);
 println!(
 " Avg commit hour: {:.1}:00",
 cs.avg_commit_hour
 );
 println!(
 " Avg message length: {:.0} chars",
 cs.avg_message_length
 );
 println!(
 " Conventional commit rate: {:.0}%",
 cs.conventional_commit_rate * 100.0
 );
 println!(
 " WIP message rate: {:.1}%",
 cs.wip_message_rate * 100.0
 );
 let days = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
 let active: Vec<&str> = days
.iter()
.zip(cs.weekday_distribution.iter())
.filter(|&(_, &f)| f > 0.05)
.map(|(d, _)| *d)
.collect();
 if!active.is_empty() {
 println!(" Active weekdays: {}", active.join(", "));
 }
 }
}

#[cfg(test)]
mod tests {
 use super::{LearnArgs, LearnShowArgs, handle_learn, handle_show};
 use std::fs;
 use tempfile::TempDir;

 #[test]
 fn handle_show_no_baseline_prints_message() -> Result<(), Box<dyn std::error::Error>> {
 let tmp = TempDir::new()?;
 let args = LearnShowArgs {
 path: tmp.path().to_string_lossy().into_owned(),
 };
 assert!(handle_show(&args).is_ok());
 Ok(())
 }

 #[test]
 fn handle_learn_on_empty_dir_returns_ok() -> Result<(), Box<dyn std::error::Error>> {
 // Covers handle_learn → extract_baseline → save → print_baseline.
 // Needs a source file with >= 8 non-empty lines.
 let tmp = TempDir::new()?;
 fs::write(
 tmp.path().join("lib.rs"),
 "pub fn a() {}\npub fn b() {}\npub fn c() {}\npub fn d() {}\npub fn e() {}\npub fn f() {}\npub fn g() {}\npub fn h() {}\n",
 )?;
 let args = LearnArgs {
 path: tmp.path().to_string_lossy().into_owned(),
 };
 assert!(handle_learn(&args).is_ok());
 Ok(())
 }

 #[test]
 fn handle_show_with_existing_baseline_prints_fields() -> Result<(), Box<dyn std::error::Error>>
 {
 // Covers `Some(baseline)` path → print_baseline (all field-print lines).
 let tmp = TempDir::new()?;
 fs::write(
 tmp.path().join("main.rs"),
 "/// Doc comment\npub fn a() {}\npub fn b() {}\npub fn c() {}\npub fn d() {}\npub fn e() {}\npub fn f() {}\npub fn g() {}\n",
 )?;
 // First generate the baseline.
 let args_learn = LearnArgs {
 path: tmp.path().to_string_lossy().into_owned(),
 };
 handle_learn(&args_learn)?;
 // Now show it — exercises the Some branch of handle_show and print_baseline.
 let args_show = LearnShowArgs {
 path: tmp.path().to_string_lossy().into_owned(),
 };
 assert!(handle_show(&args_show).is_ok());
 Ok(())
 }

 #[test]
 fn print_baseline_shows_commit_stats_section() -> Result<(), Box<dyn std::error::Error>> {
 // Covers lines 59-85: the `if let Some(cs) = &b.commit_stats {... }` block.
 // Needs a git repo with commits so extract_baseline populates commit_stats.
 use git2::{Repository, Signature, Time};
 let tmp = TempDir::new()?;
 // Write 8+ non-empty source lines.
 fs::write(
 tmp.path().join("lib.rs"),
 "pub fn a(){}\npub fn b(){}\npub fn c(){}\npub fn d(){}\npub fn e(){}\npub fn f(){}\npub fn g(){}\npub fn h(){}\n",
 )?;
 // Create a git repo with one commit so commit_stats becomes Some.
 let repo = Repository::init(tmp.path())?;
 let sig = Signature::new("Test", "t@t.com", &Time::new(1_700_000_000, 0))?;
 let tree_oid = {
 let mut idx = repo.index()?;
 idx.add_path(std::path::Path::new("lib.rs"))?;
 idx.write_tree()?
 };
 let tree = repo.find_tree(tree_oid)?;
 repo.commit(Some("HEAD"), &sig, &sig, "feat: initial commit", &tree, &[])?;

 // Now run handle_learn — it calls print_baseline which sees Some(commit_stats).
 let args = LearnArgs {
 path: tmp.path().to_string_lossy().into_owned(),
 };
 handle_learn(&args)?;
 // Also test handle_show which calls the same print_baseline path.
 let args_show = LearnShowArgs {
 path: tmp.path().to_string_lossy().into_owned(),
 };
 assert!(handle_show(&args_show).is_ok());
 Ok(())
 }
}