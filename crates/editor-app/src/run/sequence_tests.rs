//! The order of preparation, its blocking rules, and the races a launch can lose.
use super::*;
use crate::run::{PreparedStep, RunPlan, RunSession, StepKind};

fn step(kind: StepKind, name: &str) -> PreparedStep {
    PreparedStep {
        kind,
        name: name.to_owned(),
        config: "run-1".to_owned(),
        request: plugin_runtime::RunRequest {
            program: "tool.exe".into(),
            args: Vec::new(),
            cwd: None,
            name: Some(name.to_owned()),
            env: Vec::new(),
        },
    }
}

fn plan() -> RunPlan {
    RunPlan {
        steps: vec![
            step(StepKind::Build, "构建"),
            step(StepKind::Prelaunch, "生成"),
            step(StepKind::Program, "程序"),
        ],
    }
}

fn sessions_with(
    state: plugin_runtime::ExecutionState,
) -> std::collections::BTreeMap<u64, RunSession> {
    std::collections::BTreeMap::from([(
        7,
        RunSession {
            id: 7,
            config: "run-1".into(),
            plugin: "terminal".into(),
            state,
            provider_session: Some("1".into()),
            failure: None,
        },
    )])
}

#[test]
fn a_sequence_starts_each_step_only_after_the_previous_one_succeeded() {
    let mut sequence = RunSequence::new("run-1", &plan());
    // Nothing starts before the first step is requested, so the order is the plan's order.
    assert_eq!(
        sequence.next_action(|_| false, |_| false),
        SequenceAction::Start { index: 0 }
    );
    sequence.started(0, 7, Some("1".into()));
    assert_eq!(
        sequence.next_action(|_| true, |_| true),
        SequenceAction::Wait
    );
    assert!(sequence.observe(0, StepOutcome::Exited { code: 0 }));
    assert_eq!(
        sequence.next_action(|_| false, |_| false),
        SequenceAction::Start { index: 1 }
    );
    sequence.started(1, 8, Some("2".into()));
    assert!(sequence.observe(1, StepOutcome::Exited { code: 0 }));
    assert_eq!(
        sequence.next_action(|_| false, |_| false),
        SequenceAction::Start { index: 2 }
    );
    sequence.started(2, 9, Some("3".into()));
    // The program is the last step: once it is running the sequence is done preparing.
    assert_eq!(
        sequence.next_action(|_| true, |_| true),
        SequenceAction::Wait
    );
    assert!(
        sequence.is_active(),
        "a running program keeps the launch owned"
    );
    assert_eq!(
        sequence.current_step().map(|step| step.kind),
        Some(StepKind::Program)
    );
}

#[test]
fn a_failing_step_blocks_every_later_step_and_keeps_the_first_cause() {
    let mut sequence = RunSequence::new("run-1", &plan());
    sequence.started(0, 7, Some("1".into()));
    assert!(sequence.observe(0, StepOutcome::Exited { code: 3 }));
    let SequenceAction::Blocked { reason } = sequence.next_action(|_| false, |_| false) else {
        panic!("a failed step blocks the sequence");
    };
    // The reason names the step, so a user knows which action to fix.
    assert!(reason.contains("构建") && reason.contains('3'), "{reason}");
    assert!(!sequence.is_active());
    // A late answer for an earlier step cannot restart a blocked sequence.
    assert!(!sequence.observe(0, StepOutcome::Exited { code: 0 }));
    let SequenceAction::Blocked { reason: again } = sequence.next_action(|_| false, |_| false)
    else {
        panic!("the block is stable");
    };
    assert_eq!(again, reason, "the first cause is the reported one");
}

#[test]
fn a_step_with_no_confirmable_end_blocks_instead_of_passing() {
    let mut sequence = RunSequence::new("run-1", &plan());
    sequence.started(0, 7, Some("1".into()));
    // A provider that retired cannot be asked what happened, and an unknown result is not success.
    assert!(sequence.observe(0, StepOutcome::Unknown));
    let SequenceAction::Blocked { reason } = sequence.next_action(|_| false, |_| false) else {
        panic!("an unconfirmed step blocks the sequence");
    };
    assert!(reason.contains("无法确认"), "{reason}");
    assert_eq!(
        sequence.next_action(|_| false, |_| false),
        SequenceAction::Blocked { reason }
    );
}

#[test]
fn stopping_during_preparation_blocks_the_launch_and_the_stop_is_addressed_to_its_session() {
    let mut sequence = RunSequence::new("run-1", &plan());
    sequence.started(0, 7, Some("1".into()));
    assert!(!sequence.is_stopping());
    sequence.request_stop();
    assert!(sequence.is_stopping());
    // The stop is addressed to the session this step owns, not to a name or a handle.
    assert_eq!(
        sequence.next_action(|_| true, |_| true),
        SequenceAction::Stop { session: 7 }
    );
    // The sequence still owns work while the stop is outstanding, so leaving cannot happen yet.
    assert!(sequence.is_active());
    // A program that ends successfully while the stop is on its way does not advance the sequence.
    assert!(!sequence.observe(0, StepOutcome::Exited { code: 0 }));
    sequence.stopped(7);
    let SequenceAction::Blocked { reason } = sequence.next_action(|_| false, |_| false) else {
        panic!("a stopped preparation blocks the launch");
    };
    assert!(reason.contains("已停止"), "{reason}");
    assert!(!sequence.is_active());
    assert_eq!(
        sequence.current_step().map(|step| step.name.as_str()),
        Some("构建")
    );
}

#[test]
fn a_stop_requested_while_a_step_is_only_queued_still_blocks_the_program() {
    let mut sequence = RunSequence::new("run-1", &plan());
    sequence.started(0, 7, Some("1".into()));
    sequence.observe(0, StepOutcome::Exited { code: 0 });
    // The second step was requested but the runtime has not answered yet.
    assert_eq!(
        sequence.next_action(|_| false, |_| false),
        SequenceAction::Start { index: 1 }
    );
    sequence.request_stop();
    // A queued step is abandoned without waiting: the launch is already decided.
    assert!(sequence.blocked_by().is_some());
    assert!(!sequence.observe(1, StepOutcome::Exited { code: 0 }));
}

#[test]
fn a_session_that_ends_without_a_result_blocks_the_sequence() {
    let mut sequence = RunSequence::new("run-1", &plan());
    sequence.started(0, 7, Some("1".into()));
    // The runtime reports the session as finished while the provider has not answered: the step
    // cannot be called successful, so the launch stops instead of continuing on an assumption.
    let SequenceAction::Blocked { reason } = sequence.next_action(|_| true, |_| false) else {
        panic!("a finished session without a result blocks the sequence");
    };
    assert!(reason.contains("未报告结果"), "{reason}");
}

#[test]
fn a_running_program_keeps_its_launch_owned_by_the_sequence() {
    let mut sequence = RunSequence::new("run-1", &plan());
    sequence.started(0, 7, Some("1".into()));
    sequence.observe(0, StepOutcome::Exited { code: 0 });
    sequence.started(1, 8, Some("2".into()));
    sequence.observe(1, StepOutcome::Exited { code: 0 });
    assert_eq!(
        sequence.next_action(|_| false, |_| false),
        SequenceAction::Start { index: 2 }
    );
    sequence.started(2, 9, Some("3".into()));
    // The program is the last step and it is running, so the launch is still owned: a repeated Run
    // click must locate this session rather than start a second program.
    assert!(sequence.is_active());
    assert_eq!(sequence.current_session(), Some(9));
    // A blocked or finished sequence owns nothing.
    sequence.request_stop();
    sequence.stopped(9);
    assert!(!sequence.is_active());
}

#[test]
fn a_build_only_sequence_never_reaches_a_program() {
    let build = vec![step(StepKind::Build, "构建"), step(StepKind::Build, "测试")];
    let mut sequence = RunSequence::build_only("run-1", &build);
    assert!(!sequence.launches_program);
    sequence.started(0, 7, Some("1".into()));
    sequence.observe(0, StepOutcome::Exited { code: 0 });
    sequence.started(1, 8, Some("2".into()));
    sequence.observe(1, StepOutcome::Exited { code: 0 });
    // The sequence ends rather than starting the configuration's program.
    assert_eq!(
        sequence.next_action(|_| false, |_| false),
        SequenceAction::Done
    );
    assert!(!sequence.is_active());
    assert!(sequence.blocked_by().is_none());
    assert_eq!(
        sequence.steps().last().map(|step| step.kind),
        Some(StepKind::Build)
    );
}

#[test]
fn a_terminated_step_blocks_even_though_nobody_asked_to_stop() {
    let mut sequence = RunSequence::new("run-1", &plan());
    sequence.started(0, 7, Some("1".into()));
    assert!(sequence.observe(0, StepOutcome::Terminated));
    let SequenceAction::Blocked { reason } = sequence.next_action(|_| false, |_| false) else {
        panic!("a terminated preparation step blocks the launch");
    };
    assert!(reason.contains("已终止"), "{reason}");
}
