use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use crate::learning::{StyleBaseline, extract_baseline};

#[derive(Debug, Args)]
pub struct LearnArgs {
    /// Path to the repository whose style should be learned.
    pub path: String,
}

#[derive(Debug, Args)]
pub struct LearnShowArgs {
    /// Path to the repository whose baseline should be displayed.
    pub path: String,
}

/// Analyse `path` and write a style baseline to `.papertowel/baseline.toml`.
pub fn handle_learn(args: &LearnArgs) -> Result<()> {
    let root = PathBuf::from(&args.path);
    println!("Analysing {} ...", root.display());
    let baseline = extract_baseline(&root)?;
    let saved = baseline.save(&root)?;
    println!("Baseline saved to {}", saved.display());
    print_baseline(&baseline);
    Ok(())
}

/// Display the existing baseline for `path`.
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
    println!("  Files analysed   : {}", b.files_analyzed);
    println!("  Source lines     : {}", b.lines_analyzed);
    println!(
        "  Comment density  : {:.1}%  (calibrated threshold: {:.1}%)",
        b.avg_comment_density * 100.0,
        b.comment_density_threshold() * 100.0,
    );
    println!("  Doc-comment ratio: {:.1}%", b.avg_doc_ratio * 100.0);
    println!(
        "  Slop rate        : {:.2} hits / 100 lines",
        b.slop_rate_per_hundred
    );

    if let Some(cs) = &b.commit_stats {
        println!();
        println!("  Commit history ({} commits):", cs.commits_analysed);
        println!(
            "    Avg commit hour          : {:.1}:00",
            cs.avg_commit_hour
        );
        println!(
            "    Avg message length       : {:.0} chars",
            cs.avg_message_length
        );
        println!(
            "    Conventional commit rate : {:.0}%",
            cs.conventional_commit_rate * 100.0
        );
        println!(
            "    WIP message rate         : {:.1}%",
            cs.wip_message_rate * 100.0
        );
        let days = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
        let active: Vec<&str> = days
            .iter()
            .zip(cs.weekday_distribution.iter())
            .filter(|&(_, &f)| f > 0.05)
            .map(|(d, _)| *d)
            .collect();
        if !active.is_empty() {
            println!("    Active weekdays          : {}", active.join(", "));
        }
    }
}
