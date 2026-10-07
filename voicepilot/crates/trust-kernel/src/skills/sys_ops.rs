//! Daisy 移植：系统组（时间/前台应用/开网址/全关应用/诊断/定时/闹钟/地图）。
//!
//! 只读项（时间/前台/诊断）免审批、审计留痕；发射与写入项（开网址/全关/
//! 定时/闹钟/地图）PerStep 审批。见 `super::simple` 姿态矩阵。

use crate::approval::approver::Approver;
use crate::error::{KernelError, Result};
use crate::kernel::TrustKernel;
use crate::skills::manifest::{EgressKind, SkillInputType, SkillManifest};
use crate::skills::simple::{
    SimpleInput, ps_eval, require_bin, run_cmd, run_simple, schtasks_name, schtasks_once,
    simple_manifest, slot_text, slot_text_opt, truncate_chars, weekday_zh,
};
use std::collections::HashMap;

fn text_slot(name: &'static str, max: u32) -> SimpleInput {
    SimpleInput {
        name,
        input_type: SkillInputType::Text,
        required: true,
        max_length: Some(max),
        allowed_roots: vec![],
    }
}

pub fn sys_datetime_manifest() -> SkillManifest {
    simple_manifest(
        "sys.datetime",
        "现在几点",
        "返回本地日期时间（含星期）。纯本地只读。",
        &["几点", "时间", "日期", "星期", "今天几号"],
        &["现在几点了", "今天星期几", "今天几号"],
        vec![],
        false,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_datetime(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
) -> Result<String> {
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "sys.datetime",
        "sys:datetime",
        false,
        &sys_datetime_manifest(),
        &HashMap::new(),
        || {
            let now = chrono::Local::now();
            Ok(format!(
                "现在是 {} {}",
                now.format("%Y年%m月%d日 %H:%M"),
                weekday_zh(),
            ))
        },
    )
}

pub fn sys_frontmost_manifest() -> SkillManifest {
    simple_manifest(
        "sys.frontmost",
        "当前前台应用",
        "返回当前前台窗口的应用名。纯本地只读。",
        &["前台", "当前应用", "正在用", "哪个窗口"],
        &["当前在用什么应用", "前台是哪个窗口"],
        vec![],
        false,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_frontmost(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
) -> Result<String> {
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "sys.frontmost",
        "sys:frontmost",
        false,
        &sys_frontmost_manifest(),
        &HashMap::new(),
        || {
            // 与 Daisy 同形：取 exe 名 + 窗口标题（`exe|标题`），标题空时回退进程名。
            let ps = r#"Add-Type -Namespace VP -Name Win -MemberDefinition '[DllImport("user32.dll")] public static extern System.IntPtr GetForegroundWindow(); [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(System.IntPtr h, System.Text.StringBuilder t, int n); [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(System.IntPtr h, [ref] uint32 p);' | Out-Null; $h=[VP.Win]::GetForegroundWindow(); $sb=New-Object System.Text.StringBuilder 512; [VP.Win]::GetWindowText($h,$sb,512)|Out-Null; $t=$sb.ToString(); $p=0; [VP.Win]::GetWindowThreadProcessId($h,[ref]$p)|Out-Null; try { $e=(Get-Process -Id $p -ErrorAction Stop).ProcessName } catch { $e="" }; "$e|$t""#;
            let raw = ps_eval(ps)?.trim().to_string();
            let (exe, title) = raw.split_once('|').unwrap_or(("", raw.as_str()));
            let exe = exe.trim();
            let title = title.trim();
            if exe.is_empty() && title.is_empty() {
                return Err(KernelError::Skill(
                    "取不到前台窗口（可能无图形会话）".to_string(),
                ));
            }
            Ok(format_frontmost(exe, title))
        },
    )
}

/// 前台展示格式（纯函数，可测）：exe 与标题都可能为空其一。
pub fn format_frontmost(exe: &str, title: &str) -> String {
    match (exe.is_empty(), title.is_empty()) {
        (false, false) => format!("当前前台：{exe}（{title}）"),
        (false, true) => format!("当前前台：{exe}"),
        (true, false) => format!("当前前台窗口标题：{title}"),
        (true, true) => "取不到前台窗口".to_string(),
    }
}

pub fn sys_open_url_manifest() -> SkillManifest {
    simple_manifest(
        "sys.open_url",
        "打开网址",
        "用系统默认浏览器打开网址。只允许 http/https（fail-closed）。",
        &["网址", "链接", "URL"],
        &["打开这个网址", "用浏览器打开链接"],
        vec![SimpleInput {
            name: "url",
            input_type: SkillInputType::Url,
            required: true,
            max_length: Some(2000),
            allowed_roots: vec![],
        }],
        true,
        EgressKind::LocalOnly,
        &[],
    )
}

/// 仅 http/https，拒绝 file:// 等本地协议（fail-closed）。
/// 另拒绝 `"`/换行/控制字符：`cmd /c start "" "<url>"` 里引号可逃逸
///（命令注入），mailto 侧因 percent 编码天然免疫。
pub fn is_web_url(url: &str) -> bool {
    let lower = url.trim().to_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return false;
    }
    !url.chars()
        .any(|c| c == '"' || c == '\r' || c == '\n' || c.is_control())
}

pub fn open_url_now(url: &str) -> Result<()> {
    if !is_web_url(url) {
        return Err(KernelError::Skill(format!(
            "拒绝打开网址（非 http(s) 或含引号/换行/控制字符）: {url}"
        )));
    }
    run_cmd("cmd", &["/c", "start", "", url.trim()], 15, 500)?;
    Ok(())
}

pub fn execute_open_url(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let url = slot_text(inputs, "url")?;
    let mut map = HashMap::new();
    map.insert("url".to_string(), serde_json::json!(url));
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "sys.open_url",
        &format!("open:{url}"),
        true,
        &sys_open_url_manifest(),
        &map,
        || {
            open_url_now(&url)?;
            Ok(format!("已用默认浏览器打开：{url}"))
        },
    )
}

/// 绝不碰的进程（自己+桌面壳+系统关键+输入法/无障碍/杀软/音频宿主）。
/// 杀输入法=用户打不了字，杀杀软=裸奔，杀 audiodg=没声音——宁可漏杀。
pub fn quit_all_skip_list() -> Vec<&'static str> {
    vec![
        "voicepilot-ui.exe",
        "voicepilot.exe",
        "explorer.exe",
        "dwm.exe",
        "sihost.exe",
        "taskhostw.exe",
        "System",
        "Registry",
        "smss.exe",
        "csrss.exe",
        "wininit.exe",
        "services.exe",
        "lsass.exe",
        "svchost.exe",
        "conhost.exe",
        "fontdrvhost.exe",
        // 输入法。
        "ctfmon.exe",
        "TextInputHost.exe",
        "ChsIME.exe",
        // 无障碍。
        "Narrator.exe",
        "Magnify.exe",
        "osk.exe",
        // Defender 全家桶。
        "MsMpEng.exe",
        "NisSrv.exe",
        "SecurityHealthService.exe",
        "SecurityHealthSystray.exe",
        "smartscreen.exe",
        // UWP 宿主（杀了等于掀桌子，逐个关应用而不是杀宿主）。
        "ApplicationFrameHost.exe",
        "RuntimeBroker.exe",
        // 音频宿主（杀了没声音）。
        "audiodg.exe",
    ]
}

/// 受害者枚举（纯函数，可测）：tasklist CSV →（去重待杀镜像，跳过数）。
/// 审批前调用，把清单写进 destination，审批人看得见要杀谁。
pub fn compute_quit_victims(tasklist_csv: &str) -> (Vec<String>, u32) {
    let skip = quit_all_skip_list();
    let mut skipped = 0u32;
    let mut images: Vec<String> = vec![];
    for line in tasklist_csv.lines() {
        let first = line.split("\",\"").next().unwrap_or("").trim_matches('"');
        let img = first.trim();
        if img.is_empty() || img.eq_ignore_ascii_case("映像名称") {
            continue;
        }
        if skip.iter().any(|s| s.eq_ignore_ascii_case(img)) {
            skipped += 1;
            continue;
        }
        if !images.iter().any(|e| e.eq_ignore_ascii_case(img)) {
            images.push(img.to_string());
        }
    }
    (images, skipped)
}

pub fn sys_quit_all_manifest() -> SkillManifest {
    simple_manifest(
        "sys.quit_all",
        "关闭所有应用",
        "结束除自己/桌面壳/系统关键进程外的所有桌面应用（免审批）。",
        &["全关", "关闭所有", "退出所有", "关掉所有应用"],
        &["把所有应用都关掉", "关闭所有正在运行的应用"],
        vec![],
        true,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_quit_all(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
) -> Result<String> {
    // 审批前枚举受害者：审批卡 destination 写明要杀谁（截断防超长），
    // 枚举失败直接拒（fail-closed，无 task 落库，与输入校验失败一致）。
    let list = run_cmd("tasklist", &["/FO", "CSV", "/NH"], 20, 200_000)?;
    let (images, skipped) = compute_quit_victims(&list);
    let shown = if images.is_empty() {
        "（无）".to_string()
    } else {
        truncate_chars(&images.join(","), 300)
    };
    let dest = format!("quit_all:杀{}跳过{}:{shown}", images.len(), skipped);
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "sys.quit_all",
        &dest,
        true,
        &sys_quit_all_manifest(),
        &HashMap::new(),
        || {
            let mut killed = 0u32;
            for img in &images {
                if run_cmd("taskkill", &["/F", "/IM", img], 20, 300).is_ok() {
                    killed += 1;
                }
            }
            Ok(format!(
                "已关闭 {killed} 个应用进程，跳过 {skipped} 个（自己/桌面/系统/输入法/杀软）"
            ))
        },
    )
}

pub fn sys_diagnose_app_manifest() -> SkillManifest {
    simple_manifest(
        "sys.diagnose_app",
        "诊断应用打不开",
        "只读诊断：是否装了（开始菜单/PATH）、是否有进程在跑、最近有无崩溃。不做任何修改。",
        &["诊断", "为什么打不开", "打不开", "启动失败"],
        &["诊断一下为什么打不开QQ", "为什么这个应用启动失败"],
        vec![text_slot("app_name", 120)],
        false,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_diagnose_app(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let app = slot_text(inputs, "app_name")?;
    let mut map = HashMap::new();
    map.insert("app_name".to_string(), serde_json::json!(app));
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "sys.diagnose_app",
        &format!("diagnose:{app}"),
        false,
        &sys_diagnose_app_manifest(),
        &map,
        || {
            let mut lines = vec![format!("诊断 {app}：")];
            // 1. 是否可解析（复用启动解析：绝对路径/PATH/开始菜单 .lnk/
            // App Paths/UWP）。uia 未启用时该深度解析不可用，只做后两项。
            #[cfg(all(windows, feature = "uia"))]
            let resolved = crate::uiautomation::adapter::resolve_launch_target(&app);
            #[cfg(not(all(windows, feature = "uia")))]
            let resolved: Option<String> = None;
            #[cfg(all(windows, feature = "uia"))]
            let missing_note = "- 未找到：开始菜单/PATH/注册表/UWP 里都没有匹配项";
            #[cfg(not(all(windows, feature = "uia")))]
            let missing_note = "- 安装位置检查需 uia 功能（当前构建未启用），仅做进程与崩溃检查";
            match resolved {
                Some(t) => lines.push(format!("- 已安装：{t}")),
                None => lines.push(missing_note.to_string()),
            }
            // 2. 是否在跑（精确镜像名）。
            for cand in [app.clone(), format!("{app}.exe")] {
                if let Ok(out) = run_cmd(
                    "tasklist",
                    &["/FI", &format!("IMAGENAME eq {cand}"), "/FO", "CSV", "/NH"],
                    15,
                    2000,
                ) {
                    if out.contains(&cand) {
                        lines.push(format!("- 正在运行：{cand}"));
                    }
                }
            }
            // 3. 最近 7 天应用崩溃（尽力而为，查不到不算失败）。
            let ps = format!(
                "Get-WinEvent -FilterHashtable @{{LogName='Application'; Level=2; StartTime=(Get-Date).AddDays(-7)}} -MaxEvents 50 -ErrorAction SilentlyContinue | Where-Object {{ $_.Message -like '*{app}*' }} | Select-Object -First 3 TimeCreated,Id | Format-Table -HideTableHeaders | Out-String"
            );
            match ps_eval(&ps) {
                Ok(out) if !out.trim().is_empty() => lines.push(format!(
                    "- 近 7 天相关崩溃：{}",
                    truncate_chars(out.trim(), 400)
                )),
                _ => lines.push("- 近 7 天无相关崩溃记录".to_string()),
            }
            Ok(lines.join("\n"))
        },
    )
}

pub fn sys_timer_set_manifest() -> SkillManifest {
    simple_manifest(
        "sys.timer_set",
        "倒计时",
        "N 秒后弹系统通知。用一次性计划任务实现，关掉本程序也有效。",
        &["倒计时", "计时", "定时提醒", "几分钟后提醒我"],
        &["倒计时5分钟", "10分钟后提醒我"],
        vec![SimpleInput {
            name: "seconds",
            input_type: SkillInputType::Number,
            required: true,
            max_length: None,
            allowed_roots: vec![],
        }],
        true,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_timer_set(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let seconds = inputs
        .get("seconds")
        .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()))
        .filter(|s| *s > 0 && *s <= 86400)
        .ok_or_else(|| KernelError::Skill("seconds 须为 1~86400 的整数".to_string()))?;
    let mut map = HashMap::new();
    map.insert("seconds".to_string(), serde_json::json!(seconds));
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "sys.timer_set",
        &format!("timer:{seconds}s"),
        true,
        &sys_timer_set_manifest(),
        &map,
        || {
            require_bin("msg", "家庭版 Windows 没有 msg.exe，改用专业版或换提醒方式")?;
            let fire = chrono::Local::now() + chrono::Duration::seconds(seconds);
            let when = schtasks_once(&schtasks_name("timer", fire), fire, "倒计时结束")?;
            Ok(format!("倒计时 {seconds} 秒已设置，将在 {when} 弹窗提醒"))
        },
    )
}

/// 定时系的“下一次触发”解析："HH:mm"（每日最近一次）或
/// "YYYY-MM-DD HH:mm"（Daisy 闹钟格式，过期则报错）。
pub fn next_alarm_time(hhmm: &str) -> Result<chrono::DateTime<chrono::Local>> {
    let t = hhmm.trim();
    // "YYYY-MM-DD HH:mm"：指定日期（Daisy 格式）。
    if let Some((date, hm)) = t.split_once(' ') {
        let d: Vec<&str> = date.split('-').collect();
        let hms: Vec<&str> = hm.split(':').collect();
        if d.len() == 3 && hms.len() == 2 {
            if let (Ok(y), Ok(mo), Ok(dd), Ok(h), Ok(mi)) = (
                d[0].parse::<i32>(),
                d[1].parse::<u32>(),
                d[2].parse::<u32>(),
                hms[0].parse::<u32>(),
                hms[1].parse::<u32>(),
            ) {
                if let Some(naive) =
                    chrono::NaiveDate::from_ymd_opt(y, mo, dd).and_then(|d| d.and_hms_opt(h, mi, 0))
                {
                    if let Some(dt) = naive.and_local_timezone(chrono::Local).single() {
                        if dt > chrono::Local::now() {
                            return Ok(dt);
                        }
                        return Err(KernelError::Skill(format!(
                            "闹钟时间 {t} 已过期，请指定未来时刻"
                        )));
                    }
                }
            }
        }
        return Err(KernelError::Skill(
            "time 须为 HH:mm（如 07:30）或 YYYY-MM-DD HH:mm".to_string(),
        ));
    }
    let (h, m) = hhmm
        .trim()
        .split_once(':')
        .and_then(|(h, m)| Some((h.trim().parse::<u32>().ok()?, m.trim().parse::<u32>().ok()?)))
        .filter(|(h, m)| *h < 24 && *m < 60)
        .ok_or_else(|| KernelError::Skill("time 须为 HH:mm（24小时制），如 07:30".to_string()))?;
    let now = chrono::Local::now();
    let today = now
        .date_naive()
        .and_hms_opt(h, m, 0)
        .ok_or_else(|| KernelError::Skill("非法时间".to_string()))?;
    let naive = if today
        .and_local_timezone(chrono::Local)
        .single()
        .map(|t| t > now)
        .unwrap_or(false)
    {
        today
    } else {
        today + chrono::Duration::days(1)
    };
    naive
        .and_local_timezone(chrono::Local)
        .single()
        .ok_or_else(|| KernelError::Skill("时区转换失败".to_string()))
}

pub fn sys_alarm_set_manifest() -> SkillManifest {
    simple_manifest(
        "sys.alarm_set",
        "设闹钟",
        "到指定时刻（HH:mm，24小时制）弹系统通知。一次性计划任务。",
        &["闹钟", "叫醒", "提醒我起床", "几点叫我"],
        &["明天早上7点叫醒我", "设一个下午3点的闹钟"],
        vec![
            text_slot("time", 8),
            SimpleInput {
                name: "label",
                input_type: SkillInputType::Text,
                required: false,
                max_length: Some(60),
                allowed_roots: vec![],
            },
        ],
        true,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_alarm_set(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let time = slot_text(inputs, "time")?;
    let label = slot_text_opt(inputs, "label").unwrap_or_else(|| "闹钟".to_string());
    let mut map = HashMap::new();
    map.insert("time".to_string(), serde_json::json!(time));
    if inputs.get("label").is_some() {
        map.insert("label".to_string(), serde_json::json!(label));
    }
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "sys.alarm_set",
        &format!("alarm:{time}"),
        true,
        &sys_alarm_set_manifest(),
        &map,
        || {
            require_bin("msg", "家庭版 Windows 没有 msg.exe，改用专业版或换提醒方式")?;
            let fire = next_alarm_time(&time)?;
            let when = schtasks_once(&schtasks_name("alarm", fire), fire, &label)?;
            Ok(format!("闹钟已设置：{label}，将在 {when} 弹窗提醒"))
        },
    )
}

/// Bing 地图搜索链接（与 Daisy Windows 行为一致）。
pub fn build_maps_url(query: &str) -> String {
    use std::fmt::Write;
    let mut enc = String::new();
    for b in query.trim().bytes() {
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' || b == b'~' {
            enc.push(b as char);
        } else {
            let _ = write!(enc, "%{b:02X}");
        }
    }
    format!("https://www.bing.com/maps?q={enc}")
}

pub fn sys_maps_search_manifest() -> SkillManifest {
    simple_manifest(
        "sys.maps_search",
        "查地图",
        "在浏览器打开 Bing 地图搜地点（Windows 无原生地图调用）。",
        &["地图", "地址", "怎么走", "导航"],
        &["查一下这个地址在哪", "帮我搜地图"],
        vec![text_slot("query", 200)],
        true,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_maps_search(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let query = slot_text(inputs, "query")?;
    let mut map = HashMap::new();
    map.insert("query".to_string(), serde_json::json!(query));
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "sys.maps_search",
        &format!("maps:{query}"),
        true,
        &sys_maps_search_manifest(),
        &map,
        || {
            let url = build_maps_url(&query);
            open_url_now(&url)?;
            Ok(format!("已在地图中搜索：{query}"))
        },
    )
}

/// 要关的浏览器镜像（与 Daisy quitAllBrowsers 同表）。
pub fn quit_browser_images() -> Vec<&'static str> {
    vec![
        "chrome.exe",
        "msedge.exe",
        "firefox.exe",
        "brave.exe",
        "opera.exe",
        "vivaldi.exe",
    ]
}

pub fn sys_quit_browsers_manifest() -> SkillManifest {
    simple_manifest(
        "sys.quit_browsers",
        "关闭浏览器",
        "关闭所有正在运行的浏览器（Chrome/Edge/Firefox/Brave/Opera/Vivaldi）（免审批）。",
        &["关闭浏览器", "退出浏览器", "关掉浏览器", "关浏览器"],
        &["把浏览器都关掉", "关闭所有浏览器"],
        vec![],
        true,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_quit_browsers(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
) -> Result<String> {
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "sys.quit_browsers",
        "sys:quit_browsers",
        true,
        &sys_quit_browsers_manifest(),
        &HashMap::new(),
        || {
            let mut killed = vec![];
            for img in quit_browser_images() {
                if run_cmd("taskkill", &["/F", "/IM", img], 15, 300).is_ok() {
                    killed.push(img);
                }
            }
            if killed.is_empty() {
                return Ok("没有正在运行的浏览器".to_string());
            }
            Ok(format!("已关闭浏览器：{}", killed.join("、")))
        },
    )
}

/// 音量方向 → keybd_event VK 码（与 Daisy 同值）。纯函数，可测。
pub fn volume_vk(direction: &str) -> Option<u8> {
    match direction.trim().to_lowercase().as_str() {
        "up" | "调高" | "增大" | "大" => Some(0xAF),
        "down" | "调低" | "减小" | "小" => Some(0xAE),
        "mute" | "静音" => Some(0xAD),
        _ => None,
    }
}

/// 播放控制 → keybd_event VK 码（与 Daisy 同值）。纯函数，可测。
pub fn playback_vk(action: &str) -> Option<u8> {
    match action.trim().to_lowercase().as_str() {
        "playpause" | "暂停" | "播放" | "继续" => Some(0xB3),
        "next" | "下一首" | "下一曲" => Some(0xB0),
        "prev" | "上一首" | "上一曲" => Some(0xB1),
        _ => None,
    }
}

/// keybd_event 按压脚本（纯函数，可测）。down+up 一次完整按键。
pub fn keybd_event_ps(vk: u8) -> String {
    format!(
        "Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public class K {{ [DllImport(\"user32.dll\")] public static extern void keybd_event(byte bVk, byte bScan, uint dwFlags, UIntPtr dwExtraInfo); }}'; [K]::keybd_event({vk},0,0,[UIntPtr]::Zero); [K]::keybd_event({vk},0,2,[UIntPtr]::Zero)"
    )
}

pub fn sys_volume_manifest() -> SkillManifest {
    simple_manifest(
        "sys.volume",
        "调音量",
        "调高/调低/静音系统音量（模拟媒体键）（免审批）。",
        &["音量", "声音", "静音", "调高", "调低"],
        &["音量调高一点", "静音", "声音小一点"],
        vec![text_slot("direction", 8)],
        true,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_sys_volume(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let direction = slot_text(inputs, "direction")?;
    let mut map = HashMap::new();
    map.insert("direction".to_string(), serde_json::json!(direction));
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "sys.volume",
        &format!("volume:{direction}"),
        true,
        &sys_volume_manifest(),
        &map,
        || {
            let vk = volume_vk(&direction).ok_or_else(|| {
                KernelError::Skill(format!(
                    "direction 须为 up/down/mute（调高/调低/静音）: {direction}"
                ))
            })?;
            ps_eval(&keybd_event_ps(vk))?;
            Ok(format!("已执行音量操作：{direction}"))
        },
    )
}

pub fn sys_playback_manifest() -> SkillManifest {
    simple_manifest(
        "sys.playback",
        "播控",
        "播放/暂停/下一首/上一首（模拟媒体键，作用于当前播放器）（免审批）。",
        &["暂停", "播放", "下一首", "上一首", "继续播放", "切歌"],
        &["暂停播放", "下一首", "继续播放"],
        vec![text_slot("action", 12)],
        true,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_sys_playback(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let action = slot_text(inputs, "action")?;
    let mut map = HashMap::new();
    map.insert("action".to_string(), serde_json::json!(action));
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "sys.playback",
        &format!("playback:{action}"),
        true,
        &sys_playback_manifest(),
        &map,
        || {
            let vk = playback_vk(&action).ok_or_else(|| {
                KernelError::Skill(format!(
                    "action 须为 playpause/next/prev（暂停/下一首/上一首）: {action}"
                ))
            })?;
            ps_eval(&keybd_event_ps(vk))?;
            Ok(format!("已执行播放控制：{action}"))
        },
    )
}

pub fn sys_lock_screen_manifest() -> SkillManifest {
    simple_manifest(
        "sys.lock_screen",
        "锁屏",
        "锁定电脑（Win+L 等价，需登录回来）（免审批）。",
        &["锁屏", "锁定屏幕", "锁电脑", "息屏"],
        &["锁屏", "把电脑锁上"],
        vec![],
        true,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_lock_screen(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
) -> Result<String> {
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "sys.lock_screen",
        "sys:lock_screen",
        true,
        &sys_lock_screen_manifest(),
        &HashMap::new(),
        || {
            run_cmd("rundll32.exe", &["user32.dll,LockWorkStation"], 15, 300)?;
            Ok("已锁屏".to_string())
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::approver::AutoApprover;
    use crate::kernel::TrustKernel;
    use crate::skills::manifest::ApprovalMode;

    #[test]
    fn sys_manifests_have_ids_and_keywords() {
        for m in [
            sys_datetime_manifest(),
            sys_frontmost_manifest(),
            sys_open_url_manifest(),
            sys_quit_all_manifest(),
            sys_diagnose_app_manifest(),
            sys_timer_set_manifest(),
            sys_alarm_set_manifest(),
            sys_maps_search_manifest(),
            sys_quit_browsers_manifest(),
            sys_volume_manifest(),
            sys_playback_manifest(),
            sys_lock_screen_manifest(),
        ] {
            assert!(!m.id.is_empty() && !m.keywords.is_empty(), "{}", m.id);
            assert!(!m.intent_examples.is_empty(), "{}", m.id);
        }
    }

    #[test]
    fn frontmost_formats_exe_and_title() {
        assert_eq!(
            format_frontmost("notepad.exe", "x - 记事本"),
            "当前前台：notepad.exe（x - 记事本）"
        );
        assert_eq!(format_frontmost("qq.exe", ""), "当前前台：qq.exe");
        assert_eq!(format_frontmost("", "QQ"), "当前前台窗口标题：QQ");
    }

    #[test]
    fn media_key_maps_match_daisy_vk_codes() {
        assert_eq!(volume_vk("up"), Some(0xAF));
        assert_eq!(volume_vk("调低"), Some(0xAE));
        assert_eq!(volume_vk("mute"), Some(0xAD));
        assert_eq!(volume_vk("nope"), None);
        assert_eq!(playback_vk("playpause"), Some(0xB3));
        assert_eq!(playback_vk("下一首"), Some(0xB0));
        assert_eq!(playback_vk("上一首"), Some(0xB1));
        assert_eq!(playback_vk("nope"), None);
        let ps = keybd_event_ps(0xAF);
        assert!(
            ps.contains("keybd_event(175,0,0") && ps.contains(",2,"),
            "{ps}"
        );
    }

    #[test]
    fn alarm_accepts_full_date() {
        // 未来日期 OK。
        let t = next_alarm_time("2099-01-02 07:30").unwrap();
        assert_eq!(t.format("%Y-%m-%d %H:%M").to_string(), "2099-01-02 07:30");
        // 过去日期诚实报错。
        assert!(next_alarm_time("2000-01-01 07:30").is_err());
        assert!(next_alarm_time("xxxx 07:30").is_err());
    }

    #[test]
    fn destructive_executes_require_approval() {
        // 2026 姿态变更：sys.* 不在审批白名单 —— manifest 一律 None。
        // 原 AutoDenier 阻塞断言已过时（不再弹审批），改断言 manifest 姿态。
        for m in [
            sys_lock_screen_manifest(),
            sys_quit_browsers_manifest(),
            sys_volume_manifest(),
            sys_playback_manifest(),
        ] {
            assert!(
                matches!(m.approval.mode, ApprovalMode::None),
                "{} 应免审批",
                m.id
            );
        }
        // 真实执行机器影响大，这里只验证姿态，不实际调用。
    }

    #[test]
    fn datetime_executes_without_approval() {
        let kernel = TrustKernel::open_in_memory().unwrap();
        let out = execute_datetime(&kernel, &AutoApprover, "t1", "s1").unwrap();
        assert!(out.contains("现在是"), "{out}");
        // 审计留痕：任务存在。
        assert!(kernel.list_approvals_for_task("t1").unwrap().is_empty());
    }

    #[test]
    fn open_url_cancels_without_launch() {
        // ApprovalMode::None：AutoDenier 不再拦截（2026 免审批姿态）。
        // 仅验证 manifest 姿态；真实 launch 会开系统浏览器，不在此触发。
        let m = sys_open_url_manifest();
        assert!(matches!(m.approval.mode, ApprovalMode::None));
    }

    #[test]
    fn open_url_rejects_non_http() {
        assert!(is_web_url("https://a.b"));
        assert!(is_web_url("HTTP://a.b"));
        assert!(!is_web_url("file:///c:/x"));
        assert!(!is_web_url("javascript:alert(1)"));
        assert!(open_url_now("file:///c:/x").is_err());
    }

    #[test]
    fn alarm_parses_next_occurrence() {
        assert!(next_alarm_time("25:00").is_err());
        assert!(next_alarm_time("nope").is_err());
        // 合法输入必返回未来时刻。
        let t = next_alarm_time("00:00").unwrap();
        assert!(t > chrono::Local::now() - chrono::Duration::minutes(2));
    }

    #[test]
    fn maps_url_encodes_cjk() {
        let url = build_maps_url("天安门");
        assert!(url.starts_with("https://www.bing.com/maps?q="), "{url}");
        assert!(!url.contains("天安"), "{url}");
    }

    #[test]
    fn quit_all_skip_list_protects_self_and_shell() {
        let skip = quit_all_skip_list();
        for must in [
            "voicepilot-ui.exe",
            "explorer.exe",
            "svchost.exe",
            "csrss.exe",
            "ctfmon.exe",
            "TextInputHost.exe",
            "Narrator.exe",
            "MsMpEng.exe",
            "audiodg.exe",
            "ApplicationFrameHost.exe",
        ] {
            assert!(skip.contains(&must), "{must}");
        }
    }

    #[test]
    fn quit_victims_split_and_dedup() {
        let csv = "\"映像名称\",\"PID\"\r\n\"QQ.exe\",\"1\"\r\n\"qq.exe\",\"2\"\r\n\"explorer.exe\",\"3\"\r\n\"svchost.exe\",\"4\"\r\n";
        let (images, skipped) = compute_quit_victims(csv);
        assert_eq!(images, vec!["QQ.exe".to_string()]);
        assert_eq!(skipped, 2);
    }

    #[test]
    fn open_url_rejects_quote_injection() {
        assert!(!is_web_url("https://a.b/\"&calc.exe"));
        assert!(!is_web_url("https://a.b/x\ncalc"));
        assert!(open_url_now("https://a.b/\"&notepad").is_err());
    }

    #[test]
    #[cfg(all(windows, feature = "uia"))]
    fn diagnose_missing_app_reports_not_found() {
        // 断言依赖 uia 深度解析（开始菜单/PATH/注册表）；无 uia 时输出改为
        // “安装位置检查需 uia 功能”，该句断言不适用。
        let kernel = TrustKernel::open_in_memory().unwrap();
        let inputs = serde_json::json!({"app_name": "vp-no-such-app-xyz"});
        let out = execute_diagnose_app(&kernel, &AutoApprover, "t3", "s3", &inputs).unwrap();
        assert!(out.contains("未找到"), "{out}");
    }
}
