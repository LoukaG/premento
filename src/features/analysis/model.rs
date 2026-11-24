//! The review analysis returned by the LLM, and how it is stored.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Analysis {
    /// Why / what / impact, one line each.
    pub brief: Brief,
    /// Free-text summary from analyses made before `brief` existed; shown as a fallback.
    pub summary: String,
    pub risk: RiskReview,
    pub intent: IntentLog,
    /// Generic impact analysis (PRs whose repo is on no service map).
    pub impact: ImpactAnalysis,
    /// Impact on the service map that lists the PR's repo.
    pub service_impact: Vec<ServiceImpact>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Brief {
    /// The context or problem behind the PR.
    pub why: String,
    /// The decision taken: what the PR changes.
    pub what: String,
    /// Who or what is affected.
    pub impact: String,
}

impl Brief {
    pub fn is_empty(&self) -> bool {
        self.why.is_empty() && self.what.is_empty() && self.impact.is_empty()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ServiceImpact {
    /// Service id from the service map.
    pub service: String,
    /// "direct" (its code/config changes) or "indirect" (affected through a connection).
    pub level: String,
    pub severity: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RiskReview {
    /// Computed from the three dimensions below (see [`RiskReview::compute`]).
    pub level: String,
    #[serde(deserialize_with = "lenient_number")]
    pub score: f32,
    pub complexity: Complexity,
    pub worst_case: WorstCase,
    pub audience: Audience,
    pub factors: Vec<RiskFactor>,
}

/// How hard the change is to get right: a proxy for the likelihood of a bug.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Complexity {
    #[serde(deserialize_with = "lenient_number")]
    pub score: f32,
    pub reason: String,
}

/// What breaks if this PR has a bug.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WorstCase {
    /// cosmetic | feature | core_flow | system_wide
    pub tier: String,
    pub scenario: String,
}

/// Who is exposed to the change.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Audience {
    /// customer | partner | internal | developer
    pub facing: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RiskFactor {
    pub area: String,
    pub severity: String,
    pub description: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct IntentLog {
    pub stated_intent: String,
    pub inferred_intent: String,
    pub alignment: String,
    pub decisions: Vec<Decision>,
    pub open_questions: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Decision {
    pub decision: String,
    pub rationale: String,
    pub alternatives: String,
    pub source: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ImpactAnalysis {
    pub affected_components: Vec<Component>,
    pub breaking_changes: Vec<String>,
    pub downstream_effects: Vec<String>,
    pub data_and_migrations: Vec<String>,
    pub security: Vec<String>,
    pub performance: Vec<String>,
    pub test_coverage: String,
    pub suggested_tests: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Component {
    pub name: String,
    pub description: String,
}

/// LLMs sometimes return `"7"` or `"7/10"` instead of `7`.
fn lenient_number<'de, D: serde::Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(match v {
        serde_json::Value::Number(n) => n.as_f64().unwrap_or(0.0) as f32,
        serde_json::Value::String(s) => s
            .split('/')
            .next()
            .and_then(|p| p.trim().parse().ok())
            .unwrap_or(0.0),
        _ => 0.0,
    })
}

/// An analysis result as persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredAnalysis {
    pub provider: String,
    pub head_sha: String,
    pub created_at: i64,
    pub duration_secs: u64,
    pub analysis: Option<Analysis>,
    pub parse_error: Option<String>,
    pub raw: String,
}

impl StoredAnalysis {
    /// True when the PR has moved to another commit since this analysis. `current_head` is the
    /// PR's head commit, `None` when unknown (then nothing is claimed).
    pub fn is_outdated(&self, current_head: Option<&str>) -> bool {
        current_head.is_some_and(|head| !self.head_sha.is_empty() && head != self.head_sha)
    }
}
