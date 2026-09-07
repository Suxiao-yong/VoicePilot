//! Daisy 移植：剪贴板组（读/写/读选中/打字/按键）。
//!
//! 零依赖实现：剪贴板走 PowerShell（`-STA`，控制台默认 MTA 用不了
//! `Get-Clipboard`），按键走 `SendKeys`（与 Daisy 同机制）。
//! 全部免审批（2026 姿态）：读/写剪贴板只读本地、审计留痕；打字/按键/
//! 读选中虽驱动前台 UI，同样免审批直接执行，仅审计。

use crate::approval::approver::Approver;
use crate::error::{KernelError, Result};
use crate::kernel::TrustKernel;
use crate::skills::manifest::{EgressKind, SkillManifest};
use crate::skills::simple::{
    ps_sta, run_simple, simple_manifest, slot_text, truncate_chars, SimpleInput,
};
use std::collections::HashMap;
use std::path::PathBuf;

fn text_slot(name: &'static str, max: u32) -> SimpleInput {
    SimpleInput {
        name,
        input_type: crate::skills::manifest::SkillInputType::Text,
        required: true,
        max_length: Some(max),
        allowed_roots: vec![],
    }
}

/// base64 包一层：任意文本进单引号 PS 字符串不转义、不截断。
pub fn ps_set_clipboard_b64(b64: &str) -> String {
    format!(
        "$b=[Convert]::FromBase64String('{b64}'); [Text.Encoding]::UTF8.GetString($b) | Set-Clipboard"
    )
}

pub fn encode_clipboard_text(text: &str) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(text.as_bytes())
}

pub fn clip_read_manifest() -> SkillManifest {
    simple_manifest(
        "clip.read",
        "读剪贴板",
        "读取系统剪贴板文本。纯本地只读。",
        &["剪贴板", "复制了什么", "刚才复制", "粘贴板"],
        &["读取我刚才复制的内容", "剪贴板里是什么"],
        vec![],
        false,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_clip_read(
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
        "clip.read",
        "clip:read",
        false,
        &clip_read_manifest(),
        &HashMap::new(),
        || {
            let text = ps_sta("Get-Clipboard -Raw -ErrorAction Stop")?;
            let text = text.trim();
            if text.is_empty() {
                return Ok("剪贴板是空的".to_string());
            }
            Ok(format!("剪贴板内容：{}", truncate_chars(text, 4000)))
        },
    )
}

pub fn clip_write_manifest() -> SkillManifest {
    simple_manifest(
        "clip.write",
        "写剪贴板",
        "把文本写入剪贴板，用户可直接粘贴。用户可见、可重做，审计留痕。",
        &["复制", "拷贝", "存剪贴板", "放到剪贴板"],
        &["复制以下内容", "把这段文字存剪贴板"],
        vec![text_slot("text", 8000)],
        false,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_clip_write(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let text = slot_text(inputs, "text")?;
    let mut map = HashMap::new();
    map.insert("text".to_string(), serde_json::json!(text));
    // destination 绑定内容摘要（截断 120 字）：审批/审计看得见写什么。
    let dest = format!("write:{}", truncate_chars(&text, 120));
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "clip.write",
        &dest,
        false,
        &clip_write_manifest(),
        &map,
        || {
            ps_sta(&ps_set_clipboard_b64(&encode_clipboard_text(&text)))?;
            Ok(format!(
                "已写入剪贴板（{} 字），可直接粘贴",
                text.chars().count()
            ))
        },
    )
}

/// SendKeys 转义：+^%~(){}[] 为特殊语义，其余直通（含中文）。
pub fn sendkeys_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '+' | '^' | '%' | '~' | '(' | ')' | '{' | '}' | '[' | ']' => {
                out.push('{');
                out.push(ch);
                out.push('}');
            }
            _ => out.push(ch),
        }
    }
    out
}

/// 按键规格解析："ctrl+c" / "enter" / "alt+tab" / "f5"。
/// 修饰键可叠加（ctrl+shift+esc）；Win 键 SendKeys 不支持，诚实报错。
pub fn sendkeys_for_spec(spec: &str) -> Result<String> {
    let mut mods = String::new();
    let mut key: Option<String> = None;
    for tok in spec
        .split('+')
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
    {
        match tok.as_str() {
            "ctrl" | "control" => mods.push('^'),
            "alt" | "option" => mods.push('%'),
            "shift" => mods.push('+'),
            "win" | "windows" | "super" | "cmd" | "command" | "meta" => {
                return Err(KernelError::Skill(
                    "SendKeys 不支持 Win 键（改说“打开XX”走应用启动）".to_string(),
                ))
            }
            "enter" | "return" => key = Some("{ENTER}".to_string()),
            "tab" => key = Some("{TAB}".to_string()),
            "esc" | "escape" => key = Some("{ESC}".to_string()),
            "space" => key = Some(" ".to_string()),
            "backspace" | "bs" => key = Some("{BS}".to_string()),
            "delete" | "del" => key = Some("{DEL}".to_string()),
            "insert" | "ins" => key = Some("{INSERT}".to_string()),
            "home" => key = Some("{HOME}".to_string()),
            "end" => key = Some("{END}".to_string()),
            "pgup" | "pageup" => key = Some("{PGUP}".to_string()),
            "pgdn" | "pagedown" => key = Some("{PGDN}".to_string()),
            "up" => key = Some("{UP}".to_string()),
            "down" => key = Some("{DOWN}".to_string()),
            "left" => key = Some("{LEFT}".to_string()),
            "right" => key = Some("{RIGHT}".to_string()),
            s if s.len() == 1 => key = Some(sendkeys_escape(s)),
            s if s.starts_with('f')
                && s[1..]
                    .parse::<u8>()
                    .map(|n| (1..=16).contains(&n))
                    .unwrap_or(false) =>
            {
                key = Some(format!("{{{}}}", s.to_uppercase()))
            }
            other => {
                return Err(KernelError::Skill(format!("不支持的按键: {other}")));
            }
        }
    }
    let key = key
        .filter(|k| !k.is_empty())
        .ok_or_else(|| KernelError::Skill("按键规格为空（如 ctrl+c / enter）".to_string()))?;
    Ok(format!("{mods}{key}"))
}

pub fn sendkeys_now(sendkeys: &str) -> Result<()> {
    // 单引号在 PS 里转义为两个单引号；SendKeys 负载不含单引号语义，安全。
    let safe = sendkeys.replace('\'', "''");
    let ps = format!(
        "Add-Type -AssemblyName System.Windows.Forms | Out-Null; [System.Windows.Forms.SendKeys]::SendWait('{safe}')"
    );
    ps_sta(&ps)?;
    Ok(())
}

pub fn clip_selected_manifest() -> SkillManifest {
    simple_manifest(
        "clip.selected",
        "读选中文字",
        "向前台应用发 Ctrl+C，读回选中文字；读写前后备份恢复你的原剪贴板。会驱动 UI（免审批）。",
        &["选中", "选中的文字", "读选中", "刚才选的"],
        &["读取我选中的文字", "我选中的是什么"],
        vec![],
        true,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_clip_selected(
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
        "clip.selected",
        "clip:selected",
        true,
        &clip_selected_manifest(),
        &HashMap::new(),
        || {
            // 先备份用户剪贴板，读完原样恢复（不 trim：首尾空白也是数据）。
            let before =
                ps_sta("Get-Clipboard -Raw -ErrorAction SilentlyContinue").unwrap_or_default();
            let ps = "[System.Windows.Forms.SendKeys]::SendWait('^c'); Start-Sleep -Milliseconds 400; Get-Clipboard -Raw -ErrorAction Stop";
            let got = ps_sta(&format!(
                "Add-Type -AssemblyName System.Windows.Forms | Out-Null; {ps}"
            ));
            let _ = ps_sta(&ps_set_clipboard_b64(&encode_clipboard_text(&before)));
            let text = got?.trim().to_string();
            // ^c 失败时剪贴板还是备份前的内容：与备份一致即视为无选中，
            // 不把旧内容谎报成选中文字。
            if text.is_empty() || text == before.trim() {
                return Ok("没读到选中文字（可能当前无选中内容；剪贴板已恢复）".to_string());
            }
            Ok(format!(
                "选中文字：{}（你的原剪贴板已恢复）",
                truncate_chars(&text, 4000)
            ))
        },
    )
}

pub fn clip_type_text_manifest() -> SkillManifest {
    simple_manifest(
        "clip.type_text",
        "打字",
        "在当前光标处直发按键输入文字（SendKeys 直驱；长文本/CJK 偶发丢字，需焦点正确）（免审批）。",
        &["打字", "键入", "帮我输"],
        &["帮我输入这段文字", "在这里打字"],
        vec![text_slot("text", 2000)],
        true,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_clip_type_text(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let text = slot_text(inputs, "text")?;
    let mut map = HashMap::new();
    map.insert("text".to_string(), serde_json::json!(text));
    // destination 绑定内容摘要（截断 60 字）：审批/审计看得见打什么。
    let dest = format!("type:{}", truncate_chars(&text, 60));
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "clip.type_text",
        &dest,
        true,
        &clip_type_text_manifest(),
        &map,
        || {
            sendkeys_now(&sendkeys_escape(&text))?;
            Ok(format!("已输入 {} 字", text.chars().count()))
        },
    )
}

pub fn clip_press_keys_manifest() -> SkillManifest {
    simple_manifest(
        "clip.press_keys",
        "按键",
        "发送键盘快捷键（如 ctrl+c / enter / alt+tab）（免审批）。Win 键不支持。",
        &["按键", "快捷键", "回车", "复制粘贴键"],
        &["按回车", "按ctrl+c", "按alt+tab切换窗口"],
        vec![text_slot("keys", 60)],
        true,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_clip_press_keys(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let keys = slot_text(inputs, "keys")?;
    let mut map = HashMap::new();
    map.insert("keys".to_string(), serde_json::json!(keys));
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "clip.press_keys",
        &format!("keys:{keys}"),
        true,
        &clip_press_keys_manifest(),
        &map,
        || {
            let seq = sendkeys_for_spec(&keys)?;
            sendkeys_now(&seq)?;
            Ok(format!("已发送按键：{keys}"))
        },
    )
}

/// 剪贴板图片存桌面（Daisy 同机制：STA + WinForms GetImage → PNG）。
/// 返回 Ok(path)/OkEmpty/NoImage 三态：无图是正常回复，不是失败。
pub enum ClipImageOutcome {
    Saved(PathBuf),
    NoImage,
}

pub fn save_clipboard_image() -> Result<ClipImageOutcome> {
    let desktop =
        dirs::desktop_dir().ok_or_else(|| KernelError::Skill("取不到桌面目录".to_string()))?;
    let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
    let target = desktop.join(format!("剪贴板图片_{stamp}.png"));
    let ps = format!(
        "Add-Type -AssemblyName System.Windows.Forms | Out-Null; Add-Type -AssemblyName System.Drawing | Out-Null; $img = [System.Windows.Forms.Clipboard]::GetImage(); if ($null -eq $img) {{ 'NOIMAGE' }} else {{ $img.Save('{path}', [System.Drawing.Imaging.ImageFormat]::Png); 'SAVED' }}",
        path = target.to_string_lossy().replace('\'', "''")
    );
    let out = ps_sta(&ps)?;
    if out.trim() == "SAVED" {
        return Ok(ClipImageOutcome::Saved(target));
    }
    Ok(ClipImageOutcome::NoImage)
}

pub fn clip_save_image_manifest() -> SkillManifest {
    simple_manifest(
        "clip.save_image",
        "存剪贴板图片",
        "把剪贴板里的图片存到桌面 PNG。无图则如实回复（不是失败）。写文件（免审批）。",
        &["存图", "保存图片", "剪贴板图片", "截图保存"],
        &["把剪贴板的图片存下来", "保存剪贴板里的图"],
        vec![],
        true,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_clip_save_image(
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
        "clip.save_image",
        "clip:save_image",
        true,
        &clip_save_image_manifest(),
        &HashMap::new(),
        || match save_clipboard_image()? {
            ClipImageOutcome::Saved(p) => Ok(format!("已存到桌面：{}", p.display())),
            ClipImageOutcome::NoImage => Ok("剪贴板里现在没有图片".to_string()),
        },
    )
}

/// 前台资源管理器选中文件（一镜到底 STA PowerShell + Shell.Application）。
/// 只读前台 Explorer 窗口；桌面与其他窗口如实返回来源（不硬猜）。
pub enum SelectedFilesOutcome {
    Files(Vec<PathBuf>),
    Empty,
    Unsupported(String),
}

pub fn read_explorer_selection() -> Result<SelectedFilesOutcome> {
    // 前台 CabinetWClass 窗口的 SelectedItems 取 path；其他来源报来源。
    let ps = r#"$shell = New-Object -ComObject Shell.Application
Add-Type -Namespace VP -Name Win2 -MemberDefinition '[DllImport("user32.dll")] public static extern System.IntPtr GetForegroundWindow(); [DllImport("user32.dll")] public static extern System.IntPtr GetAncestor(System.IntPtr h, uint f); [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(System.IntPtr h, System.Text.StringBuilder t, int n);' | Out-Null
$fg = [VP.Win2]::GetForegroundWindow()
$root = [VP.Win2]::GetAncestor($fg, 2)
if ($root -eq [IntPtr]::Zero) { $root = $fg }
$sb = New-Object System.Text.StringBuilder 256
[VP.Win2]::GetClassName($root, $sb, 256) | Out-Null
$cls = $sb.ToString()
if ($cls -ne 'CabinetWClass') { "SRC:$cls" }
else {
  $fgInt = [long]$root.ToInt64()
  $done = $false
  foreach ($w in $shell.Windows()) {
    try {
      $wr = [VP.Win2]::GetAncestor([IntPtr]$w.HWND, 2)
      if ([long]$wr.ToInt64() -eq $fgInt) {
        $done = $true
        foreach ($item in $w.Document.SelectedItems()) { "FILE:" + $item.Path }
      }
    } catch { continue }
  }
  if (-not $done) { "SRC:no-match" }
}"#;
    let out = ps_sta(ps)?;
    let mut files = vec![];
    for line in out.lines() {
        let line = line.trim();
        if let Some(p) = line.strip_prefix("FILE:") {
            if !p.trim().is_empty() {
                files.push(PathBuf::from(p.trim()));
            }
        } else if let Some(src) = line.strip_prefix("SRC:") {
            let src = src.trim();
            if src == "Progman" || src == "WorkerW" {
                return Ok(SelectedFilesOutcome::Unsupported(
                    "当前选中的是桌面图标（只支持资源管理器窗口内选中）".to_string(),
                ));
            }
            return Ok(SelectedFilesOutcome::Unsupported(format!(
                "当前前台不是资源管理器（{src}），请先在资源管理器里选中文件"
            )));
        }
    }
    if files.is_empty() {
        return Ok(SelectedFilesOutcome::Empty);
    }
    Ok(SelectedFilesOutcome::Files(files))
}

pub fn clip_selected_files_manifest() -> SkillManifest {
    simple_manifest(
        "clip.selected_files",
        "读选中文件",
        "读取前台资源管理器窗口里选中的文件路径。纯本地只读。",
        &["选中文件", "选了哪些文件", "选中的文件", "资源管理器选中"],
        &["我选了哪些文件", "读取选中的文件"],
        vec![],
        false,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_clip_selected_files(
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
        "clip.selected_files",
        "clip:selected_files",
        false,
        &clip_selected_files_manifest(),
        &HashMap::new(),
        || match read_explorer_selection()? {
            SelectedFilesOutcome::Files(files) => {
                let names: Vec<String> = files
                    .iter()
                    .take(30)
                    .map(|p| p.display().to_string())
                    .collect();
                Ok(format!(
                    "选中了 {} 个文件：\n{}",
                    files.len(),
                    names.join("\n")
                ))
            }
            SelectedFilesOutcome::Empty => Ok("资源管理器里当前没有选中文件".to_string()),
            SelectedFilesOutcome::Unsupported(msg) => Ok(msg),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::approver::AutoApprover;
    use crate::kernel::TrustKernel;
    use crate::skills::manifest::ApprovalMode;

    // 真剪贴板是进程级全局状态：所有碰它的测试必须串行。
    static CLIP_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    fn clip_lock() -> std::sync::MutexGuard<'static, ()> {
        CLIP_LOCK
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap()
    }

    #[test]
    fn clip_manifests_have_ids_and_keywords() {
        for m in [
            clip_read_manifest(),
            clip_write_manifest(),
            clip_selected_manifest(),
            clip_type_text_manifest(),
            clip_press_keys_manifest(),
            clip_save_image_manifest(),
            clip_selected_files_manifest(),
        ] {
            assert!(!m.id.is_empty() && !m.keywords.is_empty(), "{}", m.id);
        }
    }

    #[test]
    fn save_image_approval_free_posture() {
        // 2026：clip.save_image 不在审批白名单 —— manifest 姿态 None。
        let m = clip_save_image_manifest();
        assert!(matches!(m.approval.mode, ApprovalMode::None), "{}", m.id);
    }

    #[test]
    fn sendkeys_escape_wraps_specials() {
        assert_eq!(sendkeys_escape("a+c"), "a{+}c");
        assert_eq!(sendkeys_escape("100%"), "100{%}");
        assert_eq!(sendkeys_escape("你好(世界)"), "你好{(}世界{)}");
        assert_eq!(sendkeys_escape("plain"), "plain");
    }

    #[test]
    fn sendkeys_spec_parses_modifiers_and_names() {
        assert_eq!(sendkeys_for_spec("ctrl+c").unwrap(), "^c");
        assert_eq!(sendkeys_for_spec("Ctrl+C").unwrap(), "^c");
        assert_eq!(sendkeys_for_spec("enter").unwrap(), "{ENTER}");
        assert_eq!(sendkeys_for_spec("return").unwrap(), "{ENTER}");
        assert_eq!(sendkeys_for_spec("esc").unwrap(), "{ESC}");
        assert_eq!(sendkeys_for_spec("alt+tab").unwrap(), "%{TAB}");
        assert_eq!(sendkeys_for_spec("option+tab").unwrap(), "%{TAB}");
        assert_eq!(sendkeys_for_spec("ctrl+shift+esc").unwrap(), "^+{ESC}");
        assert_eq!(sendkeys_for_spec("f5").unwrap(), "{F5}");
        assert_eq!(sendkeys_for_spec("f12").unwrap(), "{F12}");
        assert!(sendkeys_for_spec("f17").is_err());
        assert!(sendkeys_for_spec("win+r").is_err());
        assert!(sendkeys_for_spec("cmd+c").is_err());
        assert!(sendkeys_for_spec("").is_err());
        assert!(sendkeys_for_spec("hyper+z").is_err());
    }

    #[test]
    fn clipboard_write_read_restore() {
        // 合并读写断言：剪贴板是进程级全局状态，并行测试会互踩，
        // 读写必须在同一个测试里串行完成。
        let _guard = clip_lock();
        let before = ps_sta("Get-Clipboard -Raw -ErrorAction SilentlyContinue").unwrap_or_default();
        let marker = "vp-clip-test-标记123";
        ps_sta(&ps_set_clipboard_b64(&encode_clipboard_text(marker))).unwrap();
        let back = ps_sta("Get-Clipboard -Raw -ErrorAction Stop").unwrap();
        assert!(back.trim() == marker, "{back:?}");
        ps_sta(&ps_set_clipboard_b64(&encode_clipboard_text(&before))).unwrap_or_default();
    }

    #[test]
    fn type_text_approval_free_posture() {
        // 2026：clip.type_text 不在审批白名单 —— manifest 姿态 None。
        let m = clip_type_text_manifest();
        assert!(matches!(m.approval.mode, ApprovalMode::None), "{}", m.id);
    }

    #[test]
    fn press_keys_approval_free_posture() {
        // 2026：clip.press_keys 不在审批白名单 —— manifest 姿态 None。
        let m = clip_press_keys_manifest();
        assert!(matches!(m.approval.mode, ApprovalMode::None), "{}", m.id);
    }

    #[test]
    fn clip_write_executes_and_restores() {
        // AutoApprover 真写一次（测试标记），随后恢复，验证链路通。
        let _guard = clip_lock();
        let before = ps_sta("Get-Clipboard -Raw -ErrorAction SilentlyContinue").unwrap_or_default();
        let kernel = TrustKernel::open_in_memory().unwrap();
        let inputs = serde_json::json!({"text": "vp-write-验证"});
        let out = execute_clip_write(&kernel, &AutoApprover, "t3", "s3", &inputs).unwrap();
        assert!(out.contains("已写入剪贴板"), "{out}");
        ps_sta(&ps_set_clipboard_b64(&encode_clipboard_text(&before))).unwrap_or_default();
    }
}
