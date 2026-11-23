//! The attraction-inc service map: services, their connections, and which ones a PR impacts.

pub mod graph;
pub mod impact;
pub mod view;

pub use impact::{applies, node, prompt_section, resolve_impact};
