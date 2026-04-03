# VMRP Rust Phase 1 Sample-Driven Compatibility Design

**Date:** 2026-04-03

**Scope:** Define the first implementation phase for `vmrp-rust` after the current bootstrap and early compatibility work. This phase prioritizes running more real `.mrp` samples reliably on Windows by following the behavior of the C-based `vmrp` project, while keeping new host logic reusable for a later wasm/web frontend.

## Goals

- Keep all work inside `D:\opt\rust`.
- Use the existing `vmrp` repository as the behavioral reference.
- Prioritize "more real samples run stably" over perfect semantic fidelity.
- Preserve and extend the current Rust workspace structure instead of collapsing logic into `vmrp-windows`.
- Keep future wasm/web reuse possible by pushing host semantics into shared layers.
- Avoid touching the currently dirty `vmrp-cpu` files called out in `docs/SESSION-HANDOFF-2026-04-02.md`.

## Non-Goals

- Declaring `vmrp-rust` a fully complete emulator in one phase.
- Reaching perfect compatibility with every historical `.mrp`.
- Implementing the wasm/web frontend in this phase.
- Performing broad refactors unrelated to sample-driven compatibility gains.
- Reworking `vmrp-cpu` while the protected CPU files are dirty.

## Hard Constraints

- Only files under `D:\opt\rust` may be modified.
- Communication remains in Chinese.
- The following CPU-related files are out of scope for edits in this phase:
  - `D:\opt\rust\vmrp-rust\crates\vmrp-cpu\src\decode\mod.rs`
  - `D:\opt\rust\vmrp-rust\crates\vmrp-cpu\src\decode\thumb.rs`
  - `D:\opt\rust\vmrp-rust\crates\vmrp-cpu\src\execute.rs`
  - `D:\opt\rust\vmrp-rust\crates\vmrp-cpu\tests\arm_data_processing_tests.rs`
  - `D:\opt\rust\vmrp-rust\crates\vmrp-cpu\tests\thumb_tests.rs`

## Current State

- `vmrp-windows` already boots and runs multiple real samples to the current runner-supported stage.
- The verified sample set currently includes:
  - `D:\opt\rust\vmrp\wasm\dist\fs\mythroad\dsm_gm.mrp`
  - `D:\opt\rust\vmrp\wasm\dist\fs\mythroad\mpc.mrp`
  - `D:\opt\rust\vmrp\wasm\dist\fs\mythroad\ydqtwo.mrp`
  - `D:\opt\rust\vmrp\wasm\dist\fs\mythroad\plugins\netpay.mrp`
  - `D:\opt\rust\vmrp\wasm\dist\fs\mythroad\plugins\ose\brwcore.mrp`
- `vmrp-abi`, `vmrp-runtime`, `vmrp-platform`, and `vmrp-windows` have all gained compatibility work since the original bootstrap stage.
- Recent progress is concentrated in:
  - MRP/ext loading and decoding
  - host UI snapshot and interaction plumbing
  - `mr_platEx` compatibility branches
  - runtime guest callback scheduling
  - Windows presenter refresh and input interception

## Primary Success Metric

The primary metric for this phase is not abstract API coverage. It is the ability to run more real `.mrp` packages more reliably, with regressions caught by both automated tests and sample execution checks.

That means each implementation cycle should answer:

1. Which real sample exposed a compatibility gap?
2. What guest-visible behavior was missing or wrong?
3. What minimal shared-layer change closes that gap?
4. Which tests and sample runs prove the improvement?

## Sample Regression Matrix

Phase 1 uses a mixed regression matrix instead of focusing on one app class:

- Game / ordinary package samples
  - `ydqtwo.mrp`
  - `dsm_gm.mrp`
  - `mpc.mrp`
- Plugin / browser-oriented samples
  - `netpay.mrp`
  - `brwcore.mrp`

The rule for prioritization is:

- Keep all currently verified samples running.
- Whichever sample exposes the next concrete gap becomes the next compatibility target.
- Prefer changes that improve multiple samples at once, but do not block on finding a perfect abstraction first.

## Architecture Boundaries

### `vmrp-platform`

This is the main implementation surface for phase 1.

Responsibilities:

- Mythroad / DSM host behavior
- `mr_platEx` compatibility branches
- file and directory semantics
- timer commands and host callback behavior
- host UI state and metadata
- media state machines and compatibility stubs with real observable behavior
- network and socket behavior where it is independent of the Windows window layer

Rules:

- Any guest-visible behavior that is not inherently tied to Win32 windows should go here first.
- New compatibility branches should be testable without a real native window.
- Stub replacement work should be driven by real sample needs, not by checklist completion alone.

### `vmrp-runtime`

Responsibilities:

- event queue ownership
- timer scheduling
- guest callback scheduling
- runtime orchestration that is not presenter-specific

Rules:

- Only move behavior here when it is truly runtime policy.
- Do not turn phase 1 into a broad architecture rewrite.
- Prefer small extractions that reduce accidental coupling between runner logic and shared runtime behavior.

### `vmrp-windows`

Responsibilities:

- process entrypoint
- sample startup wiring
- presenter / real window
- Win32 input collection and translation
- integration glue between runtime and presenter

Rules:

- Do not continue growing Windows-only files with general host compatibility logic unless the behavior is genuinely window-specific.
- If a bug is exposed through the Windows runner but the missing behavior is generic host logic, the fix should land in `vmrp-platform` or `vmrp-runtime`.

### `vmrp-abi`

Responsibilities in this phase:

- package decoding fixes only when required by real samples
- resource loading, decompression, and encoding fixes directly tied to compatibility gaps

Rules:

- Do not expand ABI scope speculatively.
- Fix only what concrete samples prove is necessary.

## Implementation Strategy

Phase 1 follows a sample-driven loop:

1. Freeze a real-sample regression set.
2. Run the samples and inspect the first failing or obviously degraded behavior.
3. Identify the smallest guest-visible semantic gap.
4. Write a failing automated test for that behavior.
5. Implement the minimal fix in the shared layer when possible.
6. Re-run the targeted tests.
7. Re-run the relevant real samples.
8. Keep the sample in the regression set.

This approach is intentionally biased toward shipping a more usable emulator sooner, even if some implementations remain simplified or partial.

## Compatibility Priorities

The next areas of work should be chosen from the following pool, in the order exposed by real samples:

1. `mr_platEx` branches that still behave like weak stubs or fake success
2. file and directory behavior that affects package startup or app flow
3. network async and callback behavior
4. media state transitions and observable playback semantics
5. host UI interaction loops beyond the currently working menu/dialog/edit baseline
6. runner/runtime glue that still blocks long-running or interactive sample behavior

The priority rule is practical:

- if a missing branch blocks a sample from progressing, it wins
- if several gaps are visible, prefer the one that is easiest to verify and likely to help multiple samples

## Error Handling

- Unsupported behavior should remain explicit in verbose mode where it helps tracing.
- Replace pure success stubs only when guest-visible semantics matter or real samples depend on them.
- Prefer minimal correct behavior over large speculative implementations.
- Keep failures classifiable enough that future work can tell whether the gap is in:
  - host behavior
  - runtime orchestration
  - package loading
  - protected CPU execution paths

## Testing Strategy

Every behavior change in phase 1 should follow TDD:

- write a failing targeted test first
- confirm the failure is caused by the missing behavior
- implement the smallest fix
- re-run the targeted tests

Test placement rules:

- `crates/vmrp-platform/tests`
  - host semantics
  - `mr_platEx`
  - files, dirs, network, media, UI state
- `crates/vmrp-runtime/tests`
  - event queue, timers, callback orchestration
- `apps/vmrp-windows/tests`
  - runner-level integration and sample-specific regression behavior

Real sample verification is mandatory in addition to unit or integration tests for changes that affect actual package execution.

## Phase 1 Acceptance Criteria

Phase 1 is considered successful when all of the following are true:

- `cargo test --workspace` remains green.
- The currently known working sample set continues to run.
- New compatibility work results in observable sample-level improvement, not only isolated unit tests.
- New guest-visible host semantics are added mostly to shared layers, not accumulated in Windows-only glue.
- The codebase is better prepared for a later wasm/web frontend because core host behavior remains outside the Windows presenter layer.

## Risks

- Sample-driven work can tempt ad hoc fixes unless boundaries are enforced.
- `vmrp-windows` already contains a lot of orchestration logic, so some fixes may try to land there for convenience.
- Network and media behaviors can expand quickly if scope is not kept tied to sample evidence.
- Protected CPU files mean some future gaps may need to be deferred if they turn out to be decode or execute issues rather than host issues.

## Immediate Next Step

Write an implementation plan for this phase that:

- names the sample regression matrix explicitly
- sequences compatibility work around real-sample failures
- keeps new host semantics in `vmrp-platform` and `vmrp-runtime` when possible
- includes concrete verification commands for tests and sample runs
