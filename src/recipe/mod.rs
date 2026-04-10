//!
//! They can be:
//! - Built-in (embedded in the binary)

pub mod loader;
pub mod matcher;
pub mod types;

pub use loader::RecipeLoader;
pub use matcher::RecipeMatcher;
pub use types::{
    ContextualPattern, PhrasePatterns, Recipe, RecipePattern, RegexPattern, WordPatterns,
};
