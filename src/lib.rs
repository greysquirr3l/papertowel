#![expect(
    clippy::multiple_crate_versions,
    reason = "transitive dependency graph currently includes duplicate versions"
)]

#[doc(hidden)]
pub mod cleanup;
#[doc(hidden)]
pub mod cli;
#[doc(hidden)]
pub mod config;
pub mod detection;
#[doc(hidden)]
pub mod domain;
#[doc(hidden)]
pub mod learning;
#[doc(hidden)]
pub mod profile;
pub mod recipe;
pub mod scrubber;
#[doc(hidden)]
pub mod wringer;
