use vmrp_runtime::{Runtime, RuntimeEvent, RuntimeStepResult};

#[test]
fn runtime_queue_push_and_pop_round_trip() {
    let mut runtime = Runtime::new();
    assert_eq!(runtime.pop_event(), None);

    runtime.push_event(RuntimeEvent::Bootstrap);

    assert_eq!(runtime.pop_event(), Some(RuntimeEvent::Bootstrap));
    assert_eq!(runtime.pop_event(), None);
}

#[test]
fn runtime_timer_emits_event_after_deadline() {
    let mut runtime = Runtime::new();

    runtime.start_timer(25);
    runtime.advance_time(24);
    runtime.poll_timers();
    assert_eq!(runtime.pop_event(), None);

    runtime.advance_time(1);
    runtime.poll_timers();
    assert_eq!(runtime.pop_event(), Some(RuntimeEvent::Timer));
    assert_eq!(runtime.pop_event(), None);
}

#[test]
fn runtime_timer_restart_replaces_previous_deadline() {
    let mut runtime = Runtime::new();

    runtime.start_timer(50);
    runtime.advance_time(20);
    runtime.start_timer(40);
    runtime.advance_time(29);
    runtime.poll_timers();
    assert_eq!(runtime.pop_event(), None);

    runtime.advance_time(11);
    runtime.poll_timers();
    assert_eq!(runtime.pop_event(), Some(RuntimeEvent::Timer));
}

#[test]
fn runtime_timer_stop_cancels_pending_timer() {
    let mut runtime = Runtime::new();

    runtime.start_timer(10);
    runtime.stop_timer();
    runtime.advance_time(10);
    runtime.poll_timers();

    assert_eq!(runtime.pop_event(), None);
}

#[test]
fn runtime_stage_runner_counts_steps_until_stop() {
    let mut index = 0usize;
    let script = [
        RuntimeStepResult::GuestStep,
        RuntimeStepResult::HostStep,
        RuntimeStepResult::Stop(String::from("done")),
    ];

    let stage = Runtime::run_stage("demo", 10, || {
        let item = script[index].clone();
        index += 1;
        item
    });

    assert_eq!(stage.label, "demo");
    assert_eq!(stage.executed, 2);
    assert_eq!(stage.stop_reason, "done");
}

#[test]
fn runtime_stage_runner_reports_budget_exhaustion() {
    let stage = Runtime::run_stage("budget", 2, || RuntimeStepResult::GuestStep);

    assert_eq!(stage.label, "budget");
    assert_eq!(stage.executed, 2);
    assert_eq!(stage.stop_reason, "step budget exhausted");
}


#[test]
fn runtime_reports_remaining_time_for_pending_timer() {
    let mut runtime = Runtime::new();

    runtime.start_timer(50);
    assert_eq!(runtime.time_until_next_timer_ms(), Some(50));

    runtime.advance_time(20);
    assert_eq!(runtime.time_until_next_timer_ms(), Some(30));

    runtime.advance_time(30);
    assert_eq!(runtime.time_until_next_timer_ms(), Some(0));
}

#[test]
fn runtime_clears_pending_timer_after_poll() {
    let mut runtime = Runtime::new();

    runtime.start_timer(10);
    runtime.advance_time(10);
    runtime.poll_timers();

    assert_eq!(runtime.time_until_next_timer_ms(), None);
    assert_eq!(runtime.pop_event(), Some(RuntimeEvent::Timer));
}

#[test]
fn runtime_queue_preserves_guest_event_payload() {
    let mut runtime = Runtime::new();

    runtime.push_event(RuntimeEvent::GuestEvent {
        code: 1001,
        p0: 2,
        p1: 3,
    });

    assert_eq!(
        runtime.pop_event(),
        Some(RuntimeEvent::GuestEvent {
            code: 1001,
            p0: 2,
            p1: 3,
        })
    );
}
