//! The analysis prompt, its JSON schema and lenient parsing of the response.

use anyhow::{Context, Result, anyhow};

use super::model::Analysis;

const SCHEMA_CORE: &str = r#"{
  "brief": {
    "why": "one sentence, max 25 words: the context or problem that motivates this PR",
    "what": "one sentence, max 25 words: the decision taken, i.e. what the PR changes",
    "impact": "one sentence, max 25 words: who or what is affected by the change"
  },
  "risk": {
    "complexity": {
      "score": 0-10,
      "reason": "max 20 words: how hard this change is to get right"
    },
    "worst_case": {
      "tier": "cosmetic | feature | core_flow | system_wide",
      "scenario": "max 20 words: the realistic worst thing that happens if this PR has a bug"
    },
    "audience": {
      "facing": "customer | partner | internal | developer",
      "reason": "max 20 words: who uses the code this PR changes"
    },
    "factors": [
      {
        "area": "short name of the risk (e.g. 'auth token handling')",
        "severity": "medium | high | critical",
        "description": "one sentence, max 25 words: what could go wrong",
        "files": ["paths involved"]
      }
    ]
  },
  "intent": {
    "stated_intent": "one sentence: what the author says the PR does (from title/description/commits)",
    "inferred_intent": "one sentence: what the code actually appears to do",
    "alignment": "one sentence: does the code match the stated intent? name any scope creep or missing piece",
    "decisions": [
      {
        "decision": "a design/implementation choice made in this PR",
        "rationale": "one short sentence: why it was made (say if inferred)",
        "alternatives": "one short sentence: plausible alternatives not chosen",
        "source": "description | commit | discussion | code (inferred)"
      }
    ],
    "open_questions": ["one line: a question that would clarify the context or the decision"]
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
        "You are a senior staff engineer helping a teammate quickly understand a GitHub pull request: \
why it exists, what it decides and what it affects. This is not a code review.\n\
Produce three analyses of the PR below:\n\
1. Risk: how risky is this change and which areas carry the risk.\n\
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
Be terse: short sentences, no filler, never restate the same point in several fields, respect the word limits. \
List at most 3 risk factors (medium severity or above), 4 decisions and 3 open questions; fewer is fine.\n\
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

    #[test]
    fn parses_legacy_analysis_with_summary() {
        let raw = r#"{"summary":"old","risk":{"rationale":"r","recommended_review_depth":"skim",
            "factors":[{"area":"a","severity":"high","mitigation":"m"}]}}"#;
        let a = parse(raw).unwrap();
        assert_eq!(a.summary, "old");
        assert!(a.brief.is_empty());
        assert_eq!(a.risk.factors.len(), 1);
    }

    #[test]
    fn rejects_non_json() {
        assert!(parse("sorry, I cannot do that").is_err());
    }
}
