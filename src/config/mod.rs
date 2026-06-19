//! GitVersion configuration: data model, workflow defaults, effective config, loader.

pub mod defaults;
pub mod effective;
pub mod loader;
pub mod model;

pub use model::*;
