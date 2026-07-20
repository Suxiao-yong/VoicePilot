//! SkillRouter — V1.1 §5.1.
//!
//! W3b: deterministic keyword matching against `intent_examples`.
//! Future W7 will add a lightweight intent classifier (BERT mini or
//! similar), but V1.1 §5.2 explicitly says Skill routing must NOT
//! call an LLM — so even W7's classifier runs locally.
//!
//! Matching rule: case-insensitive substring match. First registered
//! Skill whose intent_examples contains a substring of the user_goal
//! wins (first-match-wins).

use crate::skills::manifest::SkillManifest;

#[derive(Debug, Clone)]
pub enum RouteDecision {
    Skill(SkillManifest),
    Planner,
}

#[derive(Debug, Clone, Default)]
pub struct SkillRouter {
    skills: Vec<SkillManifest>,
}

impl SkillRouter {
    pub fn new() -> Self {
        Self { skills: Vec::new() }
    }

    pub fn register(&mut self, manifest: SkillManifest) {
        self.skills.push(manifest);
    }

    /// Route a user goal to a Skill or to the Planner.
    /// Matching is case-insensitive substring over `intent_examples`.
    pub fn route(&self, user_goal: &str) -> RouteDecision {
        let goal_lower = user_goal.to_lowercase();
        for skill in &self.skills {
            for example in &skill.intent_examples {
                let example_lower = example.to_lowercase();
                // Two-way substring match: goal contains example OR example contains goal.
                // The two-way handles both "short goal matches long example" and
                // "long goal contains short example keyword".
                if goal_lower.contains(&example_lower) || example_lower.contains(&goal_lower) {
                    return RouteDecision::Skill(skill.clone());
                }
                // Partial keyword match: any 2-char (non-whitespace) substring of
                // the example also appears in the goal. This catches shared keywords
                // like "整理" / "下载" without requiring the full example sentence to
                // be present. Windows containing whitespace are skipped to avoid
                // false positives like " P" or "F ".
                let example_chars: Vec<char> = example_lower.chars().collect();
                for window in example_chars.windows(2) {
                    if window.iter().any(|c| c.is_whitespace()) {
                        continue;
                    }
                    let window_str: String = window.iter().collect();
                    if goal_lower.contains(&window_str) {
                        return RouteDecision::Skill(skill.clone());
                    }
                }
            }
        }
        RouteDecision::Planner
    }
}
