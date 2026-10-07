//! 桌宠窗口命令 + 鼠标穿透看门狗(桌宠化改造)。
//!
//! - `show_main_window` / `hide_main_window`:主界面显隐(桌宠右键菜单"打开主界面")
//! - `exit_app`:唯一退出路径 —— main 关闭只是隐藏,进程随 pet 常驻
//! - `pet_set_bubble_visible`:前端把确认气泡可见性同步给看门狗热区判定
//! - `spawn_pet_cursor_watchdog`:~100ms 轮询全局光标 vs 热区,动态切换
//!   `set_ignore_cursor_events`,让 pet 窗口的透明区域不挡桌面点击。
//!
//! 热区几何为逻辑坐标,须与 PetWindow.tsx 的 CSS 布局保持一致:
//!   pet 窗口 300×360;兽身渲染在底部中央 (50,140)-(250,360);
//!   确认气泡顶部居中,约 (20,12)-(280,130)。

use std::sync::atomic::Ordering;
use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::state::AppState;

/// pet 窗口逻辑尺寸(与 tauri.conf.json 保持一致)。
const PET_W: f64 = 300.0;
const PET_H: f64 = 360.0;
/// 兽身热区 (x0, y0, x1, y1) 逻辑坐标:底部中央谛听渲染区 + 少量容差。
const BEAST_RECT: (f64, f64, f64, f64) = (45.0, 135.0, 255.0, PET_H);
/// 气泡热区:顶部居中确认气泡;仅 bubble_visible = true 时纳入交互。
const BUBBLE_RECT: (f64, f64, f64, f64) = (10.0, 5.0, 290.0, 135.0);

/// 显示并聚焦主窗口(桌宠右键菜单 / 气泡"去主界面")。
#[tauri::command]
pub fn show_main_window(app: AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.unminimize();
        win.show().map_err(|e| e.to_string())?;
        let _ = win.set_focus();
    }
    Ok(())
}

/// 隐藏主窗口(主界面收起到后台,进程继续驻留)。
#[tauri::command]
pub fn hide_main_window(app: AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("main") {
        win.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 唯一退出路径:桌宠右键菜单"退出程序"。不经过这里进程不退出。
#[tauri::command]
pub fn exit_app(app: AppHandle) {
    app.exit(0);
}

// 直调 user32 重申置顶。tao 的 `set_always_on_top` 带 diff 检查
// (window_state.rs apply_diff):已是 HWND_TOPMOST 时直接 return,
// 防不了其他置顶窗口(聊天工具/游戏覆盖层)把自己抬到置顶带顶端、
// 把桌宠压到 Z 序带底部的场景。BongoCat 的做法是每 16ms 无条件
// SetWindowPos(HWND_TOPMOST),这里沿用同一做法(频率降到 1s 一次)。
#[link(name = "user32")]
unsafe extern "system" {
    fn SetWindowPos(hwnd: isize, after: isize, x: i32, y: i32, cx: i32, cy: i32, flags: u32)
    -> i32;
}

const HWND_TOPMOST: isize = -1;
/// SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS
const SWP_REASSERT_TOPMOST: u32 = 0x0002 | 0x0001 | 0x0010 | 0x4000;

/// PetWindow 同步确认气泡可见性给穿透看门狗。
#[tauri::command]
pub fn pet_set_bubble_visible(state: tauri::State<'_, AppState>, visible: bool) {
    state.bubble_visible.store(visible, Ordering::SeqCst);
}

/// 诊断探针:前端把存活/异常状态打到 stderr(桌宠白屏排查)。
#[tauri::command]
pub fn pet_probe(status: String) {
    eprintln!("[voice] pet probe: {}", status);
}

/// 启动穿透看门狗后台线程(app setup 里调用)。
///
/// 初始即置 `set_ignore_cursor_events(true)`(透明区放行点击),之后每 100ms
/// 计算光标是否落在兽身/气泡热区内,只在状态变化时调用 Win32 切换,
/// 避免每 tick 都打系统调用。getter 不存在(tauri 2.11 无
/// `is_ignore_cursor_events`),用本地布尔跟踪当前状态。
pub fn spawn_pet_cursor_watchdog(app: AppHandle) {
    std::thread::spawn(move || {
        // 【关键时序】不能在 setup 里同步 set_ignore_cursor_events(true):
        // 该调用给宿主窗口加 WS_EX_LAYERED|WS_EX_TRANSPARENT 样式,若赶在
        // WebView2 合成器附着到 HWND 之前执行,实测整窗像素永不输出 ——
        // JS 照跑、rAF 照跳、探针全绿,但屏幕上什么都看不到。
        // BongoCat 的同功能是从前端(watch immediate)在页面加载后才调,
        // 天然避开了这个竞态。这里统一推迟 1.5s,让首绘先完成。
        // (代价:头 1.5s 整窗可交互,可能挡住底下桌面点击,可接受)
        std::thread::sleep(Duration::from_millis(1500));
        if let Some(pet) = app.get_webview_window("pet") {
            let _ = pet.set_ignore_cursor_events(true);
        }
        let mut ignoring = true;
        let mut tick: u32 = 0;
        loop {
            std::thread::sleep(Duration::from_millis(100));
            tick = tick.wrapping_add(1);
            // 每 ~1s 重申一次置顶:直调 Win32(见上方说明),把桌宠抬回
            // 置顶带顶端。SWP_ASYNCWINDOWPOS 避免跨线程等 UI 线程消息队列。
            if tick % 10 == 0 {
                if let Some(pet) = app.get_webview_window("pet") {
                    if let Ok(hwnd) = pet.hwnd() {
                        unsafe {
                            SetWindowPos(
                                hwnd.0 as isize,
                                HWND_TOPMOST,
                                0,
                                0,
                                0,
                                0,
                                SWP_REASSERT_TOPMOST,
                            );
                        }
                    }
                }
            }
            let should_ignore = match compute_interactive(&app) {
                Some(interactive) => !interactive,
                None => continue,
            };
            if should_ignore != ignoring {
                if let Some(pet) = app.get_webview_window("pet") {
                    if pet.set_ignore_cursor_events(should_ignore).is_ok() {
                        ignoring = should_ignore;
                        // 诊断:热区切换留痕(定位"桌宠点不动/穿透不生效")
                        eprintln!(
                            "[voice] pet hotzone -> {}",
                            if should_ignore {
                                "pass-through"
                            } else {
                                "interactive"
                            }
                        );
                    }
                }
            }
        }
    });
}

/// 计算"光标是否落在 pet 窗口可交互热区内"。窗口不存在或查询失败返回 None
/// (维持现状,不抖动)。
fn compute_interactive(app: &AppHandle) -> Option<bool> {
    let pet = app.get_webview_window("pet")?;
    let bubble_visible = app
        .try_state::<AppState>()
        .map(|s| s.bubble_visible.load(Ordering::SeqCst))
        .unwrap_or(false);

    // 光标(物理)相对窗口原点(物理)→ 除以缩放得逻辑坐标
    let cursor = pet.cursor_position().ok()?;
    let pos = pet.outer_position().ok()?;
    let scale = pet.scale_factor().ok()?;
    if scale <= 0.0 {
        return Some(false);
    }
    let lx = (cursor.x - pos.x as f64) / scale;
    let ly = (cursor.y - pos.y as f64) / scale;

    // 光标不在窗口内 → 必定穿透
    if !(0.0..=PET_W).contains(&lx) || !(0.0..=PET_H).contains(&ly) {
        return Some(false);
    }

    let in_rect = |r: (f64, f64, f64, f64)| lx >= r.0 && lx <= r.2 && ly >= r.1 && ly <= r.3;
    let interactive = in_rect(BEAST_RECT) || (bubble_visible && in_rect(BUBBLE_RECT));
    Some(interactive)
}
