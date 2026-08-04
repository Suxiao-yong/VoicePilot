//! W10 Plan 4 — Kill Switch 1s SLA 断言测试(spec §6.4 + §9.4 ⑨)。
//!
//! 5 个可中断路径:
//! 1. Idle → Cancelled(直跳,≤ 100ms,无 Cancelling 中间态)
//! 2. Listening → Cancelling → Cancelled(≤ 1s)
//! 3. Planning → Cancelling → Cancelled(≤ 1s)
//! 4. AwaitingApproval → Cancelling → Cancelled(≤ 1s)
//! 5. Executing → Cancelling → Cancelled(≤ 1s,模拟 voice loop chunk 边界)
//!
//! 不测 Verifying/Compensating/LLM/Playwright 中断(不可中断路径,SLA 可能 > 1s,
//! 在 task_cancelled audit event 中记录 sla_met: false,不阻塞)。

use std::time::Instant;
use chrono::Utc;
use trust_kernel::kernel::TrustKernel;
use trust_kernel::state::TaskState;
use uuid::Uuid;

/// 辅助:创建 task 并 transition 到目标状态。
fn setup_task_at_state(kernel: &TrustKernel, target: TaskState) -> String {
    let task_id = Uuid::new_v4().to_string();
    kernel.create_task(&task_id, "kill switch SLA test").unwrap();
    // 按合法路径 transition 到目标态
    use TaskState::*;
    match target {
        Idle => {} // create_task 默认 Idle
        Listening => kernel.transition(&task_id, Listening).unwrap(),
        Planning => {
            kernel.transition(&task_id, Listening).unwrap();
            kernel.transition(&task_id, Planning).unwrap();
        }
        AwaitingApproval => {
            kernel.transition(&task_id, Listening).unwrap();
            kernel.transition(&task_id, Planning).unwrap();
            kernel.transition(&task_id, AwaitingApproval).unwrap();
        }
        Executing => {
            kernel.transition(&task_id, Listening).unwrap();
            kernel.transition(&task_id, Planning).unwrap();
            kernel.transition(&task_id, AwaitingApproval).unwrap();
            kernel.transition(&task_id, Executing).unwrap();
        }
        _ => panic!("unsupported target state for SLA test: {:?}", target),
    }
    task_id
}

/// 辅助:断言 task 最终状态为 Cancelled。
fn assert_task_cancelled(kernel: &TrustKernel, task_id: &str) {
    let task = kernel.get_task(task_id).unwrap().unwrap();
    assert_eq!(
        task.status,
        TaskState::Cancelled,
        "task must be Cancelled after kill switch"
    );
}

/// 辅助:断言 audit logs 中包含指定 event_type。
fn assert_audit_has_event(kernel: &TrustKernel, task_id: &str, event_type: &str) {
    let events = kernel.list_audit_for_task(task_id).unwrap();
    let found = events.iter().any(|e| e.event_type == event_type);
    assert!(
        found,
        "audit logs must contain event_type '{}'",
        event_type
    );
}

#[test]
fn kill_switch_sla_met_from_idle() {
    // Idle → Cancelled 直跳(无 Cancelling 中间态),≤ 100ms
    // spec §8.1 测试 3:Idle 无 ongoing work,直接取消
    let kernel = TrustKernel::open_in_memory().unwrap();
    let task_id = setup_task_at_state(&kernel, TaskState::Idle);

    let t0 = Instant::now();
    let triggered_at_ms = kernel.trigger_kill_switch(&task_id, "test").unwrap();
    // Idle 特殊处理:trigger_kill_switch 内部直接 → Cancelled
    // complete_cancellation 检测到已 Cancelled,跳过 transition,仅 emit task_cancelled
    kernel
        .complete_cancellation(&task_id, triggered_at_ms, Utc::now().timestamp_millis())
        .unwrap();
    let elapsed = t0.elapsed();

    assert_task_cancelled(&kernel, &task_id);
    assert!(elapsed.as_millis() <= 100, "Idle kill switch must be ≤ 100ms, got {}ms", elapsed.as_millis());
    assert_audit_has_event(&kernel, &task_id, "kill_switch_triggered");
    assert_audit_has_event(&kernel, &task_id, "task_cancelled");
}

#[test]
fn kill_switch_sla_met_from_listening() {
    // Listening → Cancelling → Cancelled,≤ 1s
    let kernel = TrustKernel::open_in_memory().unwrap();
    let task_id = setup_task_at_state(&kernel, TaskState::Listening);

    let t0 = Instant::now();
    let triggered_at_ms = kernel.trigger_kill_switch(&task_id, "test").unwrap();
    kernel
        .complete_cancellation(&task_id, triggered_at_ms, Utc::now().timestamp_millis())
        .unwrap();
    let elapsed = t0.elapsed();

    assert_task_cancelled(&kernel, &task_id);
    assert!(elapsed.as_millis() <= 1000, "Listening kill switch must be ≤ 1s, got {}ms", elapsed.as_millis());
    assert_audit_has_event(&kernel, &task_id, "kill_switch_triggered");
    assert_audit_has_event(&kernel, &task_id, "task_cancelled");
}

#[test]
fn kill_switch_sla_met_from_planning() {
    // Planning → Cancelling → Cancelled,≤ 1s
    let kernel = TrustKernel::open_in_memory().unwrap();
    let task_id = setup_task_at_state(&kernel, TaskState::Planning);

    let t0 = Instant::now();
    let triggered_at_ms = kernel.trigger_kill_switch(&task_id, "test").unwrap();
    kernel
        .complete_cancellation(&task_id, triggered_at_ms, Utc::now().timestamp_millis())
        .unwrap();
    let elapsed = t0.elapsed();

    assert_task_cancelled(&kernel, &task_id);
    assert!(elapsed.as_millis() <= 1000, "Planning kill switch must be ≤ 1s, got {}ms", elapsed.as_millis());
}

#[test]
fn kill_switch_sla_met_from_awaiting_approval() {
    // AwaitingApproval → Cancelling → Cancelled,≤ 1s
    let kernel = TrustKernel::open_in_memory().unwrap();
    let task_id = setup_task_at_state(&kernel, TaskState::AwaitingApproval);

    let t0 = Instant::now();
    let triggered_at_ms = kernel.trigger_kill_switch(&task_id, "test").unwrap();
    kernel
        .complete_cancellation(&task_id, triggered_at_ms, Utc::now().timestamp_millis())
        .unwrap();
    let elapsed = t0.elapsed();

    assert_task_cancelled(&kernel, &task_id);
    assert!(elapsed.as_millis() <= 1000, "AwaitingApproval kill switch must be ≤ 1s, got {}ms", elapsed.as_millis());
}

#[test]
fn kill_switch_sla_met_from_executing() {
    // Executing → Cancelling → Cancelled,≤ 1s(模拟 voice loop chunk 边界)
    // spec §6.2 v2 修订 #8:Executing voice loop chunk 边界可中断,≤ 500ms
    let kernel = TrustKernel::open_in_memory().unwrap();
    let task_id = setup_task_at_state(&kernel, TaskState::Executing);

    let t0 = Instant::now();
    let triggered_at_ms = kernel.trigger_kill_switch(&task_id, "test").unwrap();
    kernel
        .complete_cancellation(&task_id, triggered_at_ms, Utc::now().timestamp_millis())
        .unwrap();
    let elapsed = t0.elapsed();

    assert_task_cancelled(&kernel, &task_id);
    assert!(elapsed.as_millis() <= 1000, "Executing kill switch must be ≤ 1s, got {}ms", elapsed.as_millis());
}

#[test]
fn kill_switch_from_executing_audits_three_events() {
    // spec §6.4:断言 3 个 audit 事件类型按序触发(kill_switch_triggered /
    // state_transition / task_cancelled)。实际产生 4 事件(2 个 state_transition),
    // 但 3 类型按序出现。本测试验证顺序:kill_switch_triggered 先于 state_transition(to Cancelling)
    // 先于 task_cancelled。
    let kernel = TrustKernel::open_in_memory().unwrap();
    let task_id = setup_task_at_state(&kernel, TaskState::Executing);

    let triggered_at_ms = kernel.trigger_kill_switch(&task_id, "test").unwrap();
    kernel
        .complete_cancellation(&task_id, triggered_at_ms, Utc::now().timestamp_millis())
        .unwrap();

    let events = kernel.list_audit_for_task(&task_id).unwrap();

    // 找到 3 个关键事件的索引
    let mut kill_switch_idx = None;
    let mut state_transition_to_cancelling_idx = None;
    let mut task_cancelled_idx = None;
    for (i, e) in events.iter().enumerate() {
        if e.event_type == "kill_switch_triggered" && kill_switch_idx.is_none() {
            kill_switch_idx = Some(i);
        }
        if e.event_type == "state_transition" {
            // 找 details.to == "CANCELLING" 的 state_transition
            // TaskState 用 SCREAMING_SNAKE_CASE 序列化,Cancelling → "CANCELLING"
            let to = e.details.get("to").and_then(|v| v.as_str());
            if to == Some("CANCELLING") && state_transition_to_cancelling_idx.is_none() {
                state_transition_to_cancelling_idx = Some(i);
            }
        }
        if e.event_type == "task_cancelled" && task_cancelled_idx.is_none() {
            task_cancelled_idx = Some(i);
        }
    }

    // 断言 3 个事件都存在
    let ks = kill_switch_idx.expect("kill_switch_triggered event must exist");
    let st = state_transition_to_cancelling_idx.expect("state_transition(to Cancelling) event must exist");
    let tc = task_cancelled_idx.expect("task_cancelled event must exist");

    // 断言顺序:kill_switch_triggered < state_transition(to Cancelling) < task_cancelled
    assert!(
        ks < st,
        "kill_switch_triggered (idx {}) must come before state_transition to Cancelling (idx {})",
        ks,
        st
    );
    assert!(
        st < tc,
        "state_transition to Cancelling (idx {}) must come before task_cancelled (idx {})",
        st,
        tc
    );

    // 额外断言:task_cancelled 的 sla_met = true(Executing 可中断,SLA 应满足)
    let tc_event = &events[tc];
    let sla_met = tc_event.details.get("sla_met").and_then(|v| v.as_bool());
    assert_eq!(
        sla_met,
        Some(true),
        "sla_met must be true for interruptible Executing path"
    );
}
