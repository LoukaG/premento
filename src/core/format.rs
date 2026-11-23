//! Text formatting shared by the views.

use chrono::{DateTime, Utc};

/// `core_flow` becomes `Core flow`.
pub fn humanize(s: &str) -> String {
    let s = s.trim().replace(['_', '-'], " ");
    let mut chars = s.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

pub fn ago_str(rfc3339: &str) -> String {
    DateTime::parse_from_rfc3339(rfc3339)
        .map(|t| ago(t.with_timezone(&Utc)))
        .unwrap_or_else(|_| rfc3339.to_string())
}

pub fn ago(t: DateTime<Utc>) -> String {
    let secs = (Utc::now() - t).num_seconds().max(0);
    match secs {
        0..60 => "just now".into(),
        60..3600 => format!("{}m ago", secs / 60),
        3600..86400 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86400),
    }
}
