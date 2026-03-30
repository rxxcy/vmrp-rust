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
    ExtBootstrap, ExtHost, HostScreenRegion, HostTimerCommand, FLAG_USE_UTF8_EDIT, MR_FAILED,
    VMRP_VER,
};
use vmrp_runtime::{Runtime, RuntimeEvent, RuntimeStepResult, StageResult};

const DEFAULT_MRP_PATH: &str = r"D:\opt\rust\vmrp\mrc\asm\asm.mrp";
const START_MR_ADDR: u32 = 0x190000;
const RUNTIME_DATA_ADDR: u32 = 0x1A0000;
const DSM_INIT_CODE: i32 = -100;
const MR_START_DSM_CODE: i32 = -99;
const MR_TIMER_CODE: i32 = -96;
const MR_EVENT_CODE: i32 = -95;
const HELPER_EVENT_SIZE: u32 = 20;
const LEGACY_MR_VERSION: u32 = 1968;
const HELPER_APP_INFO_ADDR: u32 = RUNTIME_DATA_ADDR + 0x300;
const HELPER_APP_INFO_SID_NAME_ADDR: u32 = RUNTIME_DATA_ADDR + 0x340;
const HELPER_APP_INFO_SIZE: u32 = 16;
const EXT_CHUNK_ADDR: u32 = 0x182100;
const MR_C_FUNCTION_CONTEXT_EXT_CHUNK_OFFSET: u32 = 0x0C;
const MR_C_FUNCTION_CONTEXT_STACK_OFFSET: u32 = 0x10;
const LEGACY_EXT_CHUNK_CHECK: u32 = 0x7FD854EB;
const LEGACY_EXT_TIMER_HANDLE: u32 = 0x270F;
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
        .unwrap_or(1);
    cpu.regs_mut().set_reg(0, ext_entry_code);

    let mut runtime = Runtime::new();
    let mut stages = Vec::new();
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
        &mut presenter,
    );
    stages.push(stage_ext_init);

    let helper = host.ext_helper_addr();
    let helper_addr = helper.map(|v| v.get());

    let mut dsm_init_ret = MR_FAILED;
    let mut start_dsm_ret = MR_FAILED;

    if let Some(helper) = helper {
        seed_ext_chunk_descriptor(
            cpu.memory_mut(),
            &bootstrap,
            blob.entry().get(),
            blob.bytes().len() as u32 + 8,
            helper.get(),
        )
        .map_err(|err| format!("seed ext chunk descriptor failed: {err:?}"))?;

        let helper_bootstrap = prepare_helper_bootstrap_payload(cpu.memory_mut(), &mrp)
            .map_err(|err| format!("prepare helper bootstrap payload failed: {err:?}"))?;

        setup_helper_call(
            &mut cpu,
            helper,
            host.c_function_p_addr(),
            6,
            blob.load_address().get(),
            helper_bootstrap.version_code,
        );
        let stage_helper_version = run_loop(
            &mut runtime,
            &mut cpu,
            &mut host,
            config.step_limit,
            config.trace_limit,
            "helper_version",
            config.verbose,
            &mut presenter,
        );
        stages.push(stage_helper_version);
        if true {
            dump_helper_state(cpu.memory(), host.c_function_p_addr(), "helper_version");
        }

        setup_helper_call(
            &mut cpu,
            helper,
            host.c_function_p_addr(),
            8,
            helper_bootstrap.app_info_addr,
            HELPER_APP_INFO_SIZE,
        );
        let stage_helper_app_info = run_loop(
            &mut runtime,
            &mut cpu,
            &mut host,
            config.step_limit,
            config.trace_limit,
            "helper_app_info",
            config.verbose,
            &mut presenter,
        );
        stages.push(stage_helper_app_info);
        if true {
            dump_helper_state(cpu.memory(), host.c_function_p_addr(), "helper_app_info");
        }

        setup_helper_call(
            &mut cpu,
            helper,
            host.c_function_p_addr(),
            0,
            blob.load_address().get(),
            helper_bootstrap.version_code,
        );
        let stage_helper_init = run_loop(
            &mut runtime,
            &mut cpu,
            &mut host,
            config.step_limit,
            config.trace_limit,
            "helper_init",
            config.verbose,
            &mut presenter,
        );
        stages.push(stage_helper_init);
        if true {
            dump_helper_state(cpu.memory(), host.c_function_p_addr(), "helper_init");
        }
        apply_helper_rw_debug_overrides(cpu.memory_mut(), host.c_function_p_addr(), config.verbose)
            .map_err(|err| format!("apply helper rw debug overrides failed: {err:?}"))?;

        let dsm_require_funcs_addr = RUNTIME_DATA_ADDR + 0x200;
        host.install_dsm_require_funcs(
            cpu.memory_mut(),
            GuestAddr::new(dsm_require_funcs_addr),
            FLAG_USE_UTF8_EDIT,
        )
        .map_err(|err| format!("install DSM_REQUIRE_FUNCS failed: {err:?}"))?;

        let dsm_init_event_addr =
            write_event(cpu.memory_mut(), DSM_INIT_CODE, dsm_require_funcs_addr, 0)
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
            &mut presenter,
        );
        dsm_init_ret = cpu.regs().reg(0) as i32;
        stages.push(stage_dsm_init);

        let start_t_addr = prepare_start_dsm_payload(cpu.memory_mut(), &config.mrp_path)
            .map_err(|err| format!("prepare MR_START_DSM payload failed: {err:?}"))?;
        let start_event_addr = write_event(cpu.memory_mut(), MR_START_DSM_CODE, start_t_addr, 0)
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
            &mut presenter,
        );
        start_dsm_ret = cpu.regs().reg(0) as i32;
        stages.push(stage_start_dsm);

        drive_runtime_events(
            &mut runtime,
            &mut cpu,
            &mut host,
            helper,
            config,
            &mut presenter,
            &mut stages,
        )?;
    }

    let no_unimplemented = stages
        .iter()
        .all(|stage| !stage.stop_reason.contains("UnimplementedInstruction"));
    let has_helper = helper_addr.is_some();
    let version_ok = dsm_init_ret == VMRP_VER || dsm_init_ret == 0;
    let start_ok = start_dsm_ret != MR_FAILED;

    let run_ok = no_unimplemented && has_helper && version_ok && start_ok;

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

fn dump_recent_trace(label: &str, recent: &VecDeque<String>, limit: usize) {
    println!("stage={label} recent_trace_begin");
    let start = recent.len().saturating_sub(limit);
    for line in recent.iter().skip(start) {
        println!("{line}");
    }
    println!("stage={label} recent_trace_end");
}

fn run_loop(
    runtime: &mut Runtime,
    cpu: &mut Cpu<TestMemory>,
    host: &mut ExtHost,
    step_limit: usize,
    trace_limit: usize,
    label: &'static str,
    verbose: bool,
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
                let line = format!("{label}_host_step[{observed}] pc=0x{:08X}", cpu.regs().pc());
                push_recent_trace_line(&mut recent, RECENT_TRACE_CAPACITY, line.clone());
                if verbose && observed <= trace_limit {
                    println!("{line}");
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
    config: &RunnerConfig,
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
                    let timer_event_addr = write_event(cpu.memory_mut(), MR_TIMER_CODE, 0, 0)
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
                        presenter,
                    );
                    stages.push(stage_timer);
                }
                RuntimeEvent::GuestEvent { code, p0, p1 } => {
                    let event_payload_addr = write_event(cpu.memory_mut(), code, p0, p1)
                        .map_err(|err| format!("write MR_EVENT payload failed: {err:?}"))?;
                    let event_addr =
                        write_event(cpu.memory_mut(), MR_EVENT_CODE, event_payload_addr, 0)
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
    mrp: &MrpFile,
) -> Result<HelperBootstrapPayload, vmrp_cpu::MemoryAccessError> {
    let mut sid_name_cursor = HELPER_APP_INFO_SID_NAME_ADDR;
    let sid_name_ptr = write_c_string(memory, &mut sid_name_cursor, mrp.internal_name())?;
    write_u32(memory, HELPER_APP_INFO_ADDR, mrp.header().appid())?;
    write_u32(memory, HELPER_APP_INFO_ADDR + 4, mrp.header().version())?;
    write_u32(memory, HELPER_APP_INFO_ADDR + 8, sid_name_ptr)?;
    write_u32(memory, HELPER_APP_INFO_ADDR + 12, 0)?;

    Ok(HelperBootstrapPayload {
        version_code: LEGACY_MR_VERSION,
        app_info_addr: HELPER_APP_INFO_ADDR,
    })
}

fn seed_ext_chunk_descriptor(
    memory: &mut TestMemory,
    bootstrap: &ExtBootstrap,
    ext_entry_addr: u32,
    ext_code_len: u32,
    helper_addr: u32,
) -> Result<(), vmrp_cpu::MemoryAccessError> {
    let var_buf = memory.read32(GuestAddr::new(bootstrap.mr_c_function_p_addr.get()))?;
    let var_len = memory.read32(GuestAddr::new(bootstrap.mr_c_function_p_addr.get() + 4))?;

    write_u32(memory, EXT_CHUNK_ADDR, LEGACY_EXT_CHUNK_CHECK)?;
    write_u32(memory, EXT_CHUNK_ADDR + 0x04, ext_entry_addr)?;
    write_u32(memory, EXT_CHUNK_ADDR + 0x08, helper_addr)?;
    write_u32(
        memory,
        EXT_CHUNK_ADDR + 0x0C,
        DEFAULT_LAYOUT.code_address().get(),
    )?;
    write_u32(memory, EXT_CHUNK_ADDR + 0x10, ext_code_len)?;
    write_u32(memory, EXT_CHUNK_ADDR + 0x14, var_buf)?;
    write_u32(memory, EXT_CHUNK_ADDR + 0x18, var_len)?;
    write_u32(
        memory,
        EXT_CHUNK_ADDR + 0x1C,
        bootstrap.mr_c_function_p_addr.get(),
    )?;
    write_u32(memory, EXT_CHUNK_ADDR + 0x20, 20)?;
    write_u32(memory, EXT_CHUNK_ADDR + 0x24, LEGACY_EXT_TIMER_HANDLE)?;
    write_u32(memory, EXT_CHUNK_ADDR + 0x28, 0)?;
    write_u32(memory, EXT_CHUNK_ADDR + 0x2C, bootstrap.mr_table_addr.get())?;

    write_u32(
        memory,
        bootstrap.mr_c_function_p_addr.get() + MR_C_FUNCTION_CONTEXT_EXT_CHUNK_OFFSET,
        EXT_CHUNK_ADDR,
    )?;
    write_u32(
        memory,
        bootstrap.mr_c_function_p_addr.get() + MR_C_FUNCTION_CONTEXT_STACK_OFFSET,
        DEFAULT_LAYOUT.stack_size(),
    )?;
    Ok(())
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
        for offset in [0x20u32, 0x24, 0x1BC, 0x1C0] {
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

fn mrp_guest_filename(mrp_path: &str) -> String {
    Path::new(mrp_path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| mrp_path.to_string())
}
fn prepare_start_dsm_payload(
    memory: &mut TestMemory,
    mrp_path: &str,
) -> Result<u32, vmrp_cpu::MemoryAccessError> {
    let mut cursor = RUNTIME_DATA_ADDR;
    let filename_ptr = write_c_string(memory, &mut cursor, &mrp_guest_filename(mrp_path))?;
    let ext_ptr = write_c_string(memory, &mut cursor, "start.mr")?;

    let start_t_addr = RUNTIME_DATA_ADDR + 0x100;
    write_u32(memory, start_t_addr, filename_ptr)?;
    write_u32(memory, start_t_addr + 4, ext_ptr)?;
    write_u32(memory, start_t_addr + 8, 0)?;

    Ok(start_t_addr)
}

fn write_event(
    memory: &mut TestMemory,
    code: i32,
    p0: u32,
    p1: u32,
) -> Result<u32, vmrp_cpu::MemoryAccessError> {
    let event_addr = RUNTIME_DATA_ADDR + 0x120;
    write_u32(memory, event_addr, code as u32)?;
    write_u32(memory, event_addr + 4, p0)?;
    write_u32(memory, event_addr + 8, p1)?;
    Ok(event_addr)
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
        default_stack_top, format_step_trace_line, guest_ram_size, helper_ext_search_paths,
        mrp_guest_filename, parse_args_from, prepare_helper_bootstrap_payload,
        push_recent_trace_line, should_keep_waiting_for_host_events, to_presenter_rect,
        ParseOutcome,
    };
    use crate::presenter::DirtyRect;
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

        let payload = prepare_helper_bootstrap_payload(&mut memory, &mrp).unwrap();

        assert_eq!(payload.version_code, 1968);
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

