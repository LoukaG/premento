//! Risk score: computed from complexity, worst case and audience.

use super::model::{Audience, RiskReview, WorstCase};

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

#[cfg(test)]
mod tests {
    use super::super::model::Complexity;
    use super::super::prompt::parse;
    use super::*;

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
}
