# C reference comparison workflow

Use the existing C project at `D:\opt\rust\vmrp` as a behavior reference.

## Manual comparison loop

1. Reproduce a small guest snippet in both projects.
2. Run the Rust CPU one step at a time and capture `StepTrace`.
3. Run the C version with matching initial register and memory state.
4. Compare:
   - current PC
   - execution mode
   - opcode
   - register writes
   - final register state
5. If the traces diverge, reduce to the smallest opcode sequence that still fails.

## Scope

This directory is documentation only for now. Automated differential tooling will be added later.
