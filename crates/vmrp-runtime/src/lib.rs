use std::collections::VecDeque;
use std::time::Instant;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeEvent {
    Bootstrap,
    Timer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeStepResult {
    HostStep,
    GuestStep,
    Stop(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageResult {
    pub label: String,
    pub executed: usize,
    pub stop_reason: String,
}

pub struct Runtime {
    epoch: Instant,
    now_ms: u64,
    timer_deadline_ms: Option<u64>,
    queue: VecDeque<RuntimeEvent>,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
            epoch: Instant::now(),
            now_ms: 0,
            timer_deadline_ms: None,
            queue: VecDeque::new(),
        }
    }
}

impl Runtime {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_event(&mut self, event: RuntimeEvent) {
        self.queue.push_back(event);
    }

    pub fn pop_event(&mut self) -> Option<RuntimeEvent> {
        self.queue.pop_front()
    }

    pub fn start_timer(&mut self, delay_ms: u32) {
        self.timer_deadline_ms = Some(self.now_ms.saturating_add(delay_ms as u64));
    }

    pub fn stop_timer(&mut self) {
        self.timer_deadline_ms = None;
    }

    pub fn advance_time(&mut self, delta_ms: u32) {
        self.now_ms = self.now_ms.saturating_add(delta_ms as u64);
    }

    pub fn sync_wall_clock(&mut self) {
        let elapsed = self.epoch.elapsed().as_millis() as u64;
        if elapsed > self.now_ms {
            self.now_ms = elapsed;
        }
    }

    pub fn poll_timers(&mut self) {
        if let Some(deadline) = self.timer_deadline_ms {
            if self.now_ms >= deadline {
                self.timer_deadline_ms = None;
                self.queue.push_back(RuntimeEvent::Timer);
            }
        }
    }

    pub fn run_stage<F>(label: &str, step_limit: usize, mut step: F) -> StageResult
    where
        F: FnMut() -> RuntimeStepResult,
    {
        let mut executed = 0usize;
        let mut stop_reason = String::from("step budget exhausted");

        for _ in 0..step_limit {
            match step() {
                RuntimeStepResult::HostStep | RuntimeStepResult::GuestStep => {
                    executed += 1;
                }
                RuntimeStepResult::Stop(reason) => {
                    stop_reason = reason;
                    break;
                }
            }
        }

        StageResult {
            label: label.to_string(),
            executed,
            stop_reason,
        }
    }
}




