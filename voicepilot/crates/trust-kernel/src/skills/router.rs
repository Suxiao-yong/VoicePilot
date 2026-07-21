//! SkillRouter — V1.1 §5.1.
//!
//! W3b: deterministic keyword matching. Two strategies, tried in order:
//!   1. Two-way substring match against `intent_examples`
//!      (goal contains example OR example contains goal).
//!      Handles the "user goal literally restates an example" case.
//!   2. Substring match against curated `keywords`.
//!      Handles the "user goal shares a semantic keyword with the Skill"
//!      case (e.g. "整理" appears in both).
//!
//! First registered Skill to match wins. No LLM.
//!
//! Future W7 will add a lightweight intent classifier (BERT mini or
//! similar) running locally, but V1.1 §5.2 explicitly forbids LLM
//! routing.

use crate::skills::manifest::SkillManifest;

#[derive(Debug, Clone)]
pub enum RouteDecision {
    Skill(Box<SkillManifest>),
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
    ///
    /// Tries two-way substring match against `intent_examples` first,
    /// then substring match against curated `keywords`. First registered
    /// Skill to match wins. All matching is case-insensitive.
    pub fn route(&self, user_goal: &str) -> RouteDecision {
        let goal_lower = user_goal.to_lowercase();
        for skill in &self.skills {
            if self.matches_intent_examples(&goal_lower, skill)
                || self.matches_keywords(&goal_lower, skill)
            {
                return RouteDecision::Skill(Box::new(skill.clone()));
            }
        }
        RouteDecision::Planner
    }

    fn matches_intent_examples(&self, goal_lower: &str, skill: &SkillManifest) -> bool {
        for example in &skill.intent_examples {
            let example_lower = example.to_lowercase();
            if goal_lower.contains(&example_lower) || example_lower.contains(goal_lower) {
                return true;
            }
        }
        false
    }

    fn matches_keywords(&self, goal_lower: &str, skill: &SkillManifest) -> bool {
        for keyword in &skill.keywords {
            let keyword_lower = keyword.to_lowercase();
            if !keyword_lower.is_empty() && goal_lower.contains(&keyword_lower) {
                return true;
            }
        }
        false
    }
}
