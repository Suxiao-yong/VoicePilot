#![cfg(feature = "tauri")]

use voicepilot_ui::commands::RouteTextResult;
use voicepilot_ui::state::AppState;

#[test]
fn route_text_returns_routed_when_skill_keyword_matches() {
    let state = AppState::new_in_memory().unwrap();
    // `files_organize_manifest` 的 keywords 是中文("整理"、"归档"、
    // "移动文件"、"下载目录"),所以测试输入也用中文才能触发匹配。
    let result = voicepilot_ui::commands::route_text(&state, "整理下载目录").unwrap();
    assert!(matches!(
        result,
        RouteTextResult::Routed { ref skill_id } if skill_id == "files.organize"
    ));
}

#[test]
fn route_text_returns_unmatched_when_no_keyword() {
    let state = AppState::new_in_memory().unwrap();
    let result = voicepilot_ui::commands::route_text(&state, "hello world").unwrap();
    assert!(matches!(result, RouteTextResult::Unmatched { .. }));
}

#[test]
fn route_text_returns_empty_for_whitespace() {
    let state = AppState::new_in_memory().unwrap();
    let result = voicepilot_ui::commands::route_text(&state, "   ").unwrap();
    assert!(matches!(result, RouteTextResult::Empty));
}
