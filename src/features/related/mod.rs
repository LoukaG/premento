//! Related PRs: links found in descriptions or suggested by the AI, grouped with their merge
//! order and drawn together in the sidebar.

pub mod ai;
pub mod body;
pub mod cache;
pub mod group;
pub mod model;
pub mod state;
pub mod view;

pub use model::{OutsidePr, PrState};
pub use state::{Entry, Event, Relations};
