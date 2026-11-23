//! The review analysis: prompt, JSON schema, parsing and on-disk cache.

use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::config::app_cache_dir;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Analysis {
    pub summary: String,
    pub risk: RiskReview,
    pub intent: IntentLog,
    /// Generic impact analysis (PRs outside attraction-inc).
    pub impact: ImpactAnalysis,
    /// Impact on the attraction-inc service map (attraction-inc PRs).
    pub service_impact: Vec<ServiceImpact>,
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
    pub rationale: String,
    pub recommended_review_depth: String,
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

/// Share of the impact that applies even to a trivial change; complexity adds the rest.
pub const BASE_LIKELIHOOD: f32 = 0.6;

impl WorstCase {
    /// Impact of the worst case, 0-10. `None` if the tier is not recognized.
    pub fn impact(&self) -> Option<f32> {
        let t = self.tier.to_ascii_lowercase();
        if t.contains("system") || t.contains("crash") || t.contains("outage") {
            Some(10.0)
        } else if t.contains("core") {
            Some(7.0)
        } else if t.contains("feature") {
            Some(3.0)
        } else if t.contains("cosmetic") {
            Some(1.0)
        } else {
            None
        }
    }
}

impl Audience {
    /// Multiplier applied to the impact. `None` if the audience is not recognized.
    pub fn factor(&self) -> Option<f32> {
        let f = self.facing.to_ascii_lowercase();
        if f.contains("customer") || f.contains("public") {
            Some(1.0)
        } else if f.contains("partner") {
            Some(0.85)
        } else if f.contains("internal") || f.contains("admin") || f.contains("staff") {
            Some(0.6)
        } else if f.contains("developer") || f.contains("dev") {
            Some(0.3)
        } else {
            None
        }
    }
}

impl RiskReview {
    /// Sets `score` and `level` from complexity, worst case and audience:
    /// `score = worst_case × audience × (0.6 + 0.4 × complexity / 10)`.
    /// Complexity scales the impact but never lifts it: a change that can only break one feature
    /// stays low however complex it is.
    /// Keeps the existing values when a dimension is missing (old cached analyses).
    pub fn compute(&mut self) {
        let (Some(impact), Some(factor)) = (self.worst_case.impact(), self.audience.factor()) else {
            return;
        };
        let complexity = self.complexity.score.clamp(0.0, 10.0);
        let likelihood = BASE_LIKELIHOOD + (1.0 - BASE_LIKELIHOOD) * complexity / 10.0;
        let score = impact * factor * likelihood;
        self.score = (score * 10.0).round() / 10.0;
        self.level = level_for(self.score).into();
    }

    pub fn has_breakdown(&self) -> bool {
        self.worst_case.impact().is_some() && self.audience.factor().is_some()
    }
}

pub fn level_for(score: f32) -> &'static str {
    match score {
        s if s >= 8.0 => "critical",
        s if s >= 6.0 => "high",
        s if s >= 3.5 => "medium",
        _ => "low",
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RiskFactor {
    pub area: String,
    pub severity: String,
    pub description: String,
    pub files: Vec<String>,
    pub mitigation: String,
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

const SCHEMA_CORE: &str = r#"{
  "summary": "2-4 sentence plain-language summary of what this PR does",
  "risk": {
    "complexity": {
      "score": 0-10,
      "reason": "how hard this change is to get right: size, logic, concurrency, state, migrations, number of moving parts"
    },
    "worst_case": {
      "tier": "cosmetic | feature | core_flow | system_wide",
      "scenario": "the realistic worst thing that happens if this PR has a bug"
    },
    "audience": {
      "facing": "customer | partner | internal | developer",
      "reason": "who uses the code this PR changes"
    },
    "rationale": "why this overall risk, in terms of the three dimensions above",
    "recommended_review_depth": "e.g. 'quick skim', 'careful line-by-line', 'needs domain expert + tests run locally'",
    "factors": [
      {
        "area": "short name of the risk (e.g. 'auth token handling')",
        "severity": "low | medium | high | critical",
        "description": "what could go wrong and why",
        "files": ["paths involved"],
        "mitigation": "what the reviewer should verify or ask for"
      }
    ]
  },
  "intent": {
    "stated_intent": "what the author says the PR does (from title/description/commits)",
    "inferred_intent": "what the code actually appears to do",
    "alignment": "does the code match the stated intent? call out scope creep, missing pieces or unrelated changes",
    "decisions": [
      {
        "decision": "a design/implementation choice made in this PR",
        "rationale": "why it was made (quote the source if explicit, otherwise say it is inferred)",
        "alternatives": "plausible alternatives that were not chosen",
        "source": "description | commit | discussion | code (inferred)"
      }
    ],
    "open_questions": ["questions the reviewer should ask the author"]
  },"#;

const SCHEMA_IMPACT: &str = r#"
  "impact": {
    "affected_components": [{ "name": "module/service/API", "description": "how it is affected" }],
    "breaking_changes": ["API/behaviour/config changes that can break callers"],
    "downstream_effects": ["consumers, jobs, clients or teams affected"],
    "data_and_migrations": ["schema, data, storage or migration concerns"],
    "security": ["security-relevant impact"],
    "performance": ["performance-relevant impact"],
    "test_coverage": "assessment of whether the changed behaviour is tested",
    "suggested_tests": ["tests that should exist or be run"]
  }
}"#;

const SCHEMA_SERVICES: &str = r#"
  "service_impact": [
    {
      "service": "service id from the SERVICES list (exactly as written)",
      "level": "direct | indirect",
      "severity": "low | medium | high | critical",
      "reason": "one or two sentences: what changes for this service and through which connection"
    }
  ]
}"#;

/// `service_map`: architecture description for PRs of a known system; switches the third
/// analysis from a generic impact analysis to a per-service impact map.
pub fn build_prompt(pr_context: &str, service_map: Option<&str>) -> String {
    let (impact_task, schema_tail, architecture) = match service_map {
        Some(map) => (
            "3. Service Impact Map: using the ARCHITECTURE below, list every service this PR impacts. \
`direct` = its code, config, build or deployment changes; `indirect` = it is affected through a connection \
(API contract, shared package, data, event, deploy order). Only use service ids from the list; omit services \
that are not impacted.",
            SCHEMA_SERVICES,
            format!("==================== ARCHITECTURE ====================\n{map}\n"),
        ),
        None => (
            "3. Impact Analysis: what else in the system is affected (components, breaking changes, \
downstream consumers, data, security, performance, tests).",
            SCHEMA_IMPACT,
            String::new(),
        ),
    };
    format!(
        "You are a senior staff engineer helping a teammate review a GitHub pull request.\n\
Produce three analyses of the PR below:\n\
1. Risk-Based Review: how risky is this change, which areas carry the risk, and how deep the review must be.\n\
2. Intent & Decision Log: what the PR is meant to do, whether the code matches, and the design decisions made (with rationale and alternatives).\n\
{impact_task}\n\n\
Rate the risk on three dimensions (the app computes the overall score from them, do not return one):\n\
- complexity: 0 = trivial (typo, config value), 5 = moderate logic change, 10 = intricate (concurrency, state machines, migrations, many coupled parts).\n\
- worst_case.tier: what breaks if the change is wrong. `cosmetic` = visual or text glitch, nothing stops working; \
`feature` = one specific feature stops working, the rest of the product is fine; `core_flow` = a core flow breaks \
(checkout, payment, orders, search, data sync) or some data is corrupted; `system_wide` = login/auth breaks, \
the whole app or service crashes or goes down, data is lost, or a security hole opens. Judge by what the changed code \
can reach, not by the size of the diff: a one-line change to auth middleware is `system_wide`.\n\
- audience.facing: who is exposed. `customer` = end customers or public users; `partner` = B2B partners, distributors \
or public API consumers; `internal` = admin, back-office or staff-only tools; `developer` = CI, build, tests, docs or \
dev tooling only. If several apply, pick the most exposed one.\n\n\
Be specific and grounded in the diff; reference file paths and function names. Do not invent facts — if something is inferred, say so. \
Do not run tools or modify files; everything you need is below.\n\n\
Respond with ONLY a single JSON object (no markdown fences, no prose before or after) matching this shape:\n\
{SCHEMA_CORE}{schema_tail}\n\n\
{architecture}\
==================== PULL REQUEST ====================\n\
{pr_context}"
    )
}

/// Extracts the JSON object from an LLM response that may include fences or chatter.
pub fn parse(raw: &str) -> Result<Analysis> {
    let start = raw.find('{').ok_or_else(|| anyhow!("no JSON object in response"))?;
    let end = raw.rfind('}').ok_or_else(|| anyhow!("no JSON object in response"))?;
    if end < start {
        return Err(anyhow!("malformed JSON in response"));
    }
    let mut analysis: Analysis =
        serde_json::from_str(&raw[start..=end]).context("response JSON did not match the schema")?;
    analysis.risk.compute();
    Ok(analysis)
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

fn cache_path(pr_key: &str) -> PathBuf {
    let safe: String = pr_key
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' { c } else { '_' })
        .collect();
    app_cache_dir().join("analyses").join(format!("{safe}.json"))
}

pub fn load_cached(pr_key: &str) -> Option<StoredAnalysis> {
    let text = std::fs::read_to_string(cache_path(pr_key)).ok()?;
    let mut stored: StoredAnalysis = serde_json::from_str(&text).ok()?;
    // Re-score with the current formula; the dimensions are stored, the score is derived.
    if let Some(a) = &mut stored.analysis {
        a.risk.compute();
    }
    Some(stored)
}

pub fn save_cached(pr_key: &str, stored: &StoredAnalysis) -> Result<()> {
    let path = cache_path(pr_key);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(stored)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fenced_partial_json() {
        let raw = "Here you go:\n```json\n{\"summary\":\"s\",\"risk\":{\"level\":\"high\",\"score\":7}}\n```";
        let a = parse(raw).unwrap();
        assert_eq!(a.risk.level, "high");
        assert!(a.impact.breaking_changes.is_empty());
    }

    fn risk(complexity: f32, tier: &str, facing: &str) -> RiskReview {
        let mut r = RiskReview {
            complexity: Complexity { score: complexity, reason: String::new() },
            worst_case: WorstCase { tier: tier.into(), scenario: String::new() },
            audience: Audience { facing: facing.into(), reason: String::new() },
            ..Default::default()
        };
        r.compute();
        r
    }

    #[test]
    fn worst_case_dominates_the_score() {
        // A small change that can break login for customers outranks a large one that breaks one feature.
        let login = risk(2.0, "system_wide", "customer");
        let feature = risk(8.0, "feature", "customer");
        assert!(login.score > feature.score, "{} vs {}", login.score, feature.score);
        assert_eq!(login.level, "high");
        assert_eq!(risk(7.0, "system_wide", "customer").level, "critical");
        assert_eq!(risk(1.0, "cosmetic", "customer").level, "low");
    }

    #[test]
    fn feature_only_change_stays_low() {
        // A complex customer-facing change whose worst case is one feature (e.g. a preview) breaking.
        assert_eq!(risk(7.0, "feature", "customer").level, "low");
        assert_eq!(risk(10.0, "feature", "customer").level, "low");
        assert_eq!(risk(5.0, "core_flow", "customer").level, "medium");
    }

    #[test]
    fn customer_facing_is_riskier_than_internal() {
        let customer = risk(5.0, "core_flow", "customer");
        let admin = risk(5.0, "core_flow", "internal");
        let dev = risk(5.0, "core_flow", "developer");
        assert!(customer.score > admin.score && admin.score > dev.score);
    }

    #[test]
    fn keeps_llm_score_without_breakdown() {
        let a = parse(r#"{"risk":{"level":"high","score":7}}"#).unwrap();
        assert_eq!((a.risk.level.as_str(), a.risk.score), ("high", 7.0));
        let a = parse(r#"{"risk":{"complexity":{"score":"6/10"},"worst_case":{"tier":"core_flow"},"audience":{"facing":"customer"}}}"#).unwrap();
        assert_eq!((a.risk.level.as_str(), a.risk.score), ("medium", 5.9));
    }

    #[test]
    fn rejects_non_json() {
        assert!(parse("sorry, I cannot do that").is_err());
    }
}
