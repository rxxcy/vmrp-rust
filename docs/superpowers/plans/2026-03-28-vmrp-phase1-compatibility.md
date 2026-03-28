# VMRP Phase 1 Compatibility Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn `vmrp-rust` from a bootstrap-only runner into a compatibility-first runtime that can execute more historical `.mrp` packages through a real event loop and timer-backed host integration.

**Architecture:** Introduce a dedicated `vmrp-runtime` crate to own emulator lifecycle, event queue, timer scheduling, and guest re-entry. Keep `vmrp-platform` focused on DSM host behavior and make `vmrp-windows` a thin adapter that runs the runtime and later hosts the desktop shell.

**Tech Stack:** Rust workspace crates, existing `vmrp-cpu`/`vmrp-abi`/`vmrp-platform`, Windows host adapter, cargo tests.

---

## File Structure

- Create: `D:\opt\rust\vmrp-rust\crates\vmrp-runtime\Cargo.toml`
- Create: `D:\opt\rust\vmrp-rust\crates\vmrp-runtime\src\lib.rs`
- Create: `D:\opt\rust\vmrp-rust\crates\vmrp-runtime\tests\runtime_loop_tests.rs`
- Modify: `D:\opt\rust\vmrp-rust\Cargo.toml`
- Modify: `D:\opt\rust\vmrp-rust\crates\vmrp-platform\src\lib.rs`
- Modify: `D:\opt\rust\vmrp-rust\crates\vmrp-platform\tests\dsm_host_tests.rs`
- Modify: `D:\opt\rust\vmrp-rust\apps\vmrp-windows\src\main.rs`
- Modify: `D:\opt\rust\vmrp-rust\apps\vmrp-windows\tests\runner_version_test.rs`
- Modify: `D:\opt\rust\vmrp-rust\README.md`
- Modify: `D:\opt\rust\vmrp-rust\README.zh-CN.md`

## Task 1: Scaffold Runtime Crate

**Files:**
- Create: `D:\opt\rust\vmrp-rust\crates\vmrp-runtime\Cargo.toml`
- Create: `D:\opt\rust\vmrp-rust\crates\vmrp-runtime\src\lib.rs`
- Modify: `D:\opt\rust\vmrp-rust\Cargo.toml`

- [ ] **Step 1: Write the failing crate wiring test**

Add a smoke test in `D:\opt\rust\vmrp-rust\crates\vmrp-runtime\tests\runtime_loop_tests.rs` that constructs a minimal runtime type and expects it to expose a no-op event queue API.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p vmrp-runtime --manifest-path D:\opt\rust\vmrp-rust\Cargo.toml`
Expected: FAIL because `vmrp-runtime` crate and types do not exist yet.

- [ ] **Step 3: Write minimal implementation**

Create the crate, add it to the workspace, and implement:
- a minimal `RuntimeEvent` enum
- a `RuntimeState`/`Runtime` struct
- queue push/pop helpers

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p vmrp-runtime --manifest-path D:\opt\rust\vmrp-rust\Cargo.toml`
Expected: PASS

- [ ] **Step 5: Commit**

Run:
`git -C D:\opt\rust\vmrp-rust add Cargo.toml crates/vmrp-runtime`
`git -C D:\opt\rust\vmrp-rust commit -m "feat: add vmrp runtime scaffold"`

## Task 2: Add Runtime Timer Queue

**Files:**
- Modify: `D:\opt\rust\vmrp-rust\crates\vmrp-runtime\src\lib.rs`
- Test: `D:\opt\rust\vmrp-rust\crates\vmrp-runtime\tests\runtime_loop_tests.rs`

- [ ] **Step 1: Write the failing timer test**

Add a test that:
- starts a timer with delay `N`
- advances/polls runtime time
- expects exactly one queued timer event
- restarts the timer and expects replacement semantics
- stops the timer and expects cancellation

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p vmrp-runtime runtime_timer --manifest-path D:\opt\rust\vmrp-rust\Cargo.toml`
Expected: FAIL because timer scheduling is not implemented.

- [ ] **Step 3: Write minimal implementation**

Implement:
- runtime monotonic clock tracking
- one-shot timer record
- start/replace/stop timer API
- poll/drain method that converts expired timers into queued runtime events

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p vmrp-runtime runtime_timer --manifest-path D:\opt\rust\vmrp-rust\Cargo.toml`
Expected: PASS

- [ ] **Step 5: Commit**

Run:
`git -C D:\opt\rust\vmrp-rust add crates/vmrp-runtime`
`git -C D:\opt\rust\vmrp-rust commit -m "feat: add runtime timer scheduling"`

## Task 3: Connect Platform Timer Host Calls To Runtime

**Files:**
- Modify: `D:\opt\rust\vmrp-rust\crates\vmrp-platform\src\lib.rs`
- Test: `D:\opt\rust\vmrp-rust\crates\vmrp-platform\tests\dsm_host_tests.rs`

- [ ] **Step 1: Write the failing platform timer tests**

Add tests that verify:
- `TimerStart` records a timer request instead of only returning success
- `TimerStop` cancels the recorded timer
- repeated `TimerStart` replaces the previous timer

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p vmrp-platform timer --test dsm_host_tests --manifest-path D:\opt\rust\vmrp-rust\Cargo.toml`
Expected: FAIL because platform has no timer-backed runtime integration.

- [ ] **Step 3: Write minimal implementation**

Refactor `ExtHost` so timer-related DSM callbacks mutate runtime-facing timer state instead of acting as stubs. Keep the interface generic enough for later UI/input events.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p vmrp-platform timer --test dsm_host_tests --manifest-path D:\opt\rust\vmrp-rust\Cargo.toml`
Expected: PASS

- [ ] **Step 5: Commit**

Run:
`git -C D:\opt\rust\vmrp-rust add crates/vmrp-platform/src/lib.rs crates/vmrp-platform/tests/dsm_host_tests.rs`
`git -C D:\opt\rust\vmrp-rust commit -m "feat: back DSM timers with runtime state"`

## Task 4: Move Runner Loop Into Runtime

**Files:**
- Modify: `D:\opt\rust\vmrp-rust\crates\vmrp-runtime\src\lib.rs`
- Modify: `D:\opt\rust\vmrp-rust\apps\vmrp-windows\src\main.rs`
- Test: `D:\opt\rust\vmrp-rust\crates\vmrp-runtime\tests\runtime_loop_tests.rs`

- [ ] **Step 1: Write the failing runtime loop test**

Add a test that seeds:
- bootstrap event
- helper init
- DSM init
- start event

and expects the runtime loop to:
- process queued events in order
- stop cleanly on guest exit or null PC
- report structured stage results

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p vmrp-runtime runtime_loop --manifest-path D:\opt\rust\vmrp-rust\Cargo.toml`
Expected: FAIL because event-driven orchestration does not exist yet.

- [ ] **Step 3: Write minimal implementation**

Move execution policy out of `vmrp-windows` into `vmrp-runtime`:
- queue initial events
- poll timers before stepping
- call `ExtHost`
- step CPU
- surface stop reasons and stage report data

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p vmrp-runtime runtime_loop --manifest-path D:\opt\rust\vmrp-rust\Cargo.toml`
Expected: PASS

- [ ] **Step 5: Commit**

Run:
`git -C D:\opt\rust\vmrp-rust add crates/vmrp-runtime apps/vmrp-windows/src/main.rs`
`git -C D:\opt\rust\vmrp-rust commit -m "refactor: move guest loop into vmrp runtime"`

## Task 5: Rewire Windows App To The Runtime

**Files:**
- Modify: `D:\opt\rust\vmrp-rust\apps\vmrp-windows\src\main.rs`
- Test: `D:\opt\rust\vmrp-rust\apps\vmrp-windows\tests\runner_version_test.rs`

- [ ] **Step 1: Write the failing runner regression**

Extend the integration test so it asserts the Windows app uses the new runtime entrypoint and still boots both verified samples.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p vmrp-windows --test runner_version_test --manifest-path D:\opt\rust\vmrp-rust\Cargo.toml`
Expected: FAIL while the app still depends on old loop structure.

- [ ] **Step 3: Write minimal implementation**

Make `vmrp-windows` a host adapter:
- load package
- create runtime
- execute runtime
- print report

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p vmrp-windows --test runner_version_test --manifest-path D:\opt\rust\vmrp-rust\Cargo.toml`
Expected: PASS

- [ ] **Step 5: Commit**

Run:
`git -C D:\opt\rust\vmrp-rust add apps/vmrp-windows/src/main.rs apps/vmrp-windows/tests/runner_version_test.rs`
`git -C D:\opt\rust\vmrp-rust commit -m "refactor: make vmrp-windows use runtime core"`

## Task 6: Add Compatibility Regressions Around Timer And Event Behavior

**Files:**
- Modify: `D:\opt\rust\vmrp-rust\crates\vmrp-platform\tests\dsm_host_tests.rs`
- Modify: `D:\opt\rust\vmrp-rust\crates\vmrp-runtime\tests\runtime_loop_tests.rs`
- Modify: `D:\opt\rust\vmrp-rust\apps\vmrp-windows\tests\runner_version_test.rs`

- [ ] **Step 1: Write the failing regressions**

Add tests for:
- timer restart/cancel paths
- `sleep` and uptime interaction
- host callback ordering after queued timer expiry

- [ ] **Step 2: Run test to verify it fails**

Run:
`cargo test -p vmrp-runtime --manifest-path D:\opt\rust\vmrp-rust\Cargo.toml`
`cargo test -p vmrp-platform --test dsm_host_tests --manifest-path D:\opt\rust\vmrp-rust\Cargo.toml`
Expected: at least one FAIL before implementation is complete.

- [ ] **Step 3: Write minimal implementation**

Adjust runtime/platform behavior only enough to satisfy the regressions.

- [ ] **Step 4: Run test to verify it passes**

Run:
`cargo test -p vmrp-runtime --manifest-path D:\opt\rust\vmrp-rust\Cargo.toml`
`cargo test -p vmrp-platform --test dsm_host_tests --manifest-path D:\opt\rust\vmrp-rust\Cargo.toml`
Expected: PASS

- [ ] **Step 5: Commit**

Run:
`git -C D:\opt\rust\vmrp-rust add crates/vmrp-runtime/tests crates/vmrp-platform/tests apps/vmrp-windows/tests/runner_version_test.rs`
`git -C D:\opt\rust\vmrp-rust commit -m "test: add runtime compatibility regressions"`

## Task 7: Update Docs For Phase 1 Runtime Architecture

**Files:**
- Modify: `D:\opt\rust\vmrp-rust\README.md`
- Modify: `D:\opt\rust\vmrp-rust\README.zh-CN.md`

- [ ] **Step 1: Write the failing documentation checklist**

Add a short checklist to the plan implementation notes covering:
- runtime crate exists
- compatibility-first phase is documented
- timer/event loop is described as the current priority

- [ ] **Step 2: Run verification against the current docs**

Run:
`rg -n "vmrp-runtime|timer|event loop|兼容优先|事件循环" D:\opt\rust\vmrp-rust\README* -S`
Expected: missing or incomplete coverage before edits.

- [ ] **Step 3: Write minimal implementation**

Update both readmes to describe:
- runtime split
- current compatibility phase
- later desktop shell phase

- [ ] **Step 4: Run verification to confirm the docs**

Run:
`rg -n "vmrp-runtime|timer|event loop|兼容优先|事件循环" D:\opt\rust\vmrp-rust\README* -S`
Expected: matches found in both docs.

- [ ] **Step 5: Commit**

Run:
`git -C D:\opt\rust\vmrp-rust add README.md README.zh-CN.md`
`git -C D:\opt\rust\vmrp-rust commit -m "docs: document phase1 runtime architecture"`

## Final Verification

- [ ] Run: `cargo test -p vmrp-runtime --manifest-path D:\opt\rust\vmrp-rust\Cargo.toml`
- [ ] Run: `cargo test -p vmrp-platform --test dsm_host_tests --manifest-path D:\opt\rust\vmrp-rust\Cargo.toml`
- [ ] Run: `cargo test -p vmrp-windows --test runner_version_test --manifest-path D:\opt\rust\vmrp-rust\Cargo.toml`
- [ ] Run: `cargo run -p vmrp-windows --manifest-path D:\opt\rust\vmrp-rust\Cargo.toml -- D:\opt\rust\vmrp\mrc\asm\asm.mrp`
- [ ] Run: `cargo run -p vmrp-windows --manifest-path D:\opt\rust\vmrp-rust\Cargo.toml -- D:\opt\rust\vmrp\mrc\asm\asm_thumb.mrp`

## Notes

- Do not modify files outside `D:\opt\rust\vmrp-rust`.
- Prefer TDD for every runtime/platform change.
- Prefer small commits after each completed task.
- Phase 2 desktop shell work starts only after this plan’s runtime goals are stable.
