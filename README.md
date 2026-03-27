# vmrp-rust

`vmrp-rust` is a Rust rewrite of the VMRP runtime, focused on running legacy `.mrp` packages.

## Current Status

- `asm.mrp` bootstrap path is executable.
- Startup chain is wired: `mr_c_function_load -> extHelper(code=0) -> DSM_INIT -> MR_START_DSM`.
- `vmrp-windows` now exits with clear process codes:
  - `0`: bootstrap run succeeded
  - non-zero: bootstrap run failed

Latest verified command:

```powershell
cargo run -p vmrp-windows --manifest-path D:\opt\rust\vmrp-rust\Cargo.toml -- D:\opt\rust\vmrp\mrc\asm\asm.mrp
```

Expected key output:

```text
mrp_bootstrap_run_ok=true
```

## Build And Test

```powershell
cargo test --workspace --manifest-path D:\opt\rust\vmrp-rust\Cargo.toml
```

## Runner Usage

```powershell
cargo run -p vmrp-windows --manifest-path D:\opt\rust\vmrp-rust\Cargo.toml -- [path-to.mrp]
```

Optional flags:

- `--verbose` / `-v`: print execution traces
- `--step-limit N`: max steps per stage
- `--trace-limit N`: max trace lines per stage

## Compatibility Notes

Current implementation is still a staged compatibility build:

- Core bootstrap and event entry are working for tested sample.
- `DSM_REQUIRE_FUNCS` has a practical baseline host mapping (file/time/memory/log and minimal stubs).
- Full compatibility with all historical `.mrp` titles is **not finished yet**.

## Next Targets

1. Expand real-world `.mrp` regression set.
2. Complete high-frequency DSM APIs (network/audio/ui paths).
3. Add fuller runtime event loop integration.

## Reference

This implementation is developed with reference to:

- https://github.com/vmrp/vmrp
