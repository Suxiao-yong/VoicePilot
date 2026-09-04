use anyhow::{anyhow, Result};
use std::io::{self, Write};
use trust_kernel::approval::approver::{Approver, DagApprovalOutcome};
use trust_kernel::approval::types::ApprovalDecision;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::policy::transaction::EffectManifest;
use trust_kernel::skills::dag_types::DagPlan;
use trust_kernel::skills::executor::{FilesOrganizeInput, FilesOrganizeSkill};
use trust_kernel::state::TaskState;
use trust_kernel::mcp::handler::McpHandler;
use trust_kernel::mcp::repo::McpServerRepo;
use trust_kernel::mcp::server::McpServer;
use uuid::Uuid;

/// CLI Approver that prints the effect_manifest and prompts y/n on stdin.
struct CliApprover;

impl Approver for CliApprover {
    fn prompt(&self, manifest: &EffectManifest) -> ApprovalDecision {
        println!("\n=== Effect Manifest ===");
        println!("  sources: {} file(s)", manifest.sources.len());
        println!("  total_bytes: {}", manifest.total_bytes);
        println!("  destination: {}", manifest.destination);
        if !manifest.conflicts.is_empty() {
            println!("  conflicts: {:?}", manifest.conflicts);
        }
        println!("========================\n");
        print!("approve commit? [y/N] ");
        let _ = io::stdout().flush();
        let mut buf = String::new();
        let n = io::stdin().read_line(&mut buf).unwrap_or(0);
        if n == 0 {
            // EOF — treat as deny (safer default).
            return ApprovalDecision::Deny;
        }
        let trimmed = buf.trim().to_lowercase();
        if trimmed == "y" || trimmed == "yes" {
            ApprovalDecision::Allow
        } else {
            ApprovalDecision::Deny
        }
    }

    /// W8 Plan 2: DAG 骨架审批。
    ///
    /// CLI 实现:打印 DAG 计划摘要(plan_id / user_goal / nodes / edges),
    /// 然后 y/N 提示。EOF 或解析失败 → Deny(安全默认)。
    ///
    /// W9 Plan 4:返回类型从 `ApprovalDecision` 改为 `DagApprovalOutcome`。
    /// CLI 无 Modify 入口(交互式 y/N 只产生 Allow/Deny),行为等价 W8。
    fn approve_dag_skeleton(&self, plan: &DagPlan) -> trust_kernel::error::Result<DagApprovalOutcome> {
        println!("\n=== DAG Plan Skeleton ===");
        println!("  plan_id: {}", plan.plan_id);
        println!("  user_goal: {}", plan.user_goal);
        println!("  nodes: {} node(s)", plan.nodes.len());
        for n in &plan.nodes {
            println!("    - {} [skill={}]", n.node_id, n.skill_id);
        }
        if !plan.edges.is_empty() {
            println!("  edges: {}", plan.edges.len());
            for e in &plan.edges {
                println!("    - {} -> {}", e.from, e.to);
            }
        }
        if !plan.loop_specs.is_empty() {
            println!("  loop_specs: {} node(s)", plan.loop_specs.len());
        }
        println!("  max_total_steps: {}", plan.max_total_steps);
        println!("==========================\n");
        print!("approve DAG skeleton? [y/N] ");
        let _ = io::stdout().flush();
        let mut buf = String::new();
        let n = io::stdin().read_line(&mut buf).unwrap_or(0);
        if n == 0 {
            // EOF — treat as deny (safer default).
            return Ok(DagApprovalOutcome::Deny);
        }
        let trimmed = buf.trim().to_lowercase();
        if trimmed == "y" || trimmed == "yes" {
            Ok(DagApprovalOutcome::Allow)
        } else {
            Ok(DagApprovalOutcome::Deny)
        }
    }
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("info".parse()?),
        )
        .init();

    // W11 Plan 1: voicepilot eval --input <json> 子命令 — 评测用 JSON I/O。
    // 在打开 kernel 和打印 welcome 之前处理,确保 stdout 只输出 JSON(供 Inspect AI / promptfoo 解析)。
    // 评测模式不需要持久化 kernel —— SkillRouter 是无状态的,audit_trace 在内存构造。
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 2 && args[1] == "eval" {
        return handle_eval_command(&args[2..]);
    }

    let db_path = std::env::var("VOICEPILOT_DB")
        .unwrap_or_else(|_| "voicepilot.db".to_string());
    let kernel = TrustKernel::open_file(&db_path)
        .map_err(|e| anyhow!("failed to open kernel: {}", e))?;

    println!("VoicePilot W1 — text command entry (no voice yet)");
    println!("DB: {}", db_path);
    println!("Commands:");
    println!("  <text>          create a task with the given goal, run happy-path flow");
    println!("  cancel <task>   kill-switch cancel a task");
    println!("  show <task>     show task state and audit count");
    println!("  policy <tool> <path> <D-level> [E-level]  run policy decision (W2)");
    println!("  move <src1> [src2...] <dest>  move files via prepare→commit (W3a)");
    println!("  organize <root> <filter> <dest>  run files.organize Skill (W3b)");
    println!("  mcp-serve       start MCP server on stdio (W4)");
    #[cfg(feature = "voice")]
    println!("  voice list-models         List available SenseVoice models + download URLs");
    #[cfg(feature = "voice")]
    println!("  voice transcribe <file>   Transcribe a WAV file (mono 16-bit) to text");
    #[cfg(feature = "voice")]
    println!("  voice route <text>        Route text through SkillRouter (no audio, no execution)");
    #[cfg(feature = "voice")]
    println!("  voice-dag <text>          Route text via W8 DAG-aware router (keyword→LLM decompose→W7 fallback)");
    #[cfg(feature = "voice")]
    println!("  voice listen             Record 5s audio, transcribe, route to Skill");
    println!("  voice latency-stats [--since <dur>]  Show P50/P95/P99/max voice latency (W10 Plan 3)");
    println!("  voice latency-prune [--days <N>]     Prune voice latency samples older than N days (default 30)");
    println!("  audit coverage          Show audit event_type coverage (covered/uncovered/ratio, W10 Plan 5)");
    println!("  quit");
    println!();

    loop {
        print!("> ");
        io::stdout().flush()?;
        let mut line = String::new();
        let n = io::stdin().read_line(&mut line)?;
        if n == 0 {
            // EOF on stdin — exit cleanly (e.g. piped input exhausted).
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line == "quit" || line == "exit" {
            break;
        }
        if let Some(task_id) = line.strip_prefix("cancel ") {
            match kernel.transition(task_id.trim(), TaskState::Cancelled) {
                Ok(_) => println!("cancelled task {}", task_id.trim()),
                Err(e) => println!("error: {}", e),
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("policy ") {
            handle_policy_command(&kernel, rest);
            continue;
        }
        if let Some(rest) = line.strip_prefix("organize ") {
            handle_organize_command(&kernel, rest);
            continue;
        }
        #[cfg(feature = "voice")]
        {
            if line == "voice list-models" {
                handle_voice_list_models_command();
                return Ok(());
            }
            if let Some(rest) = line.strip_prefix("voice transcribe ") {
                let path = rest.trim();
                handle_voice_transcribe_command(path)?;
                return Ok(());
            }
            if let Some(rest) = line.strip_prefix("voice route ") {
                let text = rest.trim();
                handle_voice_route_command(text)?;
                return Ok(());
            }
            // W8 Plan 4: voice-dag <text> 子命令 — 调用 route_text_with_dag
            // (keyword→LLM decompose→W7 fallback 三级路由)。
            if let Some(rest) = line.strip_prefix("voice-dag ") {
                let text = rest.trim();
                let rt = tokio::runtime::Runtime::new()
                    .map_err(|e| anyhow!("failed to create tokio runtime: {}", e))?;
                rt.block_on(handle_voice_dag_command(&kernel, text))?;
                return Ok(());
            }
            if line == "voice listen" {
                handle_voice_listen_command()?;
                return Ok(());
            }
        }
        // W10 Plan 3: voice latency admin 命令(default-gated,纯 DB 操作)。
        // 不放在 #[cfg(feature = "voice")] 块内 —— 即使 voice feature 关闭,
        // admin 也能查询 / 清理历史样本(只要 trust-kernel default migration 008 已运行)。
        if let Some(rest) = line.strip_prefix("voice latency-stats") {
            handle_voice_latency_stats_command(&kernel, rest.trim());
            continue;
        }
        if let Some(rest) = line.strip_prefix("voice latency-prune") {
            handle_voice_latency_prune_command(&kernel, rest.trim());
            continue;
        }
        // W10 Plan 5: audit coverage 命令(default-gated,纯 DB 操作)。
        if line == "audit coverage" {
            handle_audit_coverage_command(&kernel);
            continue;
        }
        if line == "mcp-serve" {
            // Terminal command — consumes kernel and exits.
            handle_mcp_serve_command(kernel);
            return Ok(());
        }
        if let Some(rest) = line.strip_prefix("move ") {
            handle_move_command(&kernel, rest);
            continue;
        }
        if let Some(task_id) = line.strip_prefix("show ") {
            match kernel.get_task(task_id.trim()) {
                Ok(Some(t)) => {
                    let audit = kernel.audit_count_for_task(task_id.trim()).unwrap_or(0);
                    println!("task {} | status={:?} | audit_events={}", t.task_id, t.status, audit);
                    println!("  goal: {}", t.user_goal);
                }
                Ok(None) => println!("task not found"),
                Err(e) => println!("error: {}", e),
            }
            continue;
        }
        // Treat the entire line as a goal — run the W1 happy path.
        let task_id = Uuid::new_v4().to_string();
        match run_happy_path(&kernel, &task_id, line) {
            Ok(_) => println!("done: task_id={}", task_id),
            Err(e) => println!("failed: {}", e),
        }
    }
    Ok(())
}

/// Run the W1 happy-path stub: IDLE → LISTENING → PLANNING → AWAITING_APPROVAL
/// → EXECUTING → VERIFYING → DONE. No real tools — just state + audit.
fn run_happy_path(kernel: &TrustKernel, task_id: &str, goal: &str) -> Result<()> {
    kernel
        .create_task(task_id, goal)
        .map_err(|e| anyhow!("create_task: {}", e))?;
    for next in [
        TaskState::Listening,
        TaskState::Planning,
        TaskState::AwaitingApproval,
        TaskState::Executing,
        TaskState::Verifying,
        TaskState::Done,
    ] {
        kernel
            .transition(task_id, next)
            .map_err(|e| anyhow!("transition {:?}: {}", next, e))?;
    }
    Ok(())
}

fn handle_policy_command(kernel: &TrustKernel, args: &str) {
    let parts: Vec<&str> = args.split_whitespace().collect();
    if parts.len() < 3 {
        println!("usage: policy <tool> <path> <D-level> [E-level]");
        println!("       D-level: D0|D1|D2|D3   E-level: E0|E1|E2|E3 (default E0)");
        return;
    }
    let tool = parts[0];
    let path = parts[1];
    let d_level = match parts[2] {
        "D0" => trust_kernel::policy::types::DLevel::D0,
        "D1" => trust_kernel::policy::types::DLevel::D1,
        "D2" => trust_kernel::policy::types::DLevel::D2,
        "D3" => trust_kernel::policy::types::DLevel::D3,
        _ => {
            println!("invalid D-level: {}", parts[2]);
            return;
        }
    };
    let e_level = if parts.len() >= 4 {
        match parts[3] {
            "E0" => trust_kernel::policy::types::ELevel::E0,
            "E1" => trust_kernel::policy::types::ELevel::E1,
            "E2" => trust_kernel::policy::types::ELevel::E2,
            "E3" => trust_kernel::policy::types::ELevel::E3,
            _ => {
                println!("invalid E-level: {}", parts[3]);
                return;
            }
        }
    } else {
        trust_kernel::policy::types::ELevel::E0
    };

    let resource = trust_kernel::policy::types::Resource {
        path: path.to_string(),
        data_class: d_level,
        provenance: "user_direct".to_string(),
    };
    let egress = if tool == "send_to_remote_llm" {
        Some(trust_kernel::policy::types::EgressDest::RemoteLlm)
    } else {
        None
    };

    match kernel.gateway().decide(tool, e_level, &resource, egress, None) {
        Ok(decision) => {
            println!("decision: {:?}", decision.effect);
            println!("  bundle_hash: {}", decision.policy_bundle_hash);
            println!("  matched_policies: {:?}", decision.matched_policies);
            if !decision.constraints_applied.is_empty() {
                println!("  constraints: {:?}", decision.constraints_applied);
            }
            if !decision.reasons.is_empty() {
                println!("  reasons: {:?}", decision.reasons);
            }
        }
        Err(e) => println!("error: {}", e),
    }
}

fn handle_move_command(kernel: &TrustKernel, args: &str) {
    let parts: Vec<&str> = args.split_whitespace().collect();
    if parts.len() < 2 {
        println!("usage: move <src1> [src2...] <dest>");
        return;
    }
    let (sources, dest) = parts.split_at(parts.len() - 1);
    let dest = std::path::PathBuf::from(dest[0]);
    let srcs: Vec<std::path::PathBuf> = sources.iter().map(std::path::PathBuf::from).collect();
    let src_refs: Vec<&std::path::Path> = srcs.iter().map(|p| p.as_path()).collect();

    // Phase 1: prepare.
    let prepared = match kernel.filesystem().prepare_move(
        "cli-task", "cli-step", &src_refs, &dest, kernel.transaction_manager(),
    ) {
        Ok(p) => p,
        Err(e) => {
            println!("prepare failed: {}", e);
            return;
        }
    };
    println!("prepare OK: {} sources, {} bytes, {} conflicts",
             prepared.manifest.sources.len(),
             prepared.manifest.total_bytes,
             prepared.manifest.conflicts.len());
    println!("  prepare_token: {}", prepared.token.token);
    println!("  preconditions_hash: {}", prepared.preconditions_hash);

    // Phase 2: commit.
    let committed = match kernel.filesystem().commit_move(
        &prepared.token, &prepared.manifest, kernel.transaction_manager(),
    ) {
        Ok(c) => c,
        Err(e) => {
            println!("commit failed: {}", e);
            return;
        }
    };
    println!("commit OK: moved {} files", committed.moved_paths.len());

    // Phase 3: verify (Strong Verifier).
    match kernel.filesystem().verify_move(&prepared.manifest) {
        Ok(v) => println!("verify OK: evidence_strength = {:?}", v.evidence_strength),
        Err(e) => println!("verify FAILED: {}", e),
    }

    // Phase 4: compensation record creation deferred to W3b (needs kernel method to access conn).
    // W3a CLI smoke stops here; W3b will add `kernel.create_compensation(rec)`.
    println!("(compensation record creation deferred to W3b — see Task 9 of W3a plan)");
}

fn handle_organize_command(kernel: &TrustKernel, args: &str) {
    let parts: Vec<&str> = args.split_whitespace().collect();
    if parts.len() != 3 {
        println!("usage: organize <root> <filter> <dest>");
        println!("  e.g. organize %TEMP%\\dl *.pdf %TEMP%\\papers");
        return;
    }
    let root = std::path::PathBuf::from(parts[0]);
    let filter = parts[1].to_string();
    let dest = std::path::PathBuf::from(parts[2]);

    // Create a task + step for this organize run.
    let task_id = format!("task-{}", Uuid::new_v4());
    let step_id = format!("step-{}", Uuid::new_v4());
    let goal = format!("organize {} ({}) -> {}", parts[0], parts[1], parts[2]);
    if let Err(e) = kernel.create_task(&task_id, &goal) {
        println!("create_task failed: {}", e);
        return;
    }
    if let Err(e) = kernel.create_step(&trust_kernel::repo::step_repo::StepRecord::new(
        &step_id, &task_id, 1,
    )) {
        println!("create_step failed: {}", e);
        return;
    }

    let input = FilesOrganizeInput {
        task_id: task_id.clone(),
        step_id: step_id.clone(),
        source: root,
        filter,
        destination: dest,
    };

    let approver = CliApprover;
    let skill = FilesOrganizeSkill::new();
    match skill.execute(kernel, &input, &approver) {
        Ok(execution) => {
            println!("\n--- ToolResult V2 ---");
            println!("  status: {:?}", execution.tool_result.status);
            println!("  evidence_strength: {:?}", execution.tool_result.evidence_strength);
            println!("  compensation_ref: {:?}", execution.tool_result.compensation_ref);
            println!("  compensation_level: {}", execution.tool_result.compensation_level.as_str());
            println!("  idempotency_key: {}", execution.tool_result.idempotency_key);
            println!("  moved: {} file(s)", execution.moved_paths.len());
            println!("  task_id: {}", task_id);
            println!("  step_id: {}", step_id);
        }
        Err(e) => {
            println!("skill execution failed: {}", e);
        }
    }
}

fn handle_mcp_serve_command(kernel: TrustKernel) {
    // Seed builtin server row (idempotent).
    let repo = McpServerRepo::new();
    if let Err(e) = repo.seed_builtin_filesystem(&kernel.conn()) {
        eprintln!("error seeding builtin mcp_servers row: {}", e);
        return;
    }
    // Load allowed_paths from the builtin row.
    let allowed = repo
        .load_allowed_paths(&kernel.conn(), "voicepilot-filesystem")
        .ok()
        .flatten();
    if let Some(allowed) = allowed {
        kernel.replace_filesystem_with_allowed_paths(allowed);
    }
    // Construct McpServer and run stdio loop.
    // mcp-serve is terminal — consumes kernel and exits when stdin closes.
    // W9 Plan 3: server_id="voicepilot-stdio" 用于 mcp_tool:<server_id> taint provenance。
    let server = McpServer::new(McpHandler::new(), kernel).with_server_id("voicepilot-stdio");
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    if let Err(e) = server.run_stdio(stdin.lock(), &mut stdout) {
        eprintln!("mcp-serve error: {}", e);
    }
}

#[cfg(feature = "voice")]
fn handle_voice_list_models_command() {
    use trust_kernel::voice::model::ModelRegistry;

    let registry = ModelRegistry::new();
    let models = registry.all_known_models();

    println!("Available SenseVoice models (sherpa-onnx):");
    println!();
    for m in &models {
        // SenseVoice 是目录模型(sherpa-onnx),以 model.onnx + tokens.txt 判定完整性。
        let complete = m.path.join("model.onnx").is_file() && m.path.join("tokens.txt").is_file();
        let present = if complete { "[installed]" } else { "[missing]  " };
        println!("  {} {} ({} MB)", present, m.name, m.size_hint_mb);
        println!("       path: {}", m.path.display());
        println!("       url:  {}", m.download_url);
        println!();
    }
    println!("Default model: {}", registry.default_model().name);
    println!();
    println!(
        "To install: download the .tar.bz2 archive from the URL above, verify the pinned SHA-256 \
         digest, then extract it into the path shown (must contain model.onnx + tokens.txt)."
    );
}

#[cfg(feature = "voice")]
fn handle_voice_transcribe_command(path: &str) -> anyhow::Result<()> {
    use trust_kernel::voice::asr::{SherpaAsrConfig, SherpaAsrEngine};
    use trust_kernel::voice::model::{ModelRegistry, SENSE_VOICE_DIR_NAME};
    use trust_kernel::voice::wav::read_wav;

    let registry = ModelRegistry::new();
    let model_dir = registry
        .resolve(SENSE_VOICE_DIR_NAME)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let engine = SherpaAsrEngine::new(SherpaAsrConfig {
        model_dir,
        language: None, // auto-detect
        ..Default::default()
    })
    .map_err(|e| anyhow::anyhow!("{}", e))?;

    let (samples, sample_rate) = read_wav(std::path::Path::new(path))
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    // Resample to 16kHz if needed.
    let samples_16k = if sample_rate != 16000 {
        resample_linear_cli(&samples, sample_rate, 16000)
    } else {
        samples
    };

    let text = engine
        .transcribe(&samples_16k)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    println!("Transcription:");
    println!("{}", text);
    Ok(())
}

#[cfg(feature = "voice")]
fn resample_linear_cli(samples: &[i16], from: u32, to: u32) -> Vec<i16> {
    if from == to || samples.is_empty() {
        return samples.to_vec();
    }
    let ratio = to as f64 / from as f64;
    let out_len = ((samples.len() as f64) * ratio) as usize;
    (0..out_len)
        .map(|i| {
            let src_idx = i as f64 / ratio;
            let lo = src_idx.floor() as usize;
            let hi = (lo + 1).min(samples.len() - 1);
            let frac = src_idx - lo as f64;
            let lo_f = samples[lo] as f64;
            let hi_f = samples[hi] as f64;
            (lo_f + (hi_f - lo_f) * frac) as i16
        })
        .collect()
}

#[cfg(feature = "voice")]
fn handle_voice_route_command(text: &str) -> anyhow::Result<()> {
    use trust_kernel::approval::approver::AutoApprover;
    use trust_kernel::voice::router_bridge::{route_text, RouteOutcome};
    use trust_kernel::kernel::TrustKernel;

    // Open in-memory kernel for routing only (no execution needed for route preview).
    let kernel = TrustKernel::open_in_memory()?;
    let approver = AutoApprover;

    let outcome = route_text(&kernel, &approver, text)?;

    match outcome {
        RouteOutcome::Routed { skill_id, .. } => {
            println!("Matched skill: {}", skill_id);
            Ok(())
        }
        // W8 Plan 4:route_text (sync) 不返回 DagPlan(它把 RouteDecision::Dag
        // 映射为 Unmatched);此处防御性 arm 保持 match 穷尽。DAG 路由请用
        // `voice-dag <text>` 子命令(调 route_text_with_dag)。
        #[cfg(feature = "llm")]
        RouteOutcome::DagPlan(_) => {
            println!("(DAG plan not handled by `voice route` — use `voice-dag <text>` for DAG routing)");
            Ok(())
        }
        RouteOutcome::Chat { text } => {
            println!("Chat: {}", text);
            Ok(())
        }
        RouteOutcome::Unmatched { text } => {
            println!("No skill matched for: {:?}", text);
            println!("(W7 LLM Planner fallback not yet implemented)");
            Ok(())
        }
        RouteOutcome::Empty => {
            println!("Empty input");
            Ok(())
        }
    }
}

/// W8 Plan 4: `voice-dag <text>` 子命令。
///
/// 调用 `route_text_with_dag`(三级路由:keyword→LLM decompose→W7 fallback),
/// 根据 `RouteOutcome` 分支打印:
/// - `Routed { skill_id }`:打印匹配的 Skill id
/// - `DagPlan(plan)`:打印 DAG 节点列表 + 询问 Allow/Deny(本 Plan 不执行 DAG,
///   实际执行由 Plan 6 集成测试 / DagExecutor 覆盖)
/// - `Unmatched { text }`:打印未匹配(W7 Planner fallback)
/// - `Empty`:打印空输入
#[cfg(feature = "voice")]
async fn handle_voice_dag_command(kernel: &TrustKernel, text: &str) -> anyhow::Result<()> {
    use trust_kernel::voice::router_bridge::{route_text_with_dag, RouteOutcome};

    let outcome = route_text_with_dag(kernel, text)
        .await
        .map_err(|e| anyhow!("route_text_with_dag failed: {}", e))?;

    match outcome {
        RouteOutcome::Routed { skill_id } => {
            println!("Routed → skill_id: {}", skill_id);
            println!("(W8 Plan 4: 执行由 Plan 5 UI / Plan 6 e2e 覆盖)");
        }
        #[cfg(feature = "llm")]
        RouteOutcome::DagPlan(plan) => {
            println!("DagPlan → plan_id: {}", plan.plan_id);
            println!("  user_goal: {}", plan.user_goal);
            println!("  max_total_steps: {}", plan.max_total_steps);
            println!("  nodes ({}):", plan.nodes.len());
            for node in &plan.nodes {
                println!(
                    "    - {} | skill_id: {} | risk: {:?}",
                    node.node_id, node.skill_id, node.risk_ceiling
                );
            }
            println!("  edges ({}):", plan.edges.len());
            for edge in &plan.edges {
                println!(
                    "    - {} → {} (port: {:?})",
                    edge.from, edge.to, edge.port_binding
                );
            }
            // 简单 Allow/Deny 询问(本 Plan 不实际执行 DAG)。
            print!("\nAllow DAG skeleton execution? [y/N] ");
            let _ = std::io::Write::flush(&mut std::io::stdout());
            let mut buf = String::new();
            let n = std::io::stdin().read_line(&mut buf).unwrap_or(0);
            if n == 0 {
                println!("(EOF → Deny)");
            } else if buf.trim().eq_ignore_ascii_case("y") {
                println!("(W8 Plan 4: DAG 执行由 DagExecutor 实现,本 Plan 仅路由)");
                // Plan 6 集成测试会调 DagExecutor::run。
            } else {
                println!("Denied — DAG not executed.");
            }
        }
        RouteOutcome::Chat { text } => {
            println!("Chat → {}", text);
        }
        RouteOutcome::Unmatched { text } => {
            println!("Unmatched → text: {}", text);
            println!("(W7 Planner fallback)");
        }
        RouteOutcome::Empty => {
            println!("Empty — no input text.");
        }
    }
    Ok(())
}

#[cfg(feature = "voice")]
fn handle_voice_listen_command() -> anyhow::Result<()> {
    use trust_kernel::approval::approver::AutoApprover;
    use trust_kernel::voice::audio::{AudioRecorder, AudioRecorderConfig};
    use trust_kernel::voice::model::{ModelRegistry, SENSE_VOICE_DIR_NAME};
    use trust_kernel::voice::router_bridge::{route_text, RouteOutcome};
    use trust_kernel::voice::vad::{VadConfig, VadDetector, VadOutcome};
    use trust_kernel::voice::asr::{SherpaAsrConfig, SherpaAsrEngine};
    use trust_kernel::kernel::TrustKernel;

    // 1. Record up to 5 seconds of audio.
    println!("Listening (5 seconds)...");
    let recorder = AudioRecorder::new(AudioRecorderConfig::default())
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    let samples = recorder
        .record_with_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    println!("Captured {} samples", samples.len());

    // 2. Run VAD — skip transcription if no speech.
    let vad = VadDetector::new(VadConfig::default());
    match vad.detect(&samples) {
        VadOutcome::Speech { .. } => {}
        VadOutcome::NoSpeech => {
            println!("No speech detected.");
            return Ok(());
        }
    }

    // 3. Load Sherpa ASR model.
    let registry = ModelRegistry::new();
    let model_dir = registry
        .resolve(SENSE_VOICE_DIR_NAME)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    // 4. Transcribe.
    let engine = SherpaAsrEngine::new(SherpaAsrConfig {
        model_dir,
        language: None,
        ..Default::default()
    })
    .map_err(|e| anyhow::anyhow!("{}", e))?;
    let text = engine
        .transcribe(&samples)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    println!("Transcription: {:?}", text);

    // 5. Route.
    let kernel = TrustKernel::open_in_memory()?;
    let approver = AutoApprover;
    let outcome = route_text(&kernel, &approver, &text)?;
    match outcome {
        RouteOutcome::Routed { skill_id, .. } => {
            println!("Matched skill: {}", skill_id);
            println!("(Skill execution requires user-supplied args; use `voicepilot organize` to run)");
        }
        // W8 Plan 4:route_text (sync) 内部已把 RouteDecision::Dag(_) 映射为
        // Unmatched,理论上不会到达此 arm;此处防御性 arm 保持 match 穷尽。
        // voice listen pipeline 不调 route_text_with_dag,DAG 审批 UI 由 Plan 5 实现。
        #[cfg(feature = "llm")]
        RouteOutcome::DagPlan(_) => {
            println!("(DAG plan not handled in voice listen pipeline — use `voice-dag <text>` for DAG routing)");
        }
        RouteOutcome::Chat { text } => {
            println!("Chat: {}", text);
        }
        RouteOutcome::Unmatched { text } => {
            println!("No skill matched for: {:?}", text);
        }
        RouteOutcome::Empty => {
            println!("Empty transcription");
        }
    }
    Ok(())
}

// ===== W10 Plan 3: voice latency admin 命令(default-gated)=====

/// W10 Plan 3: `voice latency-stats [--since <duration>]` 命令处理。
///
/// 输出 P50/P95/P99/max + 样本数。
/// `--since` 可选,格式如 `24h` / `7d` / `3600s` / `60m`,None = 全部样本。
///
/// **feature gate:** default(纯 DB 操作,不依赖 voice feature)。
/// 即使 voice feature 关闭,admin 也能查询历史样本(只要 migration 008 已运行)。
fn handle_voice_latency_stats_command(kernel: &TrustKernel, args: &str) {
    let since_offset_ms: Option<i64> = if let Some(dur_str) = args.strip_prefix("--since ") {
        match parse_duration_to_ms(dur_str.trim()) {
            Some(ms) => Some(ms),
            None => {
                println!("invalid --since duration: {} (supported: 24h / 7d / 3600s / 60m)", dur_str);
                return;
            }
        }
    } else if !args.is_empty() {
        println!("usage: voice latency-stats [--since <duration>]");
        println!("       duration format: 24h / 7d / 3600s / 60m");
        return;
    } else {
        None
    };

    // since_offset_ms 是 "距今 N ms" 的下界,转为 epoch ms:now - offset
    let since_epoch_ms = since_offset_ms.map(|offset| {
        chrono::Utc::now().timestamp_millis() - offset
    });

    match kernel.compute_voice_latency_stats(since_epoch_ms) {
        Ok(stats) => {
            println!("voice latency stats:");
            println!("  sample_count: {}", stats.sample_count);
            println!("  p50_ms: {}", stats.p50_ms);
            println!("  p95_ms: {}", stats.p95_ms);
            println!("  p99_ms: {}", stats.p99_ms);
            println!("  max_ms: {}", stats.max_ms);
            if stats.sample_count == 0 {
                println!("  (no samples — run 'voice listen' to generate)");
            }
        }
        Err(e) => println!("error: {}", e),
    }
}

/// W10 Plan 3: `voice latency-prune [--days <N>]` 命令处理。
///
/// 清理早于 N 天的样本,默认 30 天。输出删除行数。
fn handle_voice_latency_prune_command(kernel: &TrustKernel, args: &str) {
    let days: u32 = if let Some(days_str) = args.strip_prefix("--days ") {
        match days_str.trim().parse::<u32>() {
            Ok(d) => d,
            Err(_) => {
                println!("invalid --days value: {} (expected positive integer)", days_str);
                return;
            }
        }
    } else if args.is_empty() {
        30 // 默认 30 天(spec §5.2 v2 修订 #15)
    } else {
        println!("usage: voice latency-prune [--days <N>]");
        println!("       default: 30 days");
        return;
    };

    let now_ms = chrono::Utc::now().timestamp_millis();
    match kernel.prune_voice_latency_older_than(days, now_ms) {
        Ok(deleted) => {
            println!("pruned {} voice latency samples older than {} days", deleted, days);
        }
        Err(e) => println!("error: {}", e),
    }
}

/// 解析时长字符串为毫秒。支持 `24h` / `7d` / `3600s` / `60m`。
///
/// 单字符后缀:h=小时,m=分钟,s=秒,d=天。不支持组合(如 `1h30m`)。
/// 空串或无法解析返回 None。
fn parse_duration_to_ms(s: &str) -> Option<i64> {
    if s.is_empty() {
        return None;
    }
    let (num_str, unit) = s.split_at(s.len() - 1);
    let num: i64 = num_str.parse().ok()?;
    let ms = match unit {
        "s" => num * 1000,
        "m" => num * 60 * 1000,
        "h" => num * 3600 * 1000,
        "d" => num * 86_400 * 1000,
        _ => return None,
    };
    Some(ms)
}

// ===== W10 Plan 5: audit coverage admin 命令(default-gated)=====

/// W10 Plan 5: `audit coverage` 命令处理。
///
/// 输出 AUDIT_EVENT_TYPE_REGISTRY 中所有 event_type 的覆盖情况:
/// - 已覆盖事件数 / 总 registry 数
/// - 覆盖率百分比
/// - 未覆盖 event_type 列表(字母序)
///
/// **feature gate:** default(纯 DB 操作,不依赖 voice/stronghold feature)。
/// 即使 voice/stronghold feature 关闭,admin 也能查询已写入 audit_logs 的事件覆盖率。
/// 注意:default feature 下 voice_started / stronghold_snapshot_encrypted /
/// stronghold_snapshot_decrypt_failed 3 种事件不可触发,会出现在 uncovered 列表中。
fn handle_audit_coverage_command(kernel: &TrustKernel) {
    use trust_kernel::audit_coverage::AuditCoverageChecker;

    let checker = AuditCoverageChecker::new(kernel);
    match (checker.covered(), checker.uncovered(), checker.coverage_ratio()) {
        (Ok(covered), Ok(uncovered), Ok(ratio)) => {
            let total = checker.expected().len();
            println!("audit event_type coverage:");
            println!("  covered:   {} / {}", covered.len(), total);
            println!("  ratio:     {:.1}%", ratio * 100.0);
            if uncovered.is_empty() {
                println!("  uncovered: (none — 100% coverage)");
            } else {
                println!("  uncovered ({}):", uncovered.len());
                for et in &uncovered {
                    println!("    - {}", et);
                }
            }
        }
        (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => {
            println!("error computing audit coverage: {}", e);
        }
    }
}

// ===== W11 Plan 1: voicepilot eval 子命令(JSON I/O,供 Inspect AI 调用)=====

/// W11 Plan 1: `voicepilot eval --input <json>` 子命令。
///
/// 评测用 JSON I/O,供 Inspect AI / promptfoo 调用。
///
/// 输入 JSON schema:
/// ```json
/// {"transcript": "用户输入", "mode": "auto|interactive"}
/// ```
///
/// 输出 JSON schema:
/// ```json
/// {
///   "task_id": "uuid",
///   "transcript": "用户输入",
///   "skill_id": "files.organize|null",
///   "risk_level": "E2×D2",
///   "approval_decision": "auto|allow|deny",
///   "commit_status": "success|failed|aborted|skipped",
///   "blocked": false,
///   "block_reason": "policy_deny|approver_deny|llm_refuse|none",
///   "audit_trace": [...],
///   "error": null
/// }
/// ```
///
/// 评测模式不真实执行 Skill(避免文件系统副作用),只路由 + 构造 audit_trace。
/// auto mode 下 E3/D3 走 AutoDenier(blocked=true),其他走 AutoApprover(skipped)。
fn handle_eval_command(args: &[String]) -> Result<()> {
    use trust_kernel::skills::manifest::{
        app_control_manifest, files_organize_manifest, form_prepare_manifest,
        form_submit_manifest, note_capture_manifest, research_save_manifest,
        task_compensate_manifest, task_explain_manifest, task_repeat_verified_manifest,
    };
    use trust_kernel::skills::router::{RouteDecision, SkillRouter};
    use trust_kernel::skills::redteam::classify_malicious_intent;

    // 解析 --input <json>
    let mut input_json: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--input" && i + 1 < args.len() {
            input_json = Some(&args[i + 1]);
            i += 2;
        } else {
            i += 1;
        }
    }

    let input_str = input_json.ok_or_else(|| anyhow!("missing --input <json>"))?;
    let input: serde_json::Value = serde_json::from_str(input_str)
        .map_err(|e| anyhow!("invalid JSON in --input: {}", e))?;

    let transcript = input
        .get("transcript")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("missing 'transcript' field in --input JSON"))?;
    let mode = input.get("mode").and_then(|v| v.as_str()).unwrap_or("auto");

    // W11 Plan 2: red team 恶意输入分类 — 评测环境用确定性启发式模拟
    // LLM refuse / Policy deny / Approver deny 三条拦截路径(生产环境由
    // 真实 LLM 拒绝 + Policy 硬拒 + 审批拒绝完成)。命中即 blocked。
    if let Some(intent) = classify_malicious_intent(transcript) {
        let task_id = Uuid::new_v4().to_string();
        let audit_trace = vec![
            serde_json::json!({
                "event_type": "task_created",
                "task_id": task_id,
                "transcript": transcript,
            }),
            serde_json::json!({
                "event_type": "malicious_intent_detected",
                "category": intent.category.as_str(),
                "block_reason": intent.block_reason.as_str(),
                "matched_pattern": intent.matched_pattern,
            }),
        ];
        let result = serde_json::json!({
            "task_id": task_id,
            "transcript": transcript,
            "skill_id": serde_json::Value::Null,
            "risk_level": "E3×D3",
            "approval_decision": "deny",
            "commit_status": "aborted",
            "blocked": true,
            "block_reason": intent.block_reason.as_str(),
            "audit_trace": audit_trace,
            "error": serde_json::Value::Null,
        });
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    // 注册内置 Skill 到 router
    let mut router = SkillRouter::new();
    router.register(files_organize_manifest());
    router.register(note_capture_manifest());
    router.register(research_save_manifest());
    router.register(form_prepare_manifest());
    router.register(form_submit_manifest());
    router.register(task_explain_manifest());
    router.register(task_repeat_verified_manifest());
    router.register(task_compensate_manifest());
    router.register(app_control_manifest());

    let decision = router.route(transcript);

    let task_id = Uuid::new_v4().to_string();
    let mut audit_trace: Vec<serde_json::Value> = Vec::new();

    let (skill_id, risk_level, approval_decision, commit_status, blocked, block_reason, error) =
        match &decision {
            RouteDecision::Skill(manifest) => {
                let sid = manifest.id.clone();
                let e_level = manifest.risk_ceiling as u8;
                let d_level = manifest.data_class_ceiling as u8;
                let risk = format!("E{}×D{}", e_level, d_level);
                // auto mode: E3/D3 走 AutoDenier,其他走 AutoApprover
                let (approval, blocked, reason) = if mode == "auto" {
                    if e_level >= 3 || d_level >= 3 {
                        ("deny", true, "approver_deny")
                    } else {
                        ("auto", false, "none")
                    }
                } else {
                    ("allow", false, "none")
                };
                // 评测模式不真实执行 Skill(避免文件系统副作用),
                // commit_status 标记为 skipped(AutoApprover)或 aborted(AutoDenier)
                let commit = if blocked { "aborted" } else { "skipped" };
                audit_trace.push(serde_json::json!({
                    "event_type": "task_created",
                    "task_id": task_id,
                    "transcript": transcript,
                }));
                audit_trace.push(serde_json::json!({
                    "event_type": "skill_routed",
                    "skill_id": sid,
                    "risk_level": risk,
                }));
                audit_trace.push(serde_json::json!({
                    "event_type": "approval_decided",
                    "decision": approval,
                    "mode": mode,
                }));
                (
                    serde_json::Value::String(sid),
                    risk,
                    approval.to_string(),
                    commit.to_string(),
                    blocked,
                    reason.to_string(),
                    serde_json::Value::Null,
                )
            }
            RouteDecision::Planner => {
                audit_trace.push(serde_json::json!({
                    "event_type": "task_created",
                    "task_id": task_id,
                    "transcript": transcript,
                }));
                audit_trace.push(serde_json::json!({
                    "event_type": "route_fallback_to_planner",
                    "reason": "no skill matched",
                }));
                (
                    serde_json::Value::Null,
                    "L0".to_string(),
                    "auto".to_string(),
                    "skipped".to_string(),
                    false,
                    "none".to_string(),
                    serde_json::Value::Null,
                )
            }
            #[cfg(feature = "llm")]
            _ => {
                // SkillWithSlots / Dag 在评测模式简化处理:标 skill_id=null
                audit_trace.push(serde_json::json!({
                    "event_type": "task_created",
                    "task_id": task_id,
                    "transcript": transcript,
                }));
                (
                    serde_json::Value::Null,
                    "L0".to_string(),
                    "auto".to_string(),
                    "skipped".to_string(),
                    false,
                    "none".to_string(),
                    serde_json::Value::Null,
                )
            }
        };

    let result = serde_json::json!({
        "task_id": task_id,
        "transcript": transcript,
        "skill_id": skill_id,
        "risk_level": risk_level,
        "approval_decision": approval_decision,
        "commit_status": commit_status,
        "blocked": blocked,
        "block_reason": block_reason,
        "audit_trace": audit_trace,
        "error": error,
    });

    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
