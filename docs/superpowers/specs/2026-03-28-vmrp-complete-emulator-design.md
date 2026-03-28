# VMRP Complete Emulator Design

**Date:** 2026-03-28

**Scope:** Build `vmrp-rust` from the current bootstrap runner into a fuller Mythroad-compatible emulator in two ordered phases:

1. Phase 1: prioritize compatibility with more historical `.mrp` packages.
2. Phase 2: add a desktop simulator shell with window, framebuffer output, and input.

## Goals

- Keep all work inside `D:\opt\rust\vmrp-rust`.
- Preserve the existing Rust-first rewrite direction.
- Keep old `.mrp` compatibility as the primary success metric.
- Avoid coupling emulator core logic to the Windows app shell.

## Non-Goals

- Full compatibility with every historical `.mrp` in one pass.
- Audio, networking, and every platform API in the first milestone.
- Building the desktop shell before the runtime core is stable.

## Current State

- `vmrp-cpu` executes enough ARM/Thumb code to bootstrap `asm.mrp` and `asm_thumb.mrp`.
- `vmrp-abi` can load `.mrp`, `start.mr`, and `cfunction.ext`.
- `vmrp-platform` contains a growing DSM host shim with basic file, time, random, memory, and callback support.
- `vmrp-windows` is still a bootstrap-oriented runner, not a long-lived event-driven emulator host.

## Architecture

The project should settle into five layers with clear responsibility boundaries:

1. `vmrp-cpu`
   - ARM/Thumb execution only.
   - No file, timer, UI, or OS policy.

2. `vmrp-abi`
   - Parse `.mrp` container data and runtime payloads.
   - Convert package assets into guest-loadable blobs.

3. `vmrp-platform`
   - Implement Mythroad/DSM host-facing APIs.
   - Expose deterministic, testable host operations: files, time, RNG, timers, bitmap paths, events.

4. `vmrp-runtime`
   - New runtime orchestration layer.
   - Own emulator state, event queue, timer scheduling, lifecycle, and execution loop policy.
   - Bridge guest code execution with host callbacks and queued platform events.

5. `vmrp-windows`
   - Windows entrypoint and adapter.
   - In phase 1, run headless or minimal shell mode.
   - In phase 2, own window creation, framebuffer presentation, and input collection.

## Phase 1: Compatibility-First Runtime

### Objectives

- Turn the current bootstrap runner into a reusable runtime loop.
- Replace stub-like timer behavior with actual scheduled runtime events.
- Expand DSM host coverage around APIs that old `.mrp` files are likely to hit early.
- Make compatibility measurable with repeatable regression coverage.

### Required Runtime Capabilities

- Central runtime state object for guest CPU, memory, host platform state, lifecycle flags, and loaded package metadata.
- Event queue for DSM initialization, start, timer callbacks, and future host-originated events.
- Timer scheduler that can arm, replace, cancel, and inject events into the queue.
- Structured stop reasons so failures are classified as:
  - CPU decode/execute gap
  - host API gap
  - guest-requested exit
  - event loop exhaustion
  - runtime invariant failure

### Required Platform Capabilities

- Keep strengthening DSM host behavior with tests first.
- File system compatibility remains important, but next priority is:
  - timer start/stop
  - uptime and sleep consistency
  - event entry behavior
  - basic drawing surface contracts
- Host implementations should remain testable without a window.

### Phase 1 Success Criteria

- Existing verified samples still pass.
- Runtime is event-driven instead of single bootstrap-only staging.
- `TimerStart` and `TimerStop` are real runtime operations, not success stubs.
- Adding new `.mrp` regression samples becomes a data-and-test task, not a structural rewrite.

## Phase 2: Desktop Simulator Shell

### Objectives

- Present guest graphics in a native window.
- Accept keyboard input and translate it into Mythroad-compatible events.
- Keep window code outside the runtime core.

### Desktop Shell Requirements

- Create a window and present a software framebuffer.
- Add a runtime-to-shell surface interface so `vmrp-platform` can request drawing without depending on window APIs.
- Translate host keyboard input into runtime events that feed the guest event queue.
- Maintain headless testability for the runtime and platform layers.

### Phase 2 Success Criteria

- A `.mrp` can render visible output in a desktop window.
- Basic key input reaches guest code through the runtime event system.
- The same core runtime can be exercised in tests without the window.

## Data Flow

1. `vmrp-windows` loads the package through `vmrp-abi`.
2. `vmrp-runtime` creates guest memory, bootstraps helper code, and seeds the initial DSM events.
3. Guest execution enters the runtime loop.
4. Guest host calls route into `vmrp-platform`.
5. `vmrp-platform` may mutate host state directly or enqueue runtime work such as timers and window-facing updates.
6. `vmrp-runtime` drains queued events and re-enters guest code as needed.
7. In phase 2, `vmrp-windows` polls window/input state and feeds translated events back into `vmrp-runtime`.

## Error Handling

- Keep host callback failures explicit and typed.
- Preserve stop reasons in final reports and tests.
- Separate unsupported API failures from CPU instruction gaps.
- Prefer minimal but correct behavior over fake success stubs when guest-visible behavior matters.

## Testing Strategy

- Use TDD for every platform/runtime behavior change.
- Keep unit tests near `vmrp-platform` and future `vmrp-runtime`.
- Keep end-to-end sample tests in `vmrp-windows`.
- Add compatibility regressions by package/sample instead of only by isolated helper behavior.

## Risks

- Timer/event behavior is the biggest current structural gap.
- UI shell work can easily pollute the runtime if not isolated.
- Some compatibility bugs will still come from unimplemented CPU instructions, but host/runtime gaps are now the higher-yield area.

## Immediate Next Step

Create a phase-1 implementation plan focused on:

- introducing `vmrp-runtime`
- wiring real event loop and timer scheduling
- adapting `vmrp-platform` to runtime-backed timers/events
- keeping `vmrp-windows` as the host adapter and sample runner
