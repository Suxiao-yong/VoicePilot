use anyhow::{anyhow, Result};
use std::io::{self, Write};
use trust_kernel::kernel::TrustKernel;
use trust_kernel::state::TaskState;
use uuid::Uuid;

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
