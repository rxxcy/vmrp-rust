mod presenter;

use presenter::{DirtyRect, WindowPresenter};
use std::collections::VecDeque;
use std::path::Path;
use std::thread;
use std::time::Duration;

use vmrp_abi::{ExtFile, MrChunk, MrpDecodeError, MrpFile, MrpRuntimeAssets};
use vmrp_core::{GuestAddr, DEFAULT_LAYOUT};
use vmrp_cpu::{Cpu, ExecutionMode, MemoryBus, StepTrace, TestMemory};
use vmrp_platform::{
    ExtBootstrap, ExtHost, HostScreenRegion, HostTimerCommand, FLAG_USE_UTF8_EDIT,
    MR_FAILED, SEND_APP_EVENT_ADDR, VMRP_VER,
};
use vmrp_runtime::{Runtime, RuntimeEvent, RuntimeStepResult, StageResult};

const DEFAULT_MRP_PATH: &str = r"D:\opt\rust\vmrp\mrc\asm\asm.mrp";
const START_MR_ADDR: u32 = 0x190000;
const RUNTIME_DATA_SIZE: u32 = 0x400;
const RUNTIME_START_T_OFFSET: u32 = 0x100;
const RUNTIME_HELPER_EVENT_OFFSET: u32 = 0x120;
const RUNTIME_GUEST_EVENT_OFFSET: u32 = 0x140;
const RUNTIME_DSM_REQUIRE_FUNCS_OFFSET: u32 = 0x200;
const DSM_INIT_CODE: i32 = -100;
const MR_START_DSM_CODE: i32 = -99;
const MR_TIMER_CODE: i32 = -96;
const MR_EVENT_CODE: i32 = -95;
const HELPER_EVENT_SIZE: u32 = 12;
const LEGACY_MR_VERSION: u32 = 1968;
const HELPER_APP_INFO_OFFSET: u32 = 0x300;
const HELPER_APP_INFO_SID_NAME_OFFSET: u32 = 0x340;
const HELPER_APP_INFO_SIZE: u32 = 16;
const LEGACY_EXT_HANDLE_SIZE: u32 = 0x30;
const LEGACY_EXT_TABLE_SIZE: u32 = 0x248;
const LEGACY_TIMER_STRUCT_SIZE: u32 = 0x20;
const MR_C_FUNCTION_CONTEXT_EXT_CHUNK_OFFSET: u32 = 0x0C;
const MR_C_FUNCTION_CONTEXT_STACK_OFFSET: u32 = 0x10;
const MR_C_INTERNAL_TABLE_SLOT_OFFSET: u32 = 0x5C;
const LEGACY_EXT_CHUNK_CHECK: u32 = 0x7FD854EB;
const LEGACY_TIMER_CHECK: u32 = 0x79AB_BCCF;
const HELPER_RW_MR_MALLOC_SLOT_OFFSET: u32 = 0x10C;
const HELPER_RW_MR_FREE_SLOT_OFFSET: u32 = 0x110;
const HELPER_RW_INTERNAL_TABLE_OFFSET: u32 = 0x168;
const WINDOW_TITLE: &str = "vmrp-rust";
const SCREEN_WIDTH: usize = 240;
const SCREEN_HEIGHT: usize = 320;
const RECENT_TRACE_CAPACITY: usize = 4096;
const LOG_TRIGGER_TRACE_DUMP_LIMIT: usize = 2048;
const FINAL_TRACE_DUMP_LIMIT: usize = 128;

#[derive(Debug, Clone)]
struct RunnerConfig {
    mrp_path: String,
    window: bool,
    verbose: bool,
    step_limit: usize,
    trace_limit: usize,
}

#[derive(Debug)]
struct RunReport {
    mrp_path: String,
    app_name: String,
    internal_name: String,
    helper_addr: Option<u32>,
    dsm_init_ret: i32,
    start_dsm_ret: i32,
    stages: Vec<StageResult>,
    run_ok: bool,
    exit_requested: bool,
}

enum ParseOutcome {
    Help,
    Config(RunnerConfig),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HelperBootstrapPayload {
    version_code: u32,
    app_info_addr: u32,
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SeededExtChunkDescriptor {
    ext_chunk_addr: u32,
    ext_table_addr: u32,
    timer_addr: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HelperRwWatchChange {
    offset: u32,
    old: u32,
    new: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HelperRwWatchSample {
    rw_base: Option<u32>,
    base_reset: bool,
    changes: Vec<HelperRwWatchChange>,
}

#[derive(Debug, Clone)]
struct HelperRwWatch {
    c_function_p: GuestAddr,
    offsets: Vec<u32>,
    rw_base: Option<u32>,
    values: Vec<u32>,
}

impl HelperRwWatch {
    fn new<I>(c_function_p: GuestAddr, offsets: I) -> Self
    where
        I: IntoIterator<Item = u32>,
    {
        let mut offsets = offsets.into_iter().collect::<Vec<_>>();
        offsets.sort_unstable();
        offsets.dedup();
        let values = vec![0; offsets.len()];
        Self {
            c_function_p,
            offsets,
            rw_base: None,
            values,
        }
    }

    fn sample<B: MemoryBus>(
        &mut self,
        memory: &B,
    ) -> Result<HelperRwWatchSample, vmrp_cpu::MemoryAccessError> {
        let rw_base = memory.read32(self.c_function_p)?;
        if rw_base == 0 {
            self.rw_base = None;
            self.values.fill(0);
            return Ok(HelperRwWatchSample {
                rw_base: None,
                base_reset: false,
                changes: Vec::new(),
            });
        }

        let current_base = Some(rw_base);
        if self.rw_base != current_base {
            self.rw_base = current_base;
            for (index, offset) in self.offsets.iter().copied().enumerate() {
                self.values[index] = memory.read32(GuestAddr::new(rw_base.wrapping_add(offset)))?;
            }
            return Ok(HelperRwWatchSample {
                rw_base: current_base,
                base_reset: true,
                changes: Vec::new(),
            });
        }

        let mut changes = Vec::new();
        for (index, offset) in self.offsets.iter().copied().enumerate() {
            let value = memory.read32(GuestAddr::new(rw_base.wrapping_add(offset)))?;
            let old = self.values[index];
            if value != old {
                changes.push(HelperRwWatchChange {
                    offset,
                    old,
                    new: value,
                });
                self.values[index] = value;
            }
        }

        Ok(HelperRwWatchSample {
            rw_base: current_base,
            base_reset: false,
            changes,
        })
    }
}

fn main() {
    let parse = match parse_args() {
        Ok(v) => v,
        Err(err) => {
            eprintln!("{err}");
            eprintln!("\n{}", usage());
            std::process::exit(2);
        }
    };

    let config = match parse {
        ParseOutcome::Help => {
            println!("{}", usage());
            return;
        }
        ParseOutcome::Config(v) => v,
    };

    let report = match run_mrp(&config) {
        Ok(report) => report,
        Err(err) => {
            eprintln!("run_error={err}");
            std::process::exit(1);
        }
    };

    println!("vmrp-windows runner");
    println!("mrp_path={}", report.mrp_path);
    println!("internal_name={}", report.internal_name);
    println!("app_name={}", report.app_name);
    if let Some(helper) = report.helper_addr {
        println!("ext_helper_addr=0x{helper:X}");
    } else {
        println!("ext_helper_addr=<none>");
    }
    let dsm_version_compatible = report.dsm_init_ret == VMRP_VER || report.dsm_init_ret == 0;
    println!(
        "dsm_init_ret={} expected={} compatible={}",
        report.dsm_init_ret, VMRP_VER, dsm_version_compatible
    );
    println!("mr_start_dsm_ret={}", report.start_dsm_ret);
    println!("guest_exit_requested={}", report.exit_requested);

    if config.verbose {
        for stage in &report.stages {
            println!(
                "stage={} steps={} stop_reason={}",
                stage.label, stage.executed, stage.stop_reason
            );
        }
    } else if !report.run_ok {
        for stage in &report.stages {
            println!("stage={} stop_reason={}", stage.label, stage.stop_reason);
        }
    }

    println!("mrp_bootstrap_run_ok={}", report.run_ok);
    std::process::exit(if report.run_ok { 0 } else { 1 });
}

fn usage() -> &'static str {
    "Usage: vmrp-windows [--window] [--verbose|-v] [--step-limit N] [--trace-limit N] [<path-to.mrp>]"
}

fn parse_args() -> Result<ParseOutcome, String> {
    parse_args_from(std::env::args().skip(1))
}

fn parse_args_from<I, S>(args: I) -> Result<ParseOutcome, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut mrp_path: Option<String> = None;
    let mut window = false;
    let mut verbose = false;
    let mut step_limit = 4000usize;
    let mut trace_limit = 200usize;

    let args = args
        .into_iter()
        .map(|value| value.as_ref().to_string())
        .collect::<Vec<_>>();
    let mut index = 0usize;

    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--help" | "-h" => return Ok(ParseOutcome::Help),
            "--window" => {
                window = true;
                index += 1;
            }
            "--verbose" | "-v" => {
                verbose = true;
                index += 1;
            }
            "--step-limit" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| String::from("--step-limit requires a value"))?;
                step_limit = value
                    .parse::<usize>()
                    .map_err(|_| String::from("--step-limit must be an integer"))?;
                index += 2;
            }
            "--trace-limit" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| String::from("--trace-limit requires a value"))?;
                trace_limit = value
                    .parse::<usize>()
                    .map_err(|_| String::from("--trace-limit must be an integer"))?;
                index += 2;
            }
            _ if arg.starts_with('-') => {
                return Err(format!("unknown option: {arg}"));
            }
            _ => {
                if mrp_path.is_some() {
                    return Err(String::from("multiple .mrp paths provided"));
                }
                mrp_path = Some(arg.clone());
                index += 1;
            }
        }
    }

    Ok(ParseOutcome::Config(RunnerConfig {
        mrp_path: mrp_path.unwrap_or_else(|| String::from(DEFAULT_MRP_PATH)),
        window,
        verbose,
        step_limit,
        trace_limit,
    }))
}

fn helper_ext_search_paths(mrp_path: &str) -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    let path = Path::new(mrp_path);
    for ancestor in path.ancestors().skip(1) {
        paths.push(ancestor.join("cfunction.ext"));
    }
    paths
}

fn guest_ram_size() -> u32 {
    DEFAULT_LAYOUT
        .memory_manager_address()
        .get()
        .wrapping_add(DEFAULT_LAYOUT.memory_manager_size())
        .wrapping_sub(DEFAULT_LAYOUT.code_address().get())
}

fn default_stack_top() -> u32 {
    DEFAULT_LAYOUT.stack_address().get() + DEFAULT_LAYOUT.stack_size()
}

fn load_runtime_assets(mrp: &MrpFile, mrp_path: &str) -> Result<MrpRuntimeAssets, String> {
    match mrp.runtime_assets() {
        Ok(assets) => Ok(assets),
        Err(MrpDecodeError::NotFound) => {
            for candidate in helper_ext_search_paths(mrp_path) {
                if !candidate.is_file() {
                    continue;
                }
                let ext = ExtFile::from_path(&candidate).map_err(|err| {
                    format!(
                        "load fallback helper failed at {}: {err:?}",
                        candidate.display()
                    )
                })?;
                return mrp.runtime_assets_with_ext(ext).map_err(|err| {
                    format!("build runtime assets with fallback helper failed: {err:?}")
                });
            }
            Err(String::from("NotFound"))
        }
        Err(err) => Err(format!("{err:?}")),
    }
}

fn run_mrp(config: &RunnerConfig) -> Result<RunReport, String> {
    let mrp =
        MrpFile::from_path(&config.mrp_path).map_err(|err| format!("read mrp failed: {err:?}"))?;
    let assets = load_runtime_assets(&mrp, &config.mrp_path)
        .map_err(|err| format!("decode runtime assets failed: {err}"))?;
    let ext = assets.cfunction_ext().clone();
    let start_mr = assets.start_mr();

    if config.verbose {
        println!("real_mrp_header={}", String::from_utf8_lossy(mrp.magic()));
        println!("real_mrp_internal_name={}", mrp.internal_name());
        println!("real_mrp_app_name={}", mrp.app_name());
        for (index, entry) in mrp.entries().iter().enumerate() {
            println!(
                "real_mrp_entry[{index}]={} offset=0x{:X} len={}",
                entry.name(),
                entry.offset(),
                entry.len()
            );
        }
        if start_mr.len() >= 4 {
            println!(
                "real_mrp_start_mr_head={:02X}{:02X}{:02X}{:02X}",
                start_mr[0], start_mr[1], start_mr[2], start_mr[3]
            );
        }
        match MrChunk::from_bytes(start_mr) {
            Ok(chunk) => {
                println!("start_mr_chunk_version=0x{:02X}", chunk.header().version());
                println!("start_mr_main_code_count={}", chunk.main().code_count());
            }
            Err(err) => {
                println!("start_mr_chunk_parse_error={err:?}");
            }
        }
    }

    let blob = ext.to_code_blob(DEFAULT_LAYOUT.code_address().get());
    let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), guest_ram_size());

    for (offset, byte) in blob.bytes().iter().enumerate() {
        let addr = GuestAddr::new(blob.load_address().get().wrapping_add(offset as u32));
        memory
            .write8(addr, *byte)
            .map_err(|err| format!("write ext byte failed: {err:?}"))?;
    }

    for (offset, byte) in start_mr.iter().enumerate() {
        let addr = GuestAddr::new(START_MR_ADDR.wrapping_add(offset as u32));
        memory
            .write8(addr, *byte)
            .map_err(|err| format!("write start.mr byte failed: {err:?}"))?;
    }

    memory
        .write32(
            GuestAddr::new(DEFAULT_LAYOUT.code_address().get().wrapping_add(0x1130)),
            0x181140,
        )
        .map_err(|err| format!("seed ext literal failed: {err:?}"))?;

    let bootstrap = ExtBootstrap {
        mr_table_addr: GuestAddr::new(0x180000),
        mr_c_function_new_addr: GuestAddr::new(0x181000),
        mr_c_function_p_addr: GuestAddr::new(0x182000),
        mr_malloc_addr: GuestAddr::new(0x181100),
        mr_free_addr: GuestAddr::new(0x181140),
        mr_realloc_addr: GuestAddr::new(0x181180),
        mr_malloc_result_addr: GuestAddr::new(0x183000),
        memcpy_addr: GuestAddr::new(0x181200),
        memset_addr: GuestAddr::new(0x181300),
    };
    bootstrap
        .apply(&mut memory, DEFAULT_LAYOUT.code_address())
        .map_err(|err| format!("seed ext bootstrap failed: {err:?}"))?;

    let mut host = ExtHost::new(
        GuestAddr::new(0x181000),
        GuestAddr::new(0x182000),
        GuestAddr::new(0x181100),
        GuestAddr::new(0x181140),
        GuestAddr::new(0x181180),
        GuestAddr::new(0x181200),
        GuestAddr::new(0x181300),
        GuestAddr::new(0x183000),
        0x30000,
    );
    host.set_verbose(config.verbose);

    if let Some(parent) = Path::new(&config.mrp_path).parent() {
        host.set_working_dir(parent.to_path_buf());
    }
    for entry in mrp.entries() {
        if let Ok(bytes) = mrp.file_bytes_inflated(entry.name()) {
            host.register_package_file(entry.name().to_string(), bytes);
        }
    }

    let mut cpu = Cpu::new(memory);
    cpu.regs_mut().set_pc(blob.entry().get());
    cpu.regs_mut().set_execution_mode(blob.mode());
    cpu.regs_mut().set_sp(default_stack_top());

    let ext_entry_code = std::env::var("VMRP_EXT_CODE")
        .ok()
        .and_then(|raw| raw.parse::<u32>().ok())
        .unwrap_or(0);
    cpu.regs_mut().set_reg(0, ext_entry_code);

    let mut runtime = Runtime::new();
    let mut stages = Vec::new();
    let mut helper_rw_watch = None;
    let mut presenter = if config.window {
        Some(
            WindowPresenter::new(WINDOW_TITLE, SCREEN_WIDTH, SCREEN_HEIGHT)
                .map_err(|err| format!("create window presenter failed: {err}"))?,
        )
    } else {
        None
    };

    let stage_ext_init = run_loop(
        &mut runtime,
        &mut cpu,
        &mut host,
        config.step_limit,
        config.trace_limit,
        "ext_init",
        config.verbose,
        &mut helper_rw_watch,
        &mut presenter,
    );
    stages.push(stage_ext_init);

    let helper = host.ext_helper_addr();
    let helper_addr = helper.map(|v| v.get());

    let mut dsm_init_ret = MR_FAILED;
    let mut start_dsm_ret = MR_FAILED;

    if let Some(helper) = helper {
        helper_rw_watch = helper_rw_watch_from_env(host.c_function_p_addr());
        seed_ext_chunk_descriptor(
            cpu.memory_mut(),
            &mut host,
            bootstrap.mr_table_addr,
            blob.entry().get(),
            blob.bytes().len() as u32 + 8,
            helper.get(),
        )
        .map_err(|err| format!("seed ext chunk descriptor failed: {err}"))?;

        let runtime_data_base = host
            .alloc_guest_block(cpu.memory_mut(), RUNTIME_DATA_SIZE)
            .map_err(|err| format!("alloc runtime payload block failed: {err:?}"))?
            .ok_or_else(|| String::from("alloc runtime payload block returned null"))?
            .get();

        let helper_bootstrap =
            prepare_helper_bootstrap_payload(cpu.memory_mut(), runtime_data_base, &mrp)
                .map_err(|err| format!("prepare helper bootstrap payload failed: {err:?}"))?;

        for (code, event_addr, input_len) in
            helper_bootstrap_sequence_for_mrp(&mrp, blob.load_address().get(), helper_bootstrap)
        {
            let label = match code {
                6 => "helper_version",
                8 => "helper_app_info",
                0 => "helper_init",
                _ => "helper",
            };
            setup_helper_call(
                &mut cpu,
                helper,
                host.c_function_p_addr(),
                code,
                event_addr,
                input_len,
            );
            let stage = run_loop(
                &mut runtime,
                &mut cpu,
                &mut host,
                config.step_limit,
                config.trace_limit,
                label,
                config.verbose,
                &mut helper_rw_watch,
                &mut presenter,
            );
            stages.push(stage);
            if config.verbose {
                dump_helper_state(cpu.memory(), host.c_function_p_addr(), label);
            }
        }
        if should_skip_helper_bootstrap(&mrp) {
            apply_bridge_compatible_helper_state(cpu.memory_mut(), host.c_function_p_addr())
                .map_err(|err| format!("apply bridge-compatible helper state failed: {err:?}"))?;
        }
        apply_helper_rw_debug_overrides(cpu.memory_mut(), host.c_function_p_addr(), config.verbose)
            .map_err(|err| format!("apply helper rw debug overrides failed: {err:?}"))?;
        seed_helper_runtime_compat_slots(
            cpu.memory_mut(),
            host.c_function_p_addr(),
            bootstrap.mr_table_addr,
            bootstrap.mr_malloc_addr,
            bootstrap.mr_free_addr,
        )
        .map_err(|err| format!("seed helper runtime compat slots failed: {err:?}"))?;

        let dsm_require_funcs_addr = runtime_data_base + RUNTIME_DSM_REQUIRE_FUNCS_OFFSET;
        host.install_dsm_require_funcs(
            cpu.memory_mut(),
            GuestAddr::new(dsm_require_funcs_addr),
            FLAG_USE_UTF8_EDIT,
        )
        .map_err(|err| format!("install DSM_REQUIRE_FUNCS failed: {err:?}"))?;

        let dsm_init_event_addr = write_event(
            cpu.memory_mut(),
            runtime_data_base + RUNTIME_HELPER_EVENT_OFFSET,
            DSM_INIT_CODE,
            dsm_require_funcs_addr,
            0,
        )
        .map_err(|err| format!("write DSM_INIT event failed: {err:?}"))?;
        setup_helper_call(
            &mut cpu,
            helper,
            host.c_function_p_addr(),
            1,
            dsm_init_event_addr,
            HELPER_EVENT_SIZE,
        );
        let stage_dsm_init = run_loop(
            &mut runtime,
            &mut cpu,
            &mut host,
            config.step_limit,
            config.trace_limit,
            "dsm_init",
            config.verbose,
            &mut helper_rw_watch,
            &mut presenter,
        );
        dsm_init_ret = cpu.regs().reg(0) as i32;
        stages.push(stage_dsm_init);
        if config.verbose {
            dump_helper_state(cpu.memory(), host.c_function_p_addr(), "dsm_init");
        }

        let start_t_addr = prepare_start_dsm_payload(
            cpu.memory_mut(),
            runtime_data_base,
            &config.mrp_path,
        )
        .map_err(|err| format!("prepare MR_START_DSM payload failed: {err:?}"))?;
        let start_event_addr = write_event(
            cpu.memory_mut(),
            runtime_data_base + RUNTIME_HELPER_EVENT_OFFSET,
            MR_START_DSM_CODE,
            start_t_addr,
            0,
        )
        .map_err(|err| format!("write MR_START_DSM event failed: {err:?}"))?;
        setup_helper_call(
            &mut cpu,
            helper,
            host.c_function_p_addr(),
            1,
            start_event_addr,
            HELPER_EVENT_SIZE,
        );
        let stage_start_dsm = run_loop(
            &mut runtime,
            &mut cpu,
            &mut host,
            config.step_limit,
            config.trace_limit,
            "start_dsm",
            config.verbose,
            &mut helper_rw_watch,
            &mut presenter,
        );
        start_dsm_ret = cpu.regs().reg(0) as i32;
        stages.push(stage_start_dsm);
        if config.verbose {
            dump_helper_state(cpu.memory(), host.c_function_p_addr(), "start_dsm");
        }

        drive_runtime_events(
            &mut runtime,
            &mut cpu,
            &mut host,
            helper,
            runtime_data_base,
            config,
            &mut helper_rw_watch,
            &mut presenter,
            &mut stages,
        )?;
    }

    let no_runtime_faults = stages.iter().all(|stage| {
        !stage.stop_reason.contains("UnimplementedInstruction")
            && is_successful_stage_stop_reason(&stage.stop_reason)
    });
    let has_helper = helper_addr.is_some();
    let version_ok = dsm_init_ret == VMRP_VER || dsm_init_ret == 0;
    let start_ok = start_dsm_ret != MR_FAILED;

    let run_ok = no_runtime_faults && has_helper && version_ok && start_ok;

    Ok(RunReport {
        mrp_path: config.mrp_path.clone(),
        app_name: mrp.app_name().to_string(),
        internal_name: mrp.internal_name().to_string(),
        helper_addr,
        dsm_init_ret,
        start_dsm_ret,
        stages,
        run_ok,
        exit_requested: host.exit_requested(),
    })
}

fn push_recent_trace_line(recent: &mut VecDeque<String>, capacity: usize, line: String) {
    if capacity == 0 {
        return;
    }
    if recent.len() == capacity {
        recent.pop_front();
    }
    recent.push_back(line);
}

fn format_step_trace_line(label: &str, observed: usize, trace: &StepTrace) -> String {
    let writes = if trace.register_writes.is_empty() {
        String::from("writes=[]")
    } else {
        let rendered = trace
            .register_writes
            .iter()
            .map(|write| format!("r{}=0x{:08X}", write.index, write.value))
            .collect::<Vec<_>>()
            .join(",");
        format!("writes=[{rendered}]")
    };
    format!(
        "{label}_step[{observed}] mode={:?} pc=0x{:08X} op=0x{:08X} {writes}",
        trace.mode, trace.pc, trace.opcode
    )
}

fn should_dump_recent_trace_for_guest_log(msg: &str) -> bool {
    [
        "invalid compressed data",
        "unzip err",
        "cannot read start.mr",
    ]
    .iter()
    .any(|needle| msg.contains(needle))
}

fn is_successful_stage_stop_reason(reason: &str) -> bool {
    matches!(reason, "program returned to null pc" | "guest requested exit")
}

fn dump_recent_trace(label: &str, recent: &VecDeque<String>, limit: usize) {
    println!("stage={label} recent_trace_begin");
    let start = recent.len().saturating_sub(limit);
    for line in recent.iter().skip(start) {
        println!("{line}");
    }
    println!("stage={label} recent_trace_end");
}

fn parse_watch_u32(value: &str) -> Option<u32> {
    let trimmed = value.trim();
    trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .and_then(|hex| u32::from_str_radix(hex, 16).ok())
        .or_else(|| trimmed.parse::<u32>().ok())
}

fn helper_rw_watch_from_env(c_function_p: GuestAddr) -> Option<HelperRwWatch> {
    let raw = std::env::var("VMRP_HELPER_RW_WATCH").ok()?;
    let offsets = if raw.trim().is_empty() || raw.trim() == "1" {
        vec![
            0x120u32, 0x124, 0x128, 0x12C, 0x130, 0x134, 0x138, 0x13C, 0x140, 0x15C, 0x160,
            0x164, 0x168,
        ]
    } else {
        raw.split(',')
            .filter_map(parse_watch_u32)
            .collect::<Vec<_>>()
    };
    if offsets.is_empty() {
        return None;
    }
    Some(HelperRwWatch::new(c_function_p, offsets))
}

fn log_helper_rw_watch_sample(
    label: &str,
    observed: usize,
    step_kind: &str,
    pc: u32,
    sample: &HelperRwWatchSample,
) {
    if let Some(rw_base) = sample.rw_base {
        if sample.base_reset {
            println!(
                "stage={label} helper_rw_watch_base[{observed}] kind={step_kind} pc=0x{pc:08X} rw_base=0x{rw_base:X}"
            );
        }
        for change in &sample.changes {
            println!(
                "stage={label} helper_rw_watch[{observed}] kind={step_kind} pc=0x{pc:08X} offset=0x{:X} old=0x{:X} new=0x{:X}",
                change.offset, change.old, change.new
            );
        }
    }
}

fn run_loop(
    runtime: &mut Runtime,
    cpu: &mut Cpu<TestMemory>,
    host: &mut ExtHost,
    step_limit: usize,
    trace_limit: usize,
    label: &'static str,
    verbose: bool,
    helper_rw_watch: &mut Option<HelperRwWatch>,
    presenter: &mut Option<WindowPresenter>,
) -> StageResult {
    let mut observed = 0usize;
    let mut recent = VecDeque::with_capacity(RECENT_TRACE_CAPACITY);

    let stage = Runtime::run_stage(label, step_limit, || {
        if let Err(err) = drain_host_ui(runtime, host, presenter) {
            return RuntimeStepResult::Stop(err);
        }

        if let Some(command) = host.take_timer_command() {
            match command {
                HostTimerCommand::Start(delay) => runtime.start_timer(delay),
                HostTimerCommand::Stop => runtime.stop_timer(),
            }
        }

        runtime.sync_wall_clock();
        runtime.poll_timers();
        if host.exit_requested() {
            return RuntimeStepResult::Stop(String::from("guest requested exit"));
        }

        if cpu.regs().pc() == 0 {
            return RuntimeStepResult::Stop(String::from("program returned to null pc"));
        }

        match host.handle(cpu) {
            Ok(true) => {
                observed += 1;
                let pc = cpu.regs().pc();
                let line = format!("{label}_host_step[{observed}] pc=0x{pc:08X}");
                push_recent_trace_line(&mut recent, RECENT_TRACE_CAPACITY, line.clone());
                if verbose && observed <= trace_limit {
                    println!("{line}");
                }
                if let Some(watch) = helper_rw_watch.as_mut() {
                    if let Ok(sample) = watch.sample(cpu.memory()) {
                        log_helper_rw_watch_sample(label, observed, "host", pc, &sample);
                    }
                }
                if let Err(err) = drain_host_ui(runtime, host, presenter) {
                    return RuntimeStepResult::Stop(err);
                }
                if let Some(msg) = host.take_last_log_message() {
                    println!("stage={label} guest_log={msg}");
                    if verbose && should_dump_recent_trace_for_guest_log(&msg) && !recent.is_empty()
                    {
                        println!("stage={label} guest_log_trigger={msg}");
                        dump_recent_trace(label, &recent, LOG_TRIGGER_TRACE_DUMP_LIMIT);
                    }
                }
                return RuntimeStepResult::HostStep;
            }
            Ok(false) => {}
            Err(err) => {
                return RuntimeStepResult::Stop(format!("host callback error: {err:?}"));
            }
        }

        match cpu.step() {
            Ok(step) => {
                observed += 1;
                let line = format_step_trace_line(label, observed, &step.trace);
                push_recent_trace_line(&mut recent, RECENT_TRACE_CAPACITY, line.clone());
                if verbose && observed <= trace_limit {
                    println!("{line}");
                }
                if let Some(watch) = helper_rw_watch.as_mut() {
                    if let Ok(sample) = watch.sample(cpu.memory()) {
                        log_helper_rw_watch_sample(
                            label,
                            observed,
                            "guest",
                            step.trace.pc,
                            &sample,
                        );
                    }
                }
                RuntimeStepResult::GuestStep
            }
            Err(err) => RuntimeStepResult::Stop(format!("{err:?}")),
        }
    });

    if verbose && !recent.is_empty() && stage.stop_reason != "program returned to null pc" {
        dump_recent_trace(label, &recent, FINAL_TRACE_DUMP_LIMIT);
    }

    stage
}

fn drain_host_ui(
    runtime: &mut Runtime,
    host: &mut ExtHost,
    presenter: &mut Option<WindowPresenter>,
) -> Result<(), String> {
    if let Some(region) = host.take_dirty_region() {
        if let Some(presenter) = presenter.as_mut() {
            presenter
                .present(host.screen_buffer(), to_presenter_rect(region))
                .map_err(|err| format!("present frame failed: {err}"))?;
            drain_presenter_events(runtime, presenter);
        }
    } else if let Some(presenter) = presenter.as_mut() {
        presenter.pump();
        drain_presenter_events(runtime, presenter);
    }

    Ok(())
}

fn drain_presenter_events(runtime: &mut Runtime, presenter: &mut WindowPresenter) {
    for event in presenter.take_guest_events() {
        runtime.push_event(RuntimeEvent::GuestEvent {
            code: event.code,
            p0: event.p0,
            p1: event.p1,
        });
    }
}

fn should_keep_waiting_for_host_events(
    presenter_active: bool,
    next_timer_ms: Option<u32>,
    idle_polls: usize,
    step_limit: usize,
) -> bool {
    if next_timer_ms.is_some() {
        idle_polls < step_limit
    } else {
        presenter_active
    }
}

fn to_presenter_rect(region: HostScreenRegion) -> DirtyRect {
    DirtyRect {
        x: region.x,
        y: region.y,
        w: region.w,
        h: region.h,
    }
}

fn drive_runtime_events(
    runtime: &mut Runtime,
    cpu: &mut Cpu<TestMemory>,
    host: &mut ExtHost,
    helper: GuestAddr,
    runtime_data_base: u32,
    config: &RunnerConfig,
    helper_rw_watch: &mut Option<HelperRwWatch>,
    presenter: &mut Option<WindowPresenter>,
    stages: &mut Vec<StageResult>,
) -> Result<(), String> {
    let mut idle_polls = 0usize;

    loop {
        drain_host_ui(runtime, host, presenter)?;
        runtime.sync_wall_clock();
        runtime.poll_timers();

        let mut dispatched = false;
        while let Some(event) = runtime.pop_event() {
            dispatched = true;
            match event {
                RuntimeEvent::Bootstrap => {}
                RuntimeEvent::Timer => {
                    let timer_event_addr = write_event(
                        cpu.memory_mut(),
                        runtime_data_base + RUNTIME_HELPER_EVENT_OFFSET,
                        MR_TIMER_CODE,
                        0,
                        0,
                    )
                    .map_err(|err| format!("write MR_TIMER event failed: {err:?}"))?;
                    setup_helper_call(
                        cpu,
                        helper,
                        host.c_function_p_addr(),
                        1,
                        timer_event_addr,
                        HELPER_EVENT_SIZE,
                    );
                    let stage_timer = run_loop(
                        runtime,
                        cpu,
                        host,
                        config.step_limit,
                        config.trace_limit,
                        "timer",
                        config.verbose,
                        helper_rw_watch,
                        presenter,
                    );
                    stages.push(stage_timer);
                }
                RuntimeEvent::GuestEvent { code, p0, p1 } => {
                    let event_payload_addr = write_event(
                        cpu.memory_mut(),
                        runtime_data_base + RUNTIME_GUEST_EVENT_OFFSET,
                        code,
                        p0,
                        p1,
                    )
                    .map_err(|err| format!("write MR_EVENT payload failed: {err:?}"))?;
                    let event_addr = write_event(
                        cpu.memory_mut(),
                        runtime_data_base + RUNTIME_HELPER_EVENT_OFFSET,
                        MR_EVENT_CODE,
                        event_payload_addr,
                        0,
                    )
                    .map_err(|err| format!("write MR_EVENT event failed: {err:?}"))?;
                    setup_helper_call(
                        cpu,
                        helper,
                        host.c_function_p_addr(),
                        1,
                        event_addr,
                        HELPER_EVENT_SIZE,
                    );
                    let stage_event = run_loop(
                        runtime,
                        cpu,
                        host,
                        config.step_limit,
                        config.trace_limit,
                        "event",
                        config.verbose,
                        helper_rw_watch,
                        presenter,
                    );
                    stages.push(stage_event);
                }
            }
        }

        if dispatched {
            idle_polls = 0;
            continue;
        }

        let next_timer_ms = runtime.time_until_next_timer_ms();
        let presenter_active = presenter
            .as_ref()
            .map(|presenter| presenter.should_stay_open())
            .unwrap_or(false);
        if !should_keep_waiting_for_host_events(
            presenter_active,
            next_timer_ms,
            idle_polls,
            config.step_limit,
        ) {
            break;
        }

        if let Some(presenter) = presenter.as_mut() {
            presenter.pump();
            drain_presenter_events(runtime, presenter);
        }
        let sleep_ms = next_timer_ms.unwrap_or(10).clamp(1, 10) as u64;
        thread::sleep(Duration::from_millis(sleep_ms));
        idle_polls += 1;
    }

    Ok(())
}

fn prepare_helper_bootstrap_payload(
    memory: &mut TestMemory,
    runtime_data_base: u32,
    mrp: &MrpFile,
) -> Result<HelperBootstrapPayload, vmrp_cpu::MemoryAccessError> {
    let helper_app_info_addr = runtime_data_base + HELPER_APP_INFO_OFFSET;
    let mut sid_name_cursor = runtime_data_base + HELPER_APP_INFO_SID_NAME_OFFSET;
    let sid_name_ptr = write_c_string(memory, &mut sid_name_cursor, mrp.internal_name())?;
    write_u32(memory, helper_app_info_addr, mrp.header().appid())?;
    write_u32(memory, helper_app_info_addr + 4, mrp.header().version())?;
    write_u32(memory, helper_app_info_addr + 8, sid_name_ptr)?;
    write_u32(memory, helper_app_info_addr + 12, 0)?;

    Ok(HelperBootstrapPayload {
        version_code: LEGACY_MR_VERSION,
        app_info_addr: helper_app_info_addr,
    })
}

fn alloc_required_guest_block(
    memory: &mut TestMemory,
    host: &mut ExtHost,
    len: u32,
    label: &str,
) -> Result<u32, String> {
    host.alloc_guest_block(memory, len)
        .map_err(|err| format!("alloc {label} failed: {err:?}"))?
        .map(|addr| addr.get())
        .ok_or_else(|| format!("alloc {label} returned null"))
}

fn zero_guest_range(
    memory: &mut TestMemory,
    start: u32,
    len: u32,
) -> Result<(), vmrp_cpu::MemoryAccessError> {
    for offset in 0..len {
        memory.write8(GuestAddr::new(start.wrapping_add(offset)), 0)?;
    }
    Ok(())
}

fn copy_guest_words(
    memory: &mut TestMemory,
    src: u32,
    dst: u32,
    len: u32,
) -> Result<(), vmrp_cpu::MemoryAccessError> {
    for offset in (0..len).step_by(4) {
        let value = memory.read32(GuestAddr::new(src.wrapping_add(offset)))?;
        write_u32(memory, dst.wrapping_add(offset), value)?;
    }
    Ok(())
}

fn seed_ext_chunk_descriptor(
    memory: &mut TestMemory,
    host: &mut ExtHost,
    mr_table_addr: GuestAddr,
    ext_entry_addr: u32,
    ext_code_len: u32,
    helper_addr: u32,
) -> Result<SeededExtChunkDescriptor, String> {
    let c_function_p_addr = host.c_function_p_addr();
    let var_buf = memory
        .read32(GuestAddr::new(c_function_p_addr.get()))
        .map_err(|err| format!("read rw base failed: {err:?}"))?;
    let var_len = memory
        .read32(GuestAddr::new(c_function_p_addr.get() + 4))
        .map_err(|err| format!("read rw len failed: {err:?}"))?;

    let ext_table_addr =
        alloc_required_guest_block(memory, host, LEGACY_EXT_TABLE_SIZE, "ext table")?;
    copy_guest_words(memory, mr_table_addr.get(), ext_table_addr, LEGACY_EXT_TABLE_SIZE)
        .map_err(|err| format!("copy ext table failed: {err:?}"))?;

    let ext_chunk_addr =
        alloc_required_guest_block(memory, host, LEGACY_EXT_HANDLE_SIZE, "ext handle")?;
    zero_guest_range(memory, ext_chunk_addr, LEGACY_EXT_HANDLE_SIZE)
        .map_err(|err| format!("clear ext handle failed: {err:?}"))?;

    let timer_addr =
        alloc_required_guest_block(memory, host, LEGACY_TIMER_STRUCT_SIZE, "ext timer")?;
    zero_guest_range(memory, timer_addr, LEGACY_TIMER_STRUCT_SIZE)
        .map_err(|err| format!("clear ext timer failed: {err:?}"))?;
    write_u32(memory, timer_addr, LEGACY_TIMER_CHECK)
        .map_err(|err| format!("seed ext timer failed: {err:?}"))?;

    write_u32(memory, DEFAULT_LAYOUT.code_address().get(), ext_table_addr)
        .map_err(|err| format!("seed ext table pointer failed: {err:?}"))?;
    write_u32(memory, ext_chunk_addr, LEGACY_EXT_CHUNK_CHECK)
        .map_err(|err| format!("seed ext handle check failed: {err:?}"))?;
    write_u32(memory, ext_chunk_addr + 0x04, ext_entry_addr)
        .map_err(|err| format!("seed ext init func failed: {err:?}"))?;
    write_u32(memory, ext_chunk_addr + 0x08, helper_addr)
        .map_err(|err| format!("seed ext helper failed: {err:?}"))?;
    write_u32(memory, ext_chunk_addr + 0x0C, DEFAULT_LAYOUT.code_address().get())
        .map_err(|err| format!("seed ext code base failed: {err:?}"))?;
    write_u32(memory, ext_chunk_addr + 0x10, ext_code_len)
        .map_err(|err| format!("seed ext code len failed: {err:?}"))?;
    write_u32(memory, ext_chunk_addr + 0x14, var_buf)
        .map_err(|err| format!("seed ext rw base failed: {err:?}"))?;
    write_u32(memory, ext_chunk_addr + 0x18, var_len)
        .map_err(|err| format!("seed ext rw len failed: {err:?}"))?;
    write_u32(memory, ext_chunk_addr + 0x1C, c_function_p_addr.get())
        .map_err(|err| format!("seed ext context failed: {err:?}"))?;
    write_u32(memory, c_function_p_addr.get() + 8, 1)
        .map_err(|err| format!("seed ext_type failed: {err:?}"))?;
    write_u32(memory, ext_chunk_addr + 0x20, 20)
        .map_err(|err| format!("seed ext context len failed: {err:?}"))?;
    write_u32(memory, ext_chunk_addr + 0x24, timer_addr)
        .map_err(|err| format!("seed ext timer ptr failed: {err:?}"))?;
    write_u32(memory, ext_chunk_addr + 0x28, SEND_APP_EVENT_ADDR.get())
        .map_err(|err| format!("seed sendAppEvent failed: {err:?}"))?;
    write_u32(memory, ext_chunk_addr + 0x2C, ext_table_addr)
        .map_err(|err| format!("seed ext table ptr failed: {err:?}"))?;

    write_u32(
        memory,
        c_function_p_addr.get() + MR_C_FUNCTION_CONTEXT_EXT_CHUNK_OFFSET,
        ext_chunk_addr,
    )
    .map_err(|err| format!("seed context ext chunk failed: {err:?}"))?;
    write_u32(
        memory,
        c_function_p_addr.get() + MR_C_FUNCTION_CONTEXT_STACK_OFFSET,
        DEFAULT_LAYOUT.stack_size(),
    )
    .map_err(|err| format!("seed context stack failed: {err:?}"))?;

    Ok(SeededExtChunkDescriptor {
        ext_chunk_addr,
        ext_table_addr,
        timer_addr,
    })
}

fn apply_helper_rw_debug_overrides(
    memory: &mut TestMemory,
    c_function_p: GuestAddr,
    verbose: bool,
) -> Result<(), vmrp_cpu::MemoryAccessError> {
    let rw_base = memory.read32(GuestAddr::new(c_function_p.get()))?;
    if rw_base == 0 {
        return Ok(());
    }

    for (env_key, offset) in [("VMRP_FORCE_RW20", 0x20u32), ("VMRP_FORCE_RW24", 0x24u32)] {
        let Ok(raw) = std::env::var(env_key) else {
            continue;
        };
        let parsed = raw
            .strip_prefix("0x")
            .or_else(|| raw.strip_prefix("0X"))
            .and_then(|hex| u32::from_str_radix(hex, 16).ok())
            .or_else(|| raw.parse::<u32>().ok())
            .unwrap_or(0);
        write_u32(memory, rw_base.wrapping_add(offset), parsed)?;
        if verbose {
            println!("helper_debug_override[{env_key}]=0x{parsed:X}");
        }
    }
    Ok(())
}

fn dump_helper_state(memory: &TestMemory, c_function_p: GuestAddr, label: &str) {
    let rw_base = memory
        .read32(GuestAddr::new(c_function_p.get()))
        .unwrap_or(0);
    let rw_len = memory
        .read32(GuestAddr::new(c_function_p.get() + 4))
        .unwrap_or(0);
    let ext_type = memory
        .read32(GuestAddr::new(c_function_p.get() + 8))
        .unwrap_or(0);
    let ext_chunk = memory
        .read32(GuestAddr::new(
            c_function_p.get() + MR_C_FUNCTION_CONTEXT_EXT_CHUNK_OFFSET,
        ))
        .unwrap_or(0);
    let stack = memory
        .read32(GuestAddr::new(
            c_function_p.get() + MR_C_FUNCTION_CONTEXT_STACK_OFFSET,
        ))
        .unwrap_or(0);
    println!("{label}_rw_base=0x{rw_base:X}");
    println!("{label}_rw_len=0x{rw_len:X}");
    println!("{label}_ext_type={ext_type}");
    println!("{label}_ext_chunk=0x{ext_chunk:X}");
    println!("{label}_stack=0x{stack:X}");
    if rw_base != 0 {
        for offset in [0x00u32, 0x04, 0x08, 0x0C, 0x10, 0x14, 0x18, 0x1C, 0x20, 0x24, 0x2C, 0x30, 0x5C, 0x60, 0x64, 0x68, 0x6C, 0x70, 0x74, 0x88, 0x8C, 0x100, 0x104, 0x108, 0x10C, 0x110, 0x120, 0x124, 0x128, 0x12C, 0x130, 0x134, 0x138, 0x13C, 0x140, 0x15C, 0x160, 0x164, 0x168, 0x1A8, 0x1AC, 0x220, 0x224, 0x1BC, 0x1C0] {
            let value = memory
                .read32(GuestAddr::new(rw_base.wrapping_add(offset)))
                .unwrap_or(0);
            println!("{label}_rw[0x{offset:X}]=0x{value:X}");
        }
    }
    if ext_chunk != 0 {
        for offset in [
            0x00u32, 0x04, 0x08, 0x0C, 0x10, 0x14, 0x18, 0x1C, 0x20, 0x24, 0x28, 0x2C,
        ] {
            let value = memory
                .read32(GuestAddr::new(ext_chunk.wrapping_add(offset)))
                .unwrap_or(0);
            println!("{label}_ext_chunk[0x{offset:X}]=0x{value:X}");
        }
    }
}

fn setup_helper_call(
    cpu: &mut Cpu<TestMemory>,
    helper: GuestAddr,
    c_function_p: GuestAddr,
    code: u32,
    event_addr: u32,
    input_len: u32,
) {
    let helper_pc = helper.get() & !1;
    let helper_thumb = (helper.get() & 1) != 0;
    cpu.regs_mut().set_pc(helper_pc);
    cpu.regs_mut().set_execution_mode(if helper_thumb {
        ExecutionMode::Thumb
    } else {
        ExecutionMode::Arm
    });
    cpu.regs_mut().set_sp(default_stack_top());
    cpu.regs_mut().set_lr(0);
    cpu.regs_mut().set_reg(0, c_function_p.get());
    cpu.regs_mut().set_reg(1, code);
    cpu.regs_mut().set_reg(2, event_addr);
    cpu.regs_mut().set_reg(3, input_len);
}

fn helper_bootstrap_sequence(
    helper_load_addr: u32,
    payload: HelperBootstrapPayload,
) -> Vec<(u32, u32, u32)> {
    vec![
        (6, helper_load_addr, payload.version_code),
        (8, payload.app_info_addr, HELPER_APP_INFO_SIZE),
        (0, helper_load_addr, payload.version_code),
    ]
}

fn should_skip_helper_bootstrap(mrp: &MrpFile) -> bool {
    mrp.entry("game.ext").is_some() && mrp.entry("plugins.lst").is_some()
}

fn helper_bootstrap_sequence_for_mrp(
    mrp: &MrpFile,
    helper_load_addr: u32,
    payload: HelperBootstrapPayload,
) -> Vec<(u32, u32, u32)> {
    if should_skip_helper_bootstrap(mrp) {
        vec![(0, helper_load_addr, payload.version_code)]
    } else {
        helper_bootstrap_sequence(helper_load_addr, payload)
    }
}

fn mrp_guest_filename(mrp_path: &str) -> String {
    Path::new(mrp_path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| mrp_path.to_string())
}
fn prepare_start_dsm_payload(
    memory: &mut TestMemory,
    runtime_data_base: u32,
    mrp_path: &str,
) -> Result<u32, vmrp_cpu::MemoryAccessError> {
    let mut cursor = runtime_data_base;
    let filename_ptr = write_c_string(memory, &mut cursor, &mrp_guest_filename(mrp_path))?;
    let ext_ptr = write_c_string(memory, &mut cursor, "start.mr")?;

    let start_t_addr = runtime_data_base + RUNTIME_START_T_OFFSET;
    write_u32(memory, start_t_addr, filename_ptr)?;
    write_u32(memory, start_t_addr + 4, ext_ptr)?;
    write_u32(memory, start_t_addr + 8, 0)?;

    Ok(start_t_addr)
}

fn write_event(
    memory: &mut TestMemory,
    event_addr: u32,
    code: i32,
    p0: u32,
    p1: u32,
) -> Result<u32, vmrp_cpu::MemoryAccessError> {
    write_u32(memory, event_addr, code as u32)?;
    write_u32(memory, event_addr + 4, p0)?;
    write_u32(memory, event_addr + 8, p1)?;
    Ok(event_addr)
}

fn apply_bridge_compatible_helper_state<B: MemoryBus>(
    memory: &mut B,
    c_function_p: GuestAddr,
) -> Result<(), vmrp_cpu::MemoryAccessError> {
    let rw_base = memory.read32(GuestAddr::new(c_function_p.get()))?;
    if rw_base == 0 {
        return Ok(());
    }
    memory.write32(GuestAddr::new(rw_base.wrapping_add(0x04)), 0)?;
    Ok(())
}

fn seed_helper_runtime_compat_slots<B: MemoryBus>(
    memory: &mut B,
    c_function_p: GuestAddr,
    mr_table_addr: GuestAddr,
    mr_malloc_addr: GuestAddr,
    mr_free_addr: GuestAddr,
) -> Result<(), vmrp_cpu::MemoryAccessError> {
    let rw_base = memory.read32(GuestAddr::new(c_function_p.get()))?;
    let rw_len = memory.read32(GuestAddr::new(c_function_p.get().wrapping_add(4)))?;
    if rw_base == 0 {
        return Ok(());
    }

    let internal_table = memory.read32(GuestAddr::new(
        mr_table_addr
            .get()
            .wrapping_add(MR_C_INTERNAL_TABLE_SLOT_OFFSET),
    ))?;
    for (offset, value) in [
        (HELPER_RW_MR_MALLOC_SLOT_OFFSET, mr_malloc_addr.get()),
        (HELPER_RW_MR_FREE_SLOT_OFFSET, mr_free_addr.get()),
        (HELPER_RW_INTERNAL_TABLE_OFFSET, internal_table),
    ] {
        if rw_len < offset.wrapping_add(4) {
            continue;
        }
        let slot = GuestAddr::new(rw_base.wrapping_add(offset));
        if memory.read32(slot)? == 0 && value != 0 {
            memory.write32(slot, value)?;
        }
    }

    Ok(())
}

fn write_u32(
    memory: &mut TestMemory,
    addr: u32,
    value: u32,
) -> Result<(), vmrp_cpu::MemoryAccessError> {
    memory.write32(GuestAddr::new(addr), value)
}

fn write_c_string(
    memory: &mut TestMemory,
    cursor: &mut u32,
    value: &str,
) -> Result<u32, vmrp_cpu::MemoryAccessError> {
    let start = *cursor;
    for byte in value.as_bytes() {
        memory.write8(GuestAddr::new(*cursor), *byte)?;
        *cursor = cursor.wrapping_add(1);
    }
    memory.write8(GuestAddr::new(*cursor), 0)?;
    *cursor = cursor.wrapping_add(1);
    *cursor = (*cursor + 3) & !3;
    Ok(start)
}

#[cfg(test)]
mod tests {
    use super::{
        apply_bridge_compatible_helper_state,
        default_stack_top, format_step_trace_line, guest_ram_size, helper_bootstrap_sequence,
        helper_bootstrap_sequence_for_mrp,
        is_successful_stage_stop_reason,
        helper_ext_search_paths, mrp_guest_filename, parse_args_from, parse_watch_u32,
        prepare_helper_bootstrap_payload, prepare_start_dsm_payload,
        push_recent_trace_line, seed_helper_runtime_compat_slots, HelperBootstrapPayload,
        HelperRwWatch, should_skip_helper_bootstrap,
        should_keep_waiting_for_host_events, to_presenter_rect, ParseOutcome,
        HELPER_RW_INTERNAL_TABLE_OFFSET, HELPER_RW_MR_FREE_SLOT_OFFSET,
        HELPER_RW_MR_MALLOC_SLOT_OFFSET, MR_C_INTERNAL_TABLE_SLOT_OFFSET,
    };
    use crate::{presenter::DirtyRect, DSM_INIT_CODE, HELPER_APP_INFO_SIZE, HELPER_EVENT_SIZE, write_event};
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use vmrp_abi::MrpFile;
    use vmrp_core::{GuestAddr, DEFAULT_LAYOUT};
    use vmrp_cpu::{ExecutionMode, MemoryBus, StepTrace, TestMemory};
    use vmrp_platform::HostScreenRegion;

    #[test]
    fn parse_args_enables_window_mode() {
        let parse = parse_args_from(["--window", "demo.mrp"]);

        match parse.expect("parse should succeed") {
            ParseOutcome::Config(config) => {
                assert!(config.window);
                assert_eq!(config.mrp_path, "demo.mrp");
            }
            ParseOutcome::Help => panic!("expected config outcome"),
        }
    }

    #[test]
    fn converts_host_dirty_region_to_presenter_rect() {
        let rect = to_presenter_rect(HostScreenRegion {
            x: -3,
            y: 4,
            w: 5,
            h: 6,
        });

        assert_eq!(
            rect,
            DirtyRect {
                x: -3,
                y: 4,
                w: 5,
                h: 6,
            }
        );
    }

    #[test]
    fn window_idle_policy_waits_for_input_after_first_frame() {
        assert!(should_keep_waiting_for_host_events(true, None, 0, 4000));
        assert!(!should_keep_waiting_for_host_events(false, None, 0, 4000));
        assert!(should_keep_waiting_for_host_events(
            false,
            Some(10),
            0,
            4000
        ));
        assert!(!should_keep_waiting_for_host_events(
            false,
            Some(10),
            4000,
            4000
        ));
    }

    #[test]
    fn helper_search_checks_parent_directories() {
        let paths = helper_ext_search_paths(r"D:\opt\rust\vmrp\wasm\dist\fs\mythroad\ydqtwo.mrp");
        let rendered = paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>();

        assert!(rendered.contains(&String::from(
            r"D:\opt\rust\vmrp\wasm\dist\fs\mythroad\cfunction.ext"
        )));
        assert!(rendered.contains(&String::from(
            r"D:\opt\rust\vmrp\wasm\dist\fs\cfunction.ext"
        )));
    }

    #[test]
    fn helper_bootstrap_payload_uses_legacy_engine_version_and_mrp_metadata() {
        let mrp = MrpFile::from_path(PathBuf::from(
            r"D:\opt\rust\vmrp\wasm\dist\fs\mythroad\mpc.mrp",
        ))
        .unwrap();
        let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x900000);

        let payload = prepare_helper_bootstrap_payload(&mut memory, 0x280400, &mrp).unwrap();

        assert_eq!(payload.version_code, 1968);
        assert_eq!(payload.app_info_addr, 0x280700);
        assert_eq!(
            memory
                .read32(GuestAddr::new(payload.app_info_addr))
                .unwrap(),
            mrp.header().appid()
        );
        assert_eq!(
            memory
                .read32(GuestAddr::new(payload.app_info_addr + 4))
                .unwrap(),
            mrp.header().version()
        );
    }

    #[test]
    fn helper_bootstrap_is_skipped_for_bridge_compatible_game_ext_packages() {
        let mpc = MrpFile::from_path(PathBuf::from(
            r"D:\opt\rust\vmrp\wasm\dist\fs\mythroad\mpc.mrp",
        ))
        .unwrap();
        let asm = MrpFile::from_path(PathBuf::from(r"D:\opt\rust\vmrp\mrc\asm\asm.mrp")).unwrap();

        assert!(should_skip_helper_bootstrap(&mpc));
        assert!(!should_skip_helper_bootstrap(&asm));
    }

    #[test]
    fn helper_bootstrap_sequence_is_reduced_for_bridge_compatible_packages() {
        let payload = HelperBootstrapPayload {
            version_code: 1968,
            app_info_addr: 0x280700,
        };
        let mpc = MrpFile::from_path(PathBuf::from(
            r"D:\opt\rust\vmrp\wasm\dist\fs\mythroad\mpc.mrp",
        ))
        .unwrap();
        let asm = MrpFile::from_path(PathBuf::from(r"D:\opt\rust\vmrp\mrc\asm\asm.mrp")).unwrap();

        assert_eq!(
            helper_bootstrap_sequence_for_mrp(&mpc, 0x80000, payload),
            vec![(0, 0x80000, 1968)]
        );
        assert_eq!(
            helper_bootstrap_sequence_for_mrp(&asm, 0x80000, payload),
            helper_bootstrap_sequence(0x80000, payload)
        );
    }

    #[test]
    fn run_ok_only_accepts_known_success_stop_reasons() {
        assert!(is_successful_stage_stop_reason("program returned to null pc"));
        assert!(is_successful_stage_stop_reason("guest requested exit"));
        assert!(!is_successful_stage_stop_reason("Memory(OutOfRange(GuestAddr(4)))"));
        assert!(!is_successful_stage_stop_reason("step budget exhausted"));
    }

    #[test]
    fn bridge_compatible_helper_state_clears_runtime_mode_flag_but_keeps_heap_markers() {
        let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x900000);
        let c_function_p = GuestAddr::new(0x182000);
        let rw_base = 0x282020;

        memory.write32(c_function_p, rw_base).unwrap();
        memory.write32(GuestAddr::new(rw_base + 0x04), 1).unwrap();
        memory.write32(GuestAddr::new(rw_base + 0x15C), 0x282AA8).unwrap();
        memory.write32(GuestAddr::new(rw_base + 0x160), 0x96000).unwrap();
        memory.write32(GuestAddr::new(rw_base + 0x164), 0x11800).unwrap();

        apply_bridge_compatible_helper_state(&mut memory, c_function_p).unwrap();

        assert_eq!(memory.read32(GuestAddr::new(rw_base + 0x04)).unwrap(), 0);
        assert_eq!(memory.read32(GuestAddr::new(rw_base + 0x15C)).unwrap(), 0x282AA8);
        assert_eq!(memory.read32(GuestAddr::new(rw_base + 0x160)).unwrap(), 0x96000);
        assert_eq!(memory.read32(GuestAddr::new(rw_base + 0x164)).unwrap(), 0x11800);
    }

    #[test]
    fn start_dsm_payload_can_target_custom_runtime_base() {
        let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x900000);

        let start_t_addr = prepare_start_dsm_payload(
            &mut memory,
            0x280400,
            r"D:\opt\rust\vmrp\wasm\dist\fs\mythroad\mpc.mrp",
        )
        .unwrap();

        assert_eq!(start_t_addr, 0x280500);
        assert_eq!(memory.read32(GuestAddr::new(start_t_addr + 8)).unwrap(), 0);
    }

    #[test]
    fn start_dsm_payload_keeps_legacy_start_mr_entry_for_mpc_package() {
        let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x900000);

        let start_t_addr = prepare_start_dsm_payload(
            &mut memory,
            0x280400,
            r"D:\opt\rust\vmrp\wasm\dist\fs\mythroad\mpc.mrp",
        )
        .unwrap();

        let ext_ptr = memory.read32(GuestAddr::new(start_t_addr + 4)).unwrap();
        let mut bytes = Vec::new();
        for offset in 0..16u32 {
            let byte = memory.read8(GuestAddr::new(ext_ptr.wrapping_add(offset))).unwrap();
            if byte == 0 {
                break;
            }
            bytes.push(byte);
        }

        assert_eq!(String::from_utf8(bytes).unwrap(), "start.mr");
    }

    #[test]
    fn helper_event_payload_matches_legacy_bridge_event_t_layout() {
        let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x900000);

        let event_addr = write_event(&mut memory, 0x280120, DSM_INIT_CODE, 0x1111, 0x2222).unwrap();

        assert_eq!(HELPER_EVENT_SIZE, 12);
        assert_eq!(event_addr, 0x280120);
        assert_eq!(memory.read32(GuestAddr::new(0x280120)).unwrap(), DSM_INIT_CODE as u32);
        assert_eq!(memory.read32(GuestAddr::new(0x280124)).unwrap(), 0x1111);
        assert_eq!(memory.read32(GuestAddr::new(0x280128)).unwrap(), 0x2222);
    }

    #[test]
    fn seed_helper_runtime_compat_slots_populates_large_helper_layout() {
        let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x900000);
        let c_function_p = GuestAddr::new(0x182000);
        let rw_base = 0x282020;
        let mr_table_addr = GuestAddr::new(0x180000);

        memory.write32(c_function_p, rw_base).unwrap();
        memory
            .write32(GuestAddr::new(c_function_p.get().wrapping_add(4)), 0x3C4)
            .unwrap();
        memory
            .write32(
                GuestAddr::new(
                    mr_table_addr
                        .get()
                        .wrapping_add(MR_C_INTERNAL_TABLE_SLOT_OFFSET),
                ),
                0x180340,
            )
            .unwrap();

        seed_helper_runtime_compat_slots(
            &mut memory,
            c_function_p,
            mr_table_addr,
            GuestAddr::new(0x181100),
            GuestAddr::new(0x181140),
        )
        .unwrap();

        assert_eq!(
            memory
                .read32(GuestAddr::new(
                    rw_base.wrapping_add(HELPER_RW_MR_MALLOC_SLOT_OFFSET)
                ))
                .unwrap(),
            0x181100
        );
        assert_eq!(
            memory
                .read32(GuestAddr::new(
                    rw_base.wrapping_add(HELPER_RW_MR_FREE_SLOT_OFFSET)
                ))
                .unwrap(),
            0x181140
        );
        assert_eq!(
            memory
                .read32(GuestAddr::new(
                    rw_base.wrapping_add(HELPER_RW_INTERNAL_TABLE_OFFSET)
                ))
                .unwrap(),
            0x180340
        );
    }

    #[test]
    fn seed_helper_runtime_compat_slots_skips_offsets_outside_small_rw_layout() {
        let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x900000);
        let c_function_p = GuestAddr::new(0x182000);
        let rw_base = 0x282020;
        let mr_table_addr = GuestAddr::new(0x180000);

        memory.write32(c_function_p, rw_base).unwrap();
        memory
            .write32(
                GuestAddr::new(c_function_p.get().wrapping_add(4)),
                HELPER_RW_MR_FREE_SLOT_OFFSET.wrapping_add(4),
            )
            .unwrap();
        memory
            .write32(
                GuestAddr::new(
                    mr_table_addr
                        .get()
                        .wrapping_add(MR_C_INTERNAL_TABLE_SLOT_OFFSET),
                ),
                0x180340,
            )
            .unwrap();

        seed_helper_runtime_compat_slots(
            &mut memory,
            c_function_p,
            mr_table_addr,
            GuestAddr::new(0x181100),
            GuestAddr::new(0x181140),
        )
        .unwrap();

        assert_eq!(
            memory
                .read32(GuestAddr::new(
                    rw_base.wrapping_add(HELPER_RW_MR_MALLOC_SLOT_OFFSET)
                ))
                .unwrap(),
            0x181100
        );
        assert_eq!(
            memory
                .read32(GuestAddr::new(
                    rw_base.wrapping_add(HELPER_RW_MR_FREE_SLOT_OFFSET)
                ))
                .unwrap(),
            0x181140
        );
        assert_eq!(
            memory
                .read32(GuestAddr::new(
                    rw_base.wrapping_add(HELPER_RW_INTERNAL_TABLE_OFFSET)
                ))
                .unwrap(),
            0
        );
    }


    #[test]
    fn helper_bootstrap_sequence_matches_legacy_mr_do_ext_order() {
        let payload = HelperBootstrapPayload {
            version_code: 1968,
            app_info_addr: 0x280700,
        };

        assert_eq!(
            helper_bootstrap_sequence(0x80000, payload),
            vec![
                (6, 0x80000, 1968),
                (8, 0x280700, HELPER_APP_INFO_SIZE),
                (0, 0x80000, 1968),
            ]
        );
    }

    #[test]
    fn guest_ram_size_covers_default_layout_through_memory_manager_end() {
        let layout_end = DEFAULT_LAYOUT
            .memory_manager_address()
            .get()
            .wrapping_add(DEFAULT_LAYOUT.memory_manager_size());
        let ram_end = DEFAULT_LAYOUT
            .code_address()
            .get()
            .wrapping_add(guest_ram_size());

        assert_eq!(ram_end, layout_end);
    }

    #[test]
    fn default_stack_top_matches_start_of_memory_manager_region() {
        assert_eq!(
            default_stack_top(),
            DEFAULT_LAYOUT.memory_manager_address().get()
        );
    }

    #[test]
    fn mrp_guest_filename_strips_parent_directories() {
        assert_eq!(
            mrp_guest_filename(r"D:\opt\rust\vmrp\wasm\dist\fs\mythroad\ydqtwo.mrp"),
            "ydqtwo.mrp"
        );
    }

    #[test]
    fn recent_trace_buffer_keeps_only_latest_entries() {
        let mut recent = VecDeque::new();
        push_recent_trace_line(&mut recent, 2, String::from("line-1"));
        push_recent_trace_line(&mut recent, 2, String::from("line-2"));
        push_recent_trace_line(&mut recent, 2, String::from("line-3"));

        assert_eq!(
            recent.into_iter().collect::<Vec<_>>(),
            vec![String::from("line-2"), String::from("line-3")]
        );
    }

    #[test]
    fn helper_rw_watch_reports_value_changes_inside_watched_window() {
        let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x900000);
        let c_function_p = GuestAddr::new(0x182000);
        let rw_base = 0x282020;
        let mut watch = HelperRwWatch::new(c_function_p, [0x120u32, 0x124]);

        memory.write32(c_function_p, rw_base).unwrap();
        let sample = watch.sample(&memory).unwrap();
        assert_eq!(sample.rw_base, Some(rw_base));
        assert!(sample.base_reset);
        assert!(sample.changes.is_empty());

        memory
            .write32(GuestAddr::new(rw_base.wrapping_add(0x124)), 0xDEADBEEF)
            .unwrap();
        let sample = watch.sample(&memory).unwrap();
        assert_eq!(sample.rw_base, Some(rw_base));
        assert!(!sample.base_reset);
        assert_eq!(sample.changes.len(), 1);
        assert_eq!(sample.changes[0].offset, 0x124);
        assert_eq!(sample.changes[0].old, 0);
        assert_eq!(sample.changes[0].new, 0xDEADBEEF);
    }

    #[test]
    fn parse_watch_u32_accepts_hex_and_decimal_strings() {
        assert_eq!(parse_watch_u32("0x124"), Some(0x124));
        assert_eq!(parse_watch_u32("320"), Some(320));
        assert_eq!(parse_watch_u32("wat"), None);
    }

    #[test]
    fn formatted_step_trace_includes_mode_pc_and_opcode() {
        let rendered = format_step_trace_line(
            "start_dsm",
            17,
            &StepTrace {
                pc: 0x1234,
                mode: ExecutionMode::Thumb,
                opcode: 0xB500,
                register_writes: Vec::new(),
            },
        );

        assert_eq!(
            rendered,
            "start_dsm_step[17] mode=Thumb pc=0x00001234 op=0x0000B500 writes=[]"
        );
    }
}










