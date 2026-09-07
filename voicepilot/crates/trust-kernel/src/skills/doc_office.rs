//! Daisy 移植：Office 文档（Word/Excel/PPT 新建与转换，经本机 COM）。
//!
//! 对等 Daisy `office_document` 的 Windows 可用子集：create（空白文档写字）与
//! convert（含转 PDF），走本机 Office COM（`Word.Application` 等），需装
//! Office/WPS 专业版带 COM 注册；缺席诚实报错。原地 edit/inspect 语义高危
//! 且需 OfficeCLI 常驻解析，暂不支持（见 skill 描述）。PerStep 审批。

use crate::approval::approver::Approver;
use crate::error::{KernelError, Result};
use crate::kernel::TrustKernel;
use crate::skills::manifest::{EgressKind, SkillInputType, SkillManifest};
use crate::skills::simple::{
    ps_sta, run_simple, simple_manifest, slot_text, slot_text_opt, SimpleInput,
};
use std::collections::HashMap;
use std::path::Path;

fn text_slot(name: &'static str, max: u32) -> SimpleInput {
    SimpleInput {
        name,
        input_type: SkillInputType::Text,
        required: true,
        max_length: Some(max),
        allowed_roots: vec![],
    }
}

fn opt_slot(name: &'static str, max: u32) -> SimpleInput {
    SimpleInput {
        name,
        input_type: SkillInputType::Text,
        required: false,
        max_length: Some(max),
        allowed_roots: vec![],
    }
}

/// PS 单引号转义（路径/文本进单引号字符串）。
pub fn ps_quote(s: &str) -> String {
    s.replace('\'', "''")
}

/// Office 应用名 → (COM ProgID, 默认扩展名)。纯函数，可测。
pub fn office_app_id(app: &str) -> Option<(&'static str, &'static str)> {
    match app.trim().to_lowercase().as_str() {
        "word" | "word文档" | "文档" => Some(("Word.Application", "docx")),
        "excel" | "表格" | "电子表格" => Some(("Excel.Application", "xlsx")),
        "ppt" | "powerpoint" | "演示" | "幻灯片" => Some(("PowerPoint.Application", "pptx")),
        _ => None,
    }
}

/// SaveAs 格式码：(Word, Excel, PowerPoint)。PDF：17/57/32。
pub fn office_pdf_format(prog: &str) -> Option<i32> {
    match prog {
        "Word.Application" => Some(17),
        "Excel.Application" => Some(57),
        "PowerPoint.Application" => Some(32),
        _ => None,
    }
}

pub fn doc_office_manifest() -> SkillManifest {
    simple_manifest(
        "doc.office",
        "Office文档",
        "新建/转换 Word/Excel/PPT（含转 PDF），走本机 Office COM，需装 Office；原地编辑暂不支持（免审批）。",
        &["写文档", "做表格", "幻灯片", "转pdf"],
        &["新建一个word文档", "把这个表格转成pdf", "做个幻灯片"],
        vec![
            SimpleInput {
                name: "operation",
                input_type: SkillInputType::Text,
                required: true,
                max_length: Some(16),
                allowed_roots: vec![],
            },
            text_slot("app", 16),
            opt_slot("text", 8000),
            opt_slot("source", 500),
            opt_slot("target", 500),
        ],
        true,
        EgressKind::LocalOnly,
        &[],
    )
}

pub fn execute_doc_office(
    kernel: &TrustKernel,
    approver: &dyn Approver,
    task_id: &str,
    step_id: &str,
    inputs: &serde_json::Value,
) -> Result<String> {
    let operation = slot_text(inputs, "operation")?;
    let app = slot_text(inputs, "app")?;
    let mut map = HashMap::new();
    map.insert("operation".to_string(), serde_json::json!(operation));
    map.insert("app".to_string(), serde_json::json!(app));
    for k in ["text", "source", "target"] {
        if let Some(v) = slot_text_opt(inputs, k) {
            map.insert(k.to_string(), serde_json::json!(v));
        }
    }
    let text = slot_text_opt(inputs, "text").unwrap_or_default();
    let source = slot_text_opt(inputs, "source").unwrap_or_default();
    let target = slot_text_opt(inputs, "target").unwrap_or_default();
    run_simple(
        kernel,
        approver,
        task_id,
        step_id,
        "doc.office",
        &format!("office:{operation}:{app}"),
        true,
        &doc_office_manifest(),
        &map,
        || {
            let (prog, ext) = office_app_id(&app)
                .ok_or_else(|| KernelError::Skill(format!("app 须为 word/excel/ppt: {app}")))?;
            match operation.trim().to_lowercase().as_str() {
                "create" | "新建" | "创建" => {
                    if text.trim().is_empty() {
                        return Err(KernelError::Skill(
                            "create 需要 text（文档正文，按行分段）".to_string(),
                        ));
                    }
                    let dest = if target.trim().is_empty() {
                        let dir =
                            dirs::document_dir().unwrap_or_else(|| Path::new(".").to_path_buf());
                        dir.join(format!("新建文档.{ext}"))
                            .to_string_lossy()
                            .into_owned()
                    } else {
                        target.trim().to_string()
                    };
                    office_create(prog, &text, &dest)?;
                    Ok(format!("已新建：{dest}"))
                }
                "convert" | "转换" => {
                    if source.trim().is_empty() {
                        return Err(KernelError::Skill(
                            "convert 需要 source（源文件）".to_string(),
                        ));
                    }
                    let fmt = target
                        .rsplit('.')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_lowercase();
                    office_convert(prog, source.trim(), target.trim(), &fmt)?;
                    Ok(format!("已转换存为：{}", target.trim()))
                }
                other => Err(KernelError::Skill(format!(
                    "operation 仅支持 create/convert（原地 edit 暂不支持）: {other}"
                ))),
            }
        },
    )
}

/// COM 新建：正文按行写入（Word 段落 / Excel 首列 / PPT 首张文本框）。
fn office_create(prog: &str, text: &str, dest: &str) -> Result<()> {
    // base64 过 PS 单引号：正文任意字符安全。
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    let ps = match prog {
        "Word.Application" => format!(
            "$a=New-Object -ComObject Word.Application; $a.Visible=$false; $d=$a.Documents.Add(); \
             $t=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{b64}')); \
             foreach($line in $t -split \"`r?`n\"){{ $d.Content.InsertAfter($line); $d.Content.InsertParagraphAfter() }}; \
             $d.SaveAs('{dest}'); $d.Close(); $a.Quit()"
        ),
        "Excel.Application" => format!(
            "$a=New-Object -ComObject Excel.Application; $a.Visible=$false; $a.DisplayAlerts=$false; \
             $w=$a.Workbooks.Add(); $s=$w.Worksheets.Item(1); \
             $t=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{b64}')); \
             $r=1; foreach($line in $t -split \"`r?`n\"){{ $s.Cells.Item($r,1)=$line; $r++ }}; \
             $w.SaveAs('{dest}'); $w.Close(); $a.Quit()"
        ),
        _ => format!(
            "$a=New-Object -ComObject PowerPoint.Application; \
             $t=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{b64}')); \
             $p=$a.Presentations.Add(); $s=$p.Slides.Add(1, 1); \
             $s.Shapes.Title.TextFrame.TextRange.Text=($t -split \"`r?`n\" | Select-Object -First 1); \
             $body=($t -split \"`r?`n\" | Select-Object -Skip 1) -join \"`r`n\"; \
             if($body){{ $s.Shapes.Placeholders.Item(2).TextFrame.TextRange.Text=$body }}; \
             $p.SaveAs('{dest}'); $p.Close(); $a.Quit()"
        ),
    };
    let dest_q = ps_quote(dest);
    let ps = ps.replace("{dest}", &dest_q);
    ps_sta(&ps).map_err(|e| {
        KernelError::Skill(format!(
            "Office COM 新建失败（需安装 Office 且注册 COM）：{e}"
        ))
    })?;
    Ok(())
}

/// COM 转换：SaveAs 到目标格式（含 pdf）。
fn office_convert(prog: &str, source: &str, target: &str, fmt: &str) -> Result<()> {
    if target.trim().is_empty() {
        return Err(KernelError::Skill(
            "convert 需要 target（目标路径）".to_string(),
        ));
    }
    let code: i32 = if fmt.eq_ignore_ascii_case("pdf") {
        office_pdf_format(prog).ok_or_else(|| KernelError::Skill("未知应用".to_string()))?
    } else if fmt.eq_ignore_ascii_case("docx") && prog == "Word.Application" {
        16
    } else if fmt.eq_ignore_ascii_case("xlsx") && prog == "Excel.Application" {
        51
    } else if fmt.eq_ignore_ascii_case("pptx") && prog == "PowerPoint.Application" {
        24
    } else {
        return Err(KernelError::Skill(format!(
            "不支持的转换目标: {fmt}（支持 pdf/docx/xlsx/pptx，按应用匹配）"
        )));
    };
    let (src_q, dst_q) = (ps_quote(source), ps_quote(target));
    let ps = match prog {
        "Word.Application" => format!(
            "$a=New-Object -ComObject Word.Application; $a.Visible=$false; \
             $d=$a.Documents.Open('{src_q}'); $d.SaveAs('{dst_q}', {code}); $d.Close(); $a.Quit()"
        ),
        "Excel.Application" => format!(
            "$a=New-Object -ComObject Excel.Application; $a.Visible=$false; $a.DisplayAlerts=$false; \
             $w=$a.Workbooks.Open('{src_q}'); $w.SaveAs('{dst_q}', {code}); $w.Close(); $a.Quit()"
        ),
        _ => format!(
            "$a=New-Object -ComObject PowerPoint.Application; \
             $p=$a.Presentations.Open('{src_q}', $true, $false, $false); $p.SaveAs('{dst_q}', {code}); $p.Close(); $a.Quit()"
        ),
    };
    ps_sta(&ps).map_err(|e| {
        KernelError::Skill(format!(
            "Office COM 转换失败（需安装 Office 且注册 COM）：{e}"
        ))
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::approver::AutoApprover;
    use crate::kernel::TrustKernel;
    use crate::skills::manifest::ApprovalMode;

    #[test]
    fn office_manifest_has_id() {
        let m = doc_office_manifest();
        assert_eq!(m.id, "doc.office");
        assert!(!m.keywords.is_empty());
    }

    #[test]
    fn office_app_ids() {
        assert_eq!(office_app_id("word"), Some(("Word.Application", "docx")));
        assert_eq!(office_app_id("表格"), Some(("Excel.Application", "xlsx")));
        assert_eq!(
            office_app_id("幻灯片"),
            Some(("PowerPoint.Application", "pptx"))
        );
        assert_eq!(office_app_id("wps"), None);
        assert_eq!(office_pdf_format("Word.Application"), Some(17));
        assert_eq!(office_pdf_format("Excel.Application"), Some(57));
        assert_eq!(office_pdf_format("PowerPoint.Application"), Some(32));
    }

    #[test]
    fn ps_quote_doubles_quotes() {
        assert_eq!(ps_quote("a'b"), "a''b");
    }

    #[test]
    fn office_approval_free_posture_and_op_validation() {
        // 2026：doc.office 不在审批白名单 —— manifest 姿态 None。
        let m = doc_office_manifest();
        assert!(matches!(m.approval.mode, ApprovalMode::None), "{}", m.id);

        // 免审批后 op 校验照常生效：不合法 operation 报错，不触发 COM。
        let kernel = TrustKernel::open_in_memory().unwrap();
        let inputs = serde_json::json!({"operation": "edit", "app": "word"});
        let err = execute_doc_office(&kernel, &AutoApprover, "t1", "s1", &inputs).unwrap_err();
        assert!(
            format!("{err:?}").contains("操作*") || format!("{err:?}").contains("operation"),
            "{err:?}"
        );
    }
}
