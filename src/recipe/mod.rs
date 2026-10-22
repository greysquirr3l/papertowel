//!
//! optionally replace AI-style phrasing. Recipes can come from three sources:
//!

pub mod loader;
pub mod matcher;
pub mod overrides;
pub mod scrubber;
pub mod types;

pub use loader::RecipeLoader;
pub use matcher::RecipeMatcher;
pub use overrides::{apply_finding_overrides, apply_recipe_overrides};
pub use scrubber::RecipeScrubber;
pub use types::{
    ContextualPattern, PhrasePatterns, Recipe, RecipePattern, RegexPattern, WordPatterns,
};
