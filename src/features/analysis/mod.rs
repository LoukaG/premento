//! The AI review of a PR: what the LLM returns, how it is scored, prompted, cached and shown.

pub mod cache;
pub mod model;
pub mod prompt;
pub mod scoring;
pub mod state;
pub mod views;

pub use model::{Analysis, RiskReview, ServiceImpact, StoredAnalysis};
pub use state::{Analyses, Event};
