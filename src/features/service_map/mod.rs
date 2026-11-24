//! Service maps generated from a folder of repos: services, their connections, and which ones
//! a PR impacts.

pub mod ai;
pub mod graph;
pub mod impact;
pub mod page;
pub mod scan;
pub mod state;
pub mod store;
pub mod view;

pub use graph::ServiceMap;
pub use impact::{prompt_section, resolve_impact};
pub use state::{Event, ServiceMaps};
