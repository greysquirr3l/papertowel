use std::fs;
use std::path::{Path, PathBuf};

use tracing::{debug, instrument, warn};

use crate::domain::errors::PapertowelError;

use super::types::{LoadedRecipe, Recipe, RecipeSource};

/// Embedded built-in recipes.
mod builtin {
    pub const SLOP_VOCABULARY: &str = include_str!("../recipes/slop-vocabulary.toml");
    pub const COMMENT_PATTERNS: &str = include_str!("../recipes/comment-patterns.toml");
    pub const PHRASE_PATTERNS: &str = include_str!("../recipes/phrase-patterns.toml");
}

/// Discovers and loads recipes from all sources.
#[derive(Debug)]
pub struct RecipeLoader {
    repo_root: Option<PathBuf>,

    user_config_dir: Option<PathBuf>,

    include_builtin: bool,

    include_recipes: Vec<String>,

    exclude_recipes: Vec<String>,
}

impl RecipeLoader {
    pub fn new(repo_root: Option<PathBuf>) -> Self {
        Self {
            repo_root,
            user_config_dir: dirs::config_dir().map(|d| d.join("papertowel").join("recipes")),
            include_builtin: true,
            include_recipes: Vec::new(),
            exclude_recipes: Vec::new(),
        }
    }

    /// Disable built-in recipes.
    #[must_use]
    pub const fn without_builtin(mut self) -> Self {
        self.include_builtin = false;
        self
    }

    /// Only include specific recipes by name.
    #[must_use]
    pub fn include_only(mut self, names: Vec<String>) -> Self {
        self.include_recipes = names;
        self
    }

    /// Exclude specific recipes by name.
    #[must_use]
    pub fn exclude(mut self, names: Vec<String>) -> Self {
        self.exclude_recipes = names;
        self
    }

    /// Load all discovered recipes.
    #[instrument(skip(self))]
    pub fn load_all(&self) -> Result<Vec<LoadedRecipe>, PapertowelError> {
        let mut recipes = Vec::new();

        // Built-in recipes (lowest priority).
        if self.include_builtin {
            recipes.extend(Self::load_builtin());
        }

        // User global recipes (medium priority).
        if let Some(ref user_dir) = self.user_config_dir
            && user_dir.exists()
        {
            recipes.extend(Self::load_from_directory(
                user_dir,
                &RecipeSource::UserGlobal(user_dir.clone()),
            ));
        }

        // Repo local recipes (highest priority).
        if let Some(ref repo_root) = self.repo_root {
            let repo_recipes = repo_root.join(".papertowel").join("recipes");
            if repo_recipes.exists() {
                recipes.extend(Self::load_from_directory(
                    &repo_recipes,
                    &RecipeSource::RepoLocal(repo_recipes.clone()),
                ));
            }
        }

        // Apply include/exclude filters.
        let recipes = self.filter_recipes(recipes);

        debug!(count = recipes.len(), "loaded recipes");
        Ok(recipes)
    }

    /// Load built-in recipes embedded in the binary.
    fn load_builtin() -> Vec<LoadedRecipe> {
        let mut recipes = Vec::new();

        for (name, content) in [
            ("slop-vocabulary", builtin::SLOP_VOCABULARY),
            ("comment-patterns", builtin::COMMENT_PATTERNS),
            ("phrase-patterns", builtin::PHRASE_PATTERNS),
        ] {
            match toml::from_str::<Recipe>(content) {
                Ok(recipe) => {
                    recipes.push(LoadedRecipe {
                        recipe,
                        source: RecipeSource::Builtin,
                    });
                    debug!(name, "loaded builtin recipe");
                }
                Err(e) => {
                    warn!(name, error = %e, "failed to parse builtin recipe");
                }
            }
        }

        recipes
    }

    fn load_from_directory(dir: &Path, source_template: &RecipeSource) -> Vec<LoadedRecipe> {
        let mut recipes = Vec::new();

        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) => {
                warn!(path = %dir.display(), error = %e, "failed to read recipe directory");
                return recipes;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "toml") {
                match Self::load_recipe_file(&path) {
                    Ok(recipe) => {
                        let source = match &source_template {
                            RecipeSource::Builtin => RecipeSource::Builtin,
                            RecipeSource::UserGlobal(_) => RecipeSource::UserGlobal(path.clone()),
                            RecipeSource::RepoLocal(_) => RecipeSource::RepoLocal(path.clone()),
                        };
                        recipes.push(LoadedRecipe { recipe, source });
                        debug!(path = %path.display(), "loaded recipe file");
                    }
                    Err(e) => {
                        warn!(path = %path.display(), error = %e, "failed to load recipe");
                    }
                }
            }
        }

        recipes
    }

    /// Load a single recipe file.
    fn load_recipe_file(path: &Path) -> Result<Recipe, PapertowelError> {
        let content = fs::read_to_string(path).map_err(|e| PapertowelError::Io {
            path: path.to_owned(),
            source: e,
        })?;

        toml::from_str(&content).map_err(|e| {
            PapertowelError::Config(format!("invalid recipe {}: {}", path.display(), e))
        })
    }

    /// Apply include/exclude filters.
    fn filter_recipes(&self, recipes: Vec<LoadedRecipe>) -> Vec<LoadedRecipe> {
        recipes
            .into_iter()
            .filter(|r| {
                let name = &r.recipe.recipe.name;

                // Check excludes first.
                if self.exclude_recipes.contains(name) {
                    return false;
                }

                // If includes specified, only allow those.
                if !self.include_recipes.is_empty() {
                    return self.include_recipes.contains(name);
                }

                // Check if recipe itself is enabled.
                r.recipe.recipe.enabled
            })
            .collect()
    }
}

/// List available recipes without loading them fully.
#[instrument]
pub fn list_available_recipes(repo_root: Option<&Path>) -> Vec<(String, RecipeSource)> {
    let mut recipes = Vec::new();

    // Built-ins.
    recipes.push(("slop-vocabulary".to_owned(), RecipeSource::Builtin));
    recipes.push(("comment-patterns".to_owned(), RecipeSource::Builtin));
    recipes.push(("phrase-patterns".to_owned(), RecipeSource::Builtin));

    // User global.
    if let Some(config_dir) = dirs::config_dir() {
        let user_recipes = config_dir.join("papertowel").join("recipes");
        if let Ok(entries) = fs::read_dir(&user_recipes) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "toml")
                    && let Some(stem) = path.file_stem()
                {
                    recipes.push((
                        stem.to_string_lossy().into_owned(),
                        RecipeSource::UserGlobal(path),
                    ));
                }
            }
        }
    }

    // Repo local.
    if let Some(repo_root) = repo_root {
        let repo_recipes = repo_root.join(".papertowel").join("recipes");
        if let Ok(entries) = fs::read_dir(&repo_recipes) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "toml")
                    && let Some(stem) = path.file_stem()
                {
                    recipes.push((
                        stem.to_string_lossy().into_owned(),
                        RecipeSource::RepoLocal(path),
                    ));
                }
            }
        }
    }

    recipes
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn load_from_temp_directory() {
        let dir = TempDir::new().unwrap();
        let recipe_dir = dir.path().join(".papertowel").join("recipes");
        fs::create_dir_all(&recipe_dir).unwrap();

        let recipe_content = r#"
[recipe]
name = "test-recipe"
description = "A test recipe"

[patterns.words]
items = ["testword"]
"#;
        fs::write(recipe_dir.join("test.toml"), recipe_content).unwrap();

        let loader = RecipeLoader::new(Some(dir.path().to_owned())).without_builtin();
        let recipes = loader.load_all().unwrap();

        assert_eq!(recipes.len(), 1);
        assert_eq!(recipes[0].recipe.recipe.name, "test-recipe");
    }

    #[test]
    fn exclude_filter_works() {
        let loader = RecipeLoader::new(None).exclude(vec!["slop-vocabulary".to_owned()]);
        let recipes = loader.load_all().unwrap();

        assert!(
            recipes
                .iter()
                .all(|r| r.recipe.recipe.name != "slop-vocabulary")
        );
    }
}
