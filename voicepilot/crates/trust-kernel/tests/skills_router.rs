use trust_kernel::skills::manifest::files_organize_manifest;
use trust_kernel::skills::router::{RouteDecision, SkillRouter};

#[test]
fn router_returns_skill_when_intent_matches() {
    let mut router = SkillRouter::new();
    router.register(files_organize_manifest());

    let decision = router.route("把下载目录里的 PDF 移到论文文件夹");
    assert!(matches!(decision, RouteDecision::Skill(ref s) if s.id == "files.organize"));
}

#[test]
fn router_returns_skill_for_partial_keyword_match() {
    let mut router = SkillRouter::new();
    router.register(files_organize_manifest());

    // "整理" appears in intent_example "整理今天下载的文档".
    let decision = router.route("请整理一下我的下载文件夹");
    assert!(matches!(decision, RouteDecision::Skill(_)));
}

#[test]
fn router_returns_planner_when_no_skill_matches() {
    let mut router = SkillRouter::new();
    router.register(files_organize_manifest());

    let decision = router.route("帮我研究 Rust async runtime 并写一份对比报告");
    assert!(matches!(decision, RouteDecision::Planner));
}

#[test]
fn router_empty_returns_planner() {
    let router = SkillRouter::new();
    let decision = router.route("anything");
    assert!(matches!(decision, RouteDecision::Planner));
}

#[test]
fn router_first_match_wins_when_multiple_skills_match() {
    let mut router = SkillRouter::new();
    let mut skill_a = files_organize_manifest();
    skill_a.id = "files.organize.a".to_string();
    let mut skill_b = files_organize_manifest();
    skill_b.id = "files.organize.b".to_string();
    // Register in order A, B — A should win on first-match-wins.
    router.register(skill_a);
    router.register(skill_b);

    let decision = router.route("整理下载目录");
    if let RouteDecision::Skill(s) = decision {
        assert_eq!(s.id, "files.organize.a");
    } else {
        panic!("expected Skill decision");
    }
}

#[test]
fn router_does_not_match_unrelated_common_bigrams() {
    let mut router = SkillRouter::new();
    router.register(files_organize_manifest());

    // Common Chinese bigrams that appear in files.organize's intent_examples
    // but should NOT trigger routing for unrelated goals.
    let unrelated_goals = [
        "今天吃什么",
        "今天天气怎么样",
        "我要写一篇论文",
        "请帮我写一份技术文档",
        "项目进度怎么样",
    ];
    for goal in &unrelated_goals {
        let decision = router.route(goal);
        assert!(
            matches!(decision, RouteDecision::Planner),
            "goal {:?} should route to Planner, not files.organize",
            goal
        );
    }
}
