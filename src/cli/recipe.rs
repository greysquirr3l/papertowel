use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use crate::recipe::loader::{RecipeLoader, list_available_recipes};
use crate::recipe::types::RecipeSource;

#[derive(Debug, Args)]
pub struct ListArgs {
    /// Show only recipes from a specific source (builtin, user, repo).
    #[arg(long)]
    source: Option<String>,
}

#[derive(Debug, Args)]
pub struct ShowArgs {
    name: String,

    #[arg(long)]
    raw: bool,
}

#[derive(Debug, Args)]
pub struct ValidateArgs {
    path: PathBuf,
}

/// List available recipes.
pub fn handle_list(args: &ListArgs) -> Result<()> {
    let cwd = std::env::current_dir().ok();
    let recipes = list_available_recipes(cwd.as_deref());

    let source_filter = args.source.as_deref();

    println!("Available recipes:\n");
    println!("{:<25} {:<10} LOCATION", "NAME", "SOURCE");
    println!("{}", "-".repeat(70));

    for (name, source) in recipes {
        let source_str = match &source {
            RecipeSource::Builtin => "builtin",
            RecipeSource::UserGlobal(_) => "user",
            RecipeSource::RepoLocal(_) => "repo",
        };

        // Apply filter if specified.
        if let Some(filter) = source_filter
            && source_str != filter
        {
            continue;
        }

        let location = match &source {
            RecipeSource::Builtin => "(embedded)".to_owned(),
            RecipeSource::UserGlobal(p) | RecipeSource::RepoLocal(p) => p.display().to_string(),
        };

        println!("{name:<25} {source_str:<10} {location}");
    }

    Ok(())
}

/// Show details of a specific recipe.
pub fn handle_show(args: &ShowArgs) -> Result<()> {
    let cwd = std::env::current_dir().ok();
    let loader = RecipeLoader::new(cwd);
    let recipes = loader.load_all()?;

    let recipe = recipes
        .iter()
        .find(|r| r.recipe.recipe.name == args.name)
        .ok_or_else(|| anyhow::anyhow!("recipe '{}' not found", args.name))?;

    if args.raw {
        match &recipe.source {
            RecipeSource::Builtin => {
                println!("# Builtin recipe: {}", args.name);
                println!("# Raw TOML not available for embedded recipes.");
                println!(
                    "# Use 'papertowel recipe show {}' without --raw for details.",
                    args.name
                );
            }
            RecipeSource::UserGlobal(p) | RecipeSource::RepoLocal(p) => {
                let content = std::fs::read_to_string(p)?;
                println!("{content}");
            }
        }
    } else {
        let r = &recipe.recipe;
        println!("Recipe: {}", r.recipe.name);
        println!("Version: {}", r.recipe.version);
        println!("Source: {}", recipe.source);
        println!("Category: {:?}", r.recipe.category);
        println!("Default Severity: {:?}", r.recipe.default_severity);
        println!("Enabled: {}", r.recipe.enabled);

        if !r.recipe.description.is_empty() {
            println!("\nDescription:\n  {}", r.recipe.description);
        }

        println!("\nPatterns:");

        if let Some(ref words) = r.patterns.words
            && words.enabled
        {
            println!("  Words: {} items", words.items.len());
            if words.items.len() <= 10 {
                for word in &words.items {
                    println!("    - {word}");
                }
            } else {
                for word in words.items.iter().take(5) {
                    println!("    - {word}");
                }
                println!("    ... and {} more", words.items.len() - 5);
            }
        }

        if let Some(ref phrases) = r.patterns.phrases
            && phrases.enabled
        {
            println!("  Phrases: {} items", phrases.items.len());
            if phrases.items.len() <= 5 {
                for phrase in &phrases.items {
                    println!("    - {}", phrase.pattern());
                }
            } else {
                for phrase in phrases.items.iter().take(3) {
                    println!("    - {}", phrase.pattern());
                }
                println!("    ... and {} more", phrases.items.len() - 3);
            }
        }

        if !r.patterns.regex.is_empty() {
            println!("  Regex patterns: {}", r.patterns.regex.len());
            for regex in &r.patterns.regex {
                println!("    - {}: {}", regex.name, regex.pattern);
            }
        }

        if !r.patterns.contextual.is_empty() {
            println!("  Contextual patterns: {}", r.patterns.contextual.len());
            for ctx in &r.patterns.contextual {
                println!("    - {}: applies to {:?}", ctx.name, ctx.applies_to);
            }
        }

        println!("\nScoring:");
        println!(
            "  Cluster threshold: {} matches in {} lines",
            r.scoring.cluster_threshold, r.scoring.cluster_range_lines
        );
        if let Some(boost) = r.scoring.cluster_severity_boost {
            println!("  Cluster severity boost: {boost:?}");
        }
        println!(
            "  Base confidence: {:.0}%",
            r.scoring.base_confidence * 100.0
        );
    }

    Ok(())
}

/// Validate a recipe file.
pub fn handle_validate(args: &ValidateArgs) -> Result<()> {
    let content = std::fs::read_to_string(&args.path)?;

    match toml::from_str::<crate::recipe::types::Recipe>(&content) {
        Ok(recipe) => {
            println!("✓ Recipe '{}' is valid", recipe.recipe.name);
            println!("  Version: {}", recipe.recipe.version);
            println!("  Category: {:?}", recipe.recipe.category);

            let word_count = recipe.patterns.words.as_ref().map_or(0, |w| w.items.len());
            let phrase_count = recipe
                .patterns
                .phrases
                .as_ref()
                .map_or(0, |p| p.items.len());
            let regex_count = recipe.patterns.regex.len();
            let ctx_count = recipe.patterns.contextual.len();

            println!(
                "  Patterns: {word_count} words, {phrase_count} phrases, {regex_count} regex, {ctx_count} contextual"
            );
        }
        Err(e) => {
            eprintln!("✗ Invalid recipe: {e}");
            std::process::exit(1);
        }
    }

    Ok(())
}
