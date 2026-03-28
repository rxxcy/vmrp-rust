use std::path::Path;

use vmrp_abi::{MrChunk, MrpFile};
use vmrp_core::{GuestAddr, DEFAULT_LAYOUT};
use vmrp_cpu::{Cpu, ExecutionMode, MemoryBus, TestMemory};
use vmrp_platform::{
    ExtBootstrap, ExtHost, HostTimerCommand, FLAG_USE_UTF8_EDIT, MR_FAILED, VMRP_VER,
};
use vmrp_runtime::{Runtime, RuntimeEvent, RuntimeStepResult, StageResult};

const DEFAULT_MRP_PATH: &str = r"D:\opt\rust\vmrp\mrc\asm\asm.mrp";
const START_MR_ADDR: u32 = 0x190000;
const RUNTIME_DATA_ADDR: u32 = 0x1A0000;
const DSM_INIT_CODE: i32 = -100;
const MR_START_DSM_CODE: i32 = -99;
const MR_TIMER_CODE: i32 = -96;

#[derive(Debug, Clone)]
struct RunnerConfig {
    mrp_path: String,
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
            println!(
                "stage={} stop_reason={}",
                stage.label, stage.stop_reason
            );
        }
    }

    println!("mrp_bootstrap_run_ok={}", report.run_ok);
    std::process::exit(if report.run_ok { 0 } else { 1 });
}

fn usage() -> &'static str {
    "Usage: vmrp-windows [--verbose|-v] [--step-limit N] [--trace-limit N] [<path-to.mrp>]"
}

fn parse_args() -> Result<ParseOutcome, String> {
    let mut mrp_path: Option<String> = None;
    let mut verbose = false;
    let mut step_limit = 4000usize;
    let mut trace_limit = 200usize;

    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let mut index = 0usize;

    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--help" | "-h" => return Ok(ParseOutcome::Help),
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
        verbose,
        step_limit,
        trace_limit,
    }))
}

fn run_mrp(config: &RunnerConfig) -> Result<RunReport, String> {
    let mrp = MrpFile::from_path(&config.mrp_path)
        .map_err(|err| format!("read mrp failed: {err:?}"))?;
    let assets = mrp
        .runtime_assets()
        .map_err(|err| format!("decode runtime assets failed: {err:?}"))?;
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
                println!(
                    "start_mr_main_code_count={}",
                    chunk.main().code_count()
                );
            }
            Err(err) => {
                println!("start_mr_chunk_parse_error={err:?}");
            }
        }
    }

    let blob = ext.to_code_blob(DEFAULT_LAYOUT.code_address().get());
    let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x200000);

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

    let mut cpu = Cpu::new(memory);
    cpu.regs_mut().set_pc(blob.entry().get());
    cpu.regs_mut().set_execution_mode(blob.mode());
    cpu.regs_mut().set_sp(0x280000);

    let ext_entry_code = std::env::var("VMRP_EXT_CODE")
        .ok()
        .and_then(|raw| raw.parse::<u32>().ok())
        .unwrap_or(1);
    cpu.regs_mut().set_reg(0, ext_entry_code);

    let mut runtime = Runtime::new();
    let mut stages = Vec::new();

    let stage_ext_init = run_loop(
        &mut runtime,
        &mut cpu,
        &mut host,
        config.step_limit,
        config.trace_limit,
        "ext_init",
        config.verbose,
    );
    stages.push(stage_ext_init);

    let helper = host.ext_helper_addr();
    let helper_addr = helper.map(|v| v.get());

    let mut dsm_init_ret = MR_FAILED;
    let mut start_dsm_ret = MR_FAILED;

    if let Some(helper) = helper {
        setup_helper_call(&mut cpu, helper, host.c_function_p_addr(), 0, 0, 0);
        let stage_helper_init = run_loop(
        &mut runtime,
        &mut cpu,
            &mut host,
            config.step_limit,
            config.trace_limit,
            "helper_init",
            config.verbose,
        );
        stages.push(stage_helper_init);

        let dsm_require_funcs_addr = RUNTIME_DATA_ADDR + 0x200;
        host.install_dsm_require_funcs(
            cpu.memory_mut(),
            GuestAddr::new(dsm_require_funcs_addr),
            FLAG_USE_UTF8_EDIT,
        )
        .map_err(|err| format!("install DSM_REQUIRE_FUNCS failed: {err:?}"))?;

        let dsm_init_event_addr = write_event(cpu.memory_mut(), DSM_INIT_CODE, dsm_require_funcs_addr, 0)
            .map_err(|err| format!("write DSM_INIT event failed: {err:?}"))?;
        setup_helper_call(
            &mut cpu,
            helper,
            host.c_function_p_addr(),
            1,
            dsm_init_event_addr,
            12,
        );
        let stage_dsm_init = run_loop(
        &mut runtime,
        &mut cpu,
            &mut host,
            config.step_limit,
            config.trace_limit,
            "dsm_init",
            config.verbose,
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
            12,
        );
        let stage_start_dsm = run_loop(
        &mut runtime,
        &mut cpu,
            &mut host,
            config.step_limit,
            config.trace_limit,
            "start_dsm",
            config.verbose,
        );
        start_dsm_ret = cpu.regs().reg(0) as i32;
        stages.push(stage_start_dsm);

        while let Some(event) = runtime.pop_event() {
            match event {
                RuntimeEvent::Bootstrap => {}
                RuntimeEvent::Timer => {
                    let timer_event_addr = write_event(cpu.memory_mut(), MR_TIMER_CODE, 0, 0)
                        .map_err(|err| format!("write MR_TIMER event failed: {err:?}"))?;
                    setup_helper_call(
                        &mut cpu,
                        helper,
                        host.c_function_p_addr(),
                        1,
                        timer_event_addr,
                        12,
                    );
                    let stage_timer = run_loop(
                        &mut runtime,
                        &mut cpu,
                        &mut host,
                        config.step_limit,
                        config.trace_limit,
                        "timer",
                        config.verbose,
                    );
                    stages.push(stage_timer);
                }
            }
        }
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

fn run_loop(
    runtime: &mut Runtime,
    cpu: &mut Cpu<TestMemory>,
    host: &mut ExtHost,
    step_limit: usize,
    trace_limit: usize,
    label: &'static str,
    verbose: bool,
) -> StageResult {
    let mut observed = 0usize;

    Runtime::run_stage(label, step_limit, || {
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
                if verbose && observed <= trace_limit {
                    println!("{label}_host_step[{observed}]=0x{:08X}", cpu.regs().pc());
                }
                return RuntimeStepResult::HostStep;
            }
            Ok(false) => {}
            Err(err) => {
                return RuntimeStepResult::Stop(format!("host callback error: {err:?}"));
            }
        }

        let pre_pc = cpu.regs().pc();
        match cpu.step() {
            Ok(step) => {
                observed += 1;
                if verbose && observed <= trace_limit {
                    println!(
                        "{label}_step[{observed}] pc=0x{pre_pc:08X} op=0x{:08X}",
                        step.trace.opcode
                    );
                }
                RuntimeStepResult::GuestStep
            }
            Err(err) => RuntimeStepResult::Stop(format!("{err:?}")),
        }
    })
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
    cpu.regs_mut().set_sp(0x280000);
    cpu.regs_mut().set_lr(0);
    cpu.regs_mut().set_reg(0, c_function_p.get());
    cpu.regs_mut().set_reg(1, code);
    cpu.regs_mut().set_reg(2, event_addr);
    cpu.regs_mut().set_reg(3, input_len);
}

fn prepare_start_dsm_payload(
    memory: &mut TestMemory,
    mrp_path: &str,
) -> Result<u32, vmrp_cpu::MemoryAccessError> {
    let mut cursor = RUNTIME_DATA_ADDR;
    let filename_ptr = write_c_string(memory, &mut cursor, mrp_path)?;
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





