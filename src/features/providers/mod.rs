//! Local LLM CLIs (Claude Code, Codex, …): their config, running them, and picking one.

pub mod config;
pub mod picker;
pub mod runner;

pub use config::ProviderConfig;
pub use picker::{ProviderPicker, Selection};
