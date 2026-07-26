use anyhow::{anyhow, Result};
use std::io::{self, Write};
use trust_kernel::approval::approver::Approver;
use trust_kernel::approval::types::ApprovalDecision;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::policy::transaction::EffectManifest;
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
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("info".parse()?),
        )
        .init();

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
    println!("  voice list-models         List available Whisper models + download URLs");
    #[cfg(feature = "voice")]
    println!("  voice transcribe <file>   Transcribe a WAV file (mono 16-bit) to text");
    #[cfg(feature = "voice")]
    println!("  voice route <text>        Route text through SkillRouter (no audio, no execution)");
    #[cfg(feature = "voice")]
    println!("  voice listen             Record 5s audio, transcribe, route to Skill");
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
            if line == "voice listen" {
                handle_voice_listen_command()?;
                return Ok(());
            }
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
    let server = McpServer::new(McpHandler::new(), kernel);
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

    println!("Available Whisper models:");
    println!();
    for m in &models {
        let present = if m.path.is_file() { "[installed]" } else { "[missing]  " };
        println!("  {} {} ({} MB)", present, m.name, m.size_hint_mb);
        println!("       path: {}", m.path.display());
        println!("       url:  {}", m.download_url);
        println!();
    }
    println!("Default model: {}", registry.default_model().name);
    println!();
    println!("To install: download the .bin file from the URL above and place it at the path shown.");
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
        RouteOutcome::Unmatched { text } => {
            println!("No skill matched for: {:?}", text);
        }
        RouteOutcome::Empty => {
            println!("Empty transcription");
        }
    }
    Ok(())
}
