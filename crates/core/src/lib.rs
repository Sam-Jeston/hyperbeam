//! Shared domain types for Hyperbeam.

pub mod grants;
pub mod protocol;
pub mod workflow;

pub use grants::{Grant, GrantSet, RepoScope, Role};
