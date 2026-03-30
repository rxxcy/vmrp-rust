use std::path::PathBuf;

use vmrp_abi::ExtFile;
use vmrp_core::{GuestAddr, DEFAULT_LAYOUT};
use vmrp_cpu::{Cpu, ExecutionMode, MemoryBus, TestMemory};
use vmrp_platform::{ExtBootstrap, ExtHost};

fn real_ext_path() -> PathBuf {
    PathBuf::from(r"D:\opt\rust\vmrp\mrc\asm\cfunction.ext")
}

fn new_bootstrapped_real_ext_cpu() -> (Cpu<TestMemory>, ExtHost) {
    let ext = ExtFile::from_path(real_ext_path()).unwrap();
    let blob = ext.to_code_blob(DEFAULT_LAYOUT.code_address().get());

    let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x900000);
    for (offset, byte) in blob.bytes().iter().enumerate() {
        let addr = GuestAddr::new(blob.load_address().get().wrapping_add(offset as u32));
        memory.write8(addr, *byte).unwrap();
    }

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
        .unwrap();

    let mut cpu = Cpu::new(memory);
    cpu.regs_mut().set_pc(blob.entry().get());
    cpu.regs_mut().set_execution_mode(blob.mode());
    cpu.regs_mut().set_sp(0x280000);
    cpu.regs_mut().set_reg(0, 1);
    (
        cpu,
        ExtHost::new(
            GuestAddr::new(0x181000),
            GuestAddr::new(0x182000),
            GuestAddr::new(0x181100),
            GuestAddr::new(0x181140),
            GuestAddr::new(0x181180),
            GuestAddr::new(0x181200),
            GuestAddr::new(0x181300),
            GuestAddr::new(0x183000),
            0x10000,
        ),
    )
}

fn step_n(cpu: &mut Cpu<TestMemory>, host: &mut ExtHost, steps: usize) {
    for _ in 0..steps {
        if !host.handle(cpu).unwrap() {
            cpu.step().unwrap();
        }
    }
}

#[test]
fn bootstrap_makes_real_ext_mr_table_lookup_reachable() {
    let (mut cpu, mut host) = new_bootstrapped_real_ext_cpu();
    step_n(&mut cpu, &mut host, 6);

    assert_eq!(cpu.regs().reg(1), 0x180000);
    assert_eq!(cpu.regs().reg(2), 0x181000);
    assert_eq!(cpu.regs().pc(), 0x80020);
    assert_eq!(cpu.regs().execution_mode(), ExecutionMode::Arm);
}

#[test]
fn bootstrap_stub_allows_blx_call_and_return() {
    let (mut cpu, mut host) = new_bootstrapped_real_ext_cpu();
    step_n(&mut cpu, &mut host, 13);

    assert_ne!(cpu.regs().reg(0), 0);
    assert_eq!(cpu.regs().pc(), 0x80038);
    assert_eq!(cpu.regs().execution_mode(), ExecutionMode::Arm);
    let helper = host.ext_helper_addr().unwrap();
    assert!(helper.get() >= 0x80000);
}

#[test]
fn bootstrap_seeds_c_function_context_for_ext_type_store() {
    let (mut cpu, mut host) = new_bootstrapped_real_ext_cpu();
    step_n(&mut cpu, &mut host, 18);

    assert_eq!(
        cpu.memory().read32(GuestAddr::new(0x80004)).unwrap(),
        0x182000
    );
    assert_eq!(cpu.memory().read32(GuestAddr::new(0x182008)).unwrap(), 1);
    assert_eq!(cpu.regs().pc(), 0x8004C);
}

#[test]
fn bootstrap_seeds_mr_malloc_stub_for_indirect_call() {
    let (mut cpu, mut host) = new_bootstrapped_real_ext_cpu();
    step_n(&mut cpu, &mut host, 44);

    assert_eq!(
        cpu.memory().read32(GuestAddr::new(0x180000)).unwrap(),
        0x181100
    );
    assert_ne!(cpu.regs().reg(0), 0);
    assert_eq!(cpu.regs().pc(), 0x80840);
}

#[test]
fn bootstrap_maps_memory_manager_data_slots_to_guest_cells() {
    let (cpu, _host) = new_bootstrapped_real_ext_cpu();
    let memory = cpu.memory();
    let manager_base = DEFAULT_LAYOUT.memory_manager_address().get();
    let manager_len = DEFAULT_LAYOUT.memory_manager_size();
    let manager_end = manager_base.wrapping_add(manager_len);

    let base_cell = memory.read32(GuestAddr::new(0x1801B0)).unwrap();
    let len_cell = memory.read32(GuestAddr::new(0x1801B4)).unwrap();
    let end_cell = memory.read32(GuestAddr::new(0x1801B8)).unwrap();
    let left_cell = memory.read32(GuestAddr::new(0x1801BC)).unwrap();
    let min_cell = memory.read32(GuestAddr::new(0x18021C)).unwrap();
    let top_cell = memory.read32(GuestAddr::new(0x180220)).unwrap();

    for cell in [base_cell, len_cell, end_cell, left_cell, min_cell, top_cell] {
        assert_ne!(cell, 0);
    }

    assert_eq!(
        memory.read32(GuestAddr::new(base_cell)).unwrap(),
        manager_base
    );
    assert_eq!(
        memory.read32(GuestAddr::new(len_cell)).unwrap(),
        manager_len
    );
    assert_eq!(
        memory.read32(GuestAddr::new(end_cell)).unwrap(),
        manager_end
    );
    assert_eq!(
        memory.read32(GuestAddr::new(left_cell)).unwrap(),
        manager_len
    );
    assert_eq!(
        memory.read32(GuestAddr::new(min_cell)).unwrap(),
        manager_len
    );
    assert_eq!(memory.read32(GuestAddr::new(top_cell)).unwrap(), 0);
}

#[test]
fn bootstrap_maps_internal_and_port_tables_to_guest_data() {
    let (cpu, _host) = new_bootstrapped_real_ext_cpu();
    let memory = cpu.memory();

    let internal_table = memory.read32(GuestAddr::new(0x18005C)).unwrap();
    let port_table = memory.read32(GuestAddr::new(0x180060)).unwrap();

    assert_ne!(internal_table, 0);
    assert_ne!(port_table, 0);
    assert_ne!(internal_table, 0x181140);
    assert_ne!(port_table, 0x181140);

    let timer_p_cell = memory
        .read32(GuestAddr::new(internal_table.wrapping_add(0x10)))
        .unwrap();
    let timer_state_cell = memory
        .read32(GuestAddr::new(internal_table.wrapping_add(0x14)))
        .unwrap();
    let timer_run_cell = memory
        .read32(GuestAddr::new(internal_table.wrapping_add(0x18)))
        .unwrap();

    assert_ne!(timer_p_cell, 0);
    assert_ne!(timer_state_cell, 0);
    assert_ne!(timer_run_cell, 0);
    assert_eq!(memory.read32(GuestAddr::new(timer_p_cell)).unwrap(), 0);
    assert_eq!(memory.read32(GuestAddr::new(timer_state_cell)).unwrap(), 0);
    assert_eq!(memory.read32(GuestAddr::new(timer_run_cell)).unwrap(), 0);
    assert_eq!(memory.read32(GuestAddr::new(port_table)).unwrap(), 0);
}

#[test]
fn host_updates_memory_manager_cells_after_mr_malloc_and_free() {
    let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x900000);
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
        .unwrap();

    let mut host = ExtHost::new(
        GuestAddr::new(0x181000),
        GuestAddr::new(0x182000),
        GuestAddr::new(0x181100),
        GuestAddr::new(0x181140),
        GuestAddr::new(0x181180),
        GuestAddr::new(0x181200),
        GuestAddr::new(0x181300),
        GuestAddr::new(0x183000),
        0x10000,
    );
    host.set_mr_table_addr(GuestAddr::new(0x180000));

    let left_cell = memory.read32(GuestAddr::new(0x1801BC)).unwrap();
    let min_cell = memory.read32(GuestAddr::new(0x18021C)).unwrap();
    let top_cell = memory.read32(GuestAddr::new(0x180220)).unwrap();

    let mut cpu = Cpu::new(memory);
    cpu.regs_mut().set_pc(0x181100);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_reg(0, 32);

    assert!(host.handle(&mut cpu).unwrap());

    let initial_left = DEFAULT_LAYOUT.memory_manager_size();
    let alloc_len = 40;
    assert_eq!(
        cpu.memory().read32(GuestAddr::new(left_cell)).unwrap(),
        initial_left - alloc_len
    );
    assert_eq!(
        cpu.memory().read32(GuestAddr::new(min_cell)).unwrap(),
        initial_left - alloc_len
    );
    assert_eq!(
        cpu.memory().read32(GuestAddr::new(top_cell)).unwrap(),
        alloc_len
    );

    let ptr = cpu.regs().reg(0);
    cpu.regs_mut().set_pc(0x181140);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80004);
    cpu.regs_mut().set_reg(0, ptr);
    cpu.regs_mut().set_reg(1, 32);

    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(
        cpu.memory().read32(GuestAddr::new(left_cell)).unwrap(),
        initial_left
    );
    assert_eq!(
        cpu.memory().read32(GuestAddr::new(min_cell)).unwrap(),
        initial_left - alloc_len
    );
    assert_eq!(
        cpu.memory().read32(GuestAddr::new(top_cell)).unwrap(),
        alloc_len
    );
}

#[test]
fn bootstrap_keeps_safe_stub_for_other_unresolved_function_slots() {
    let (cpu, _host) = new_bootstrapped_real_ext_cpu();

    assert_eq!(
        cpu.memory().read32(GuestAddr::new(0x180144)).unwrap(),
        0x181140
    );
}

#[test]
fn host_handles_mr_get_screen_info_stub() {
    let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x900000);
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
        .unwrap();

    let mut host = ExtHost::new(
        GuestAddr::new(0x181000),
        GuestAddr::new(0x182000),
        GuestAddr::new(0x181100),
        GuestAddr::new(0x181140),
        GuestAddr::new(0x181180),
        GuestAddr::new(0x181200),
        GuestAddr::new(0x181300),
        GuestAddr::new(0x183000),
        0x10000,
    );

    let mut cpu = Cpu::new(memory);
    let screen_info_pc = cpu.memory().read32(GuestAddr::new(0x180140)).unwrap();
    cpu.regs_mut().set_pc(screen_info_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_reg(0, 0x183500);

    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(cpu.memory().read32(GuestAddr::new(0x183500)).unwrap(), 240);
    assert_eq!(cpu.memory().read32(GuestAddr::new(0x183504)).unwrap(), 320);
    assert_eq!(cpu.memory().read32(GuestAddr::new(0x183508)).unwrap(), 16);
}

#[test]
fn host_handles_memcpy_stub() {
    let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x900000);
    let mut host = ExtHost::new(
        GuestAddr::new(0x181000),
        GuestAddr::new(0x182000),
        GuestAddr::new(0x181100),
        GuestAddr::new(0x181140),
        GuestAddr::new(0x181180),
        GuestAddr::new(0x181200),
        GuestAddr::new(0x181300),
        GuestAddr::new(0x183000),
        0x10000,
    );
    memory.write8(GuestAddr::new(0x183100), 0x11).unwrap();
    memory.write8(GuestAddr::new(0x183101), 0x22).unwrap();
    memory.write8(GuestAddr::new(0x183102), 0x33).unwrap();
    memory.write8(GuestAddr::new(0x183103), 0x44).unwrap();

    let mut cpu = Cpu::new(memory);
    cpu.regs_mut().set_pc(0x181200);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x800D0);
    cpu.regs_mut().set_reg(0, 0x183200);
    cpu.regs_mut().set_reg(1, 0x183100);
    cpu.regs_mut().set_reg(2, 4);

    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().pc(), 0x800D0);
    assert_eq!(cpu.regs().reg(0), 0x183200);
    assert_eq!(
        cpu.memory().read32(GuestAddr::new(0x183200)).unwrap(),
        0x44332211
    );
}

#[test]
fn host_handles_memset_stub() {
    let memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x900000);
    let mut host = ExtHost::new(
        GuestAddr::new(0x181000),
        GuestAddr::new(0x182000),
        GuestAddr::new(0x181100),
        GuestAddr::new(0x181140),
        GuestAddr::new(0x181180),
        GuestAddr::new(0x181200),
        GuestAddr::new(0x181300),
        GuestAddr::new(0x183000),
        0x10000,
    );

    let mut cpu = Cpu::new(memory);
    cpu.regs_mut().set_pc(0x181300);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80104);
    cpu.regs_mut().set_reg(0, 0x183400);
    cpu.regs_mut().set_reg(1, 0x7F);
    cpu.regs_mut().set_reg(2, 4);

    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().pc(), 0x80104);
    assert_eq!(cpu.regs().reg(0), 0x183400);
    assert_eq!(
        cpu.memory().read32(GuestAddr::new(0x183400)).unwrap(),
        0x7F7F7F7F
    );
}

#[test]
fn host_handles_mr_c_function_new_stub() {
    let memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x900000);
    let mut host = ExtHost::new(
        GuestAddr::new(0x181000),
        GuestAddr::new(0x182000),
        GuestAddr::new(0x181100),
        GuestAddr::new(0x181140),
        GuestAddr::new(0x181180),
        GuestAddr::new(0x181200),
        GuestAddr::new(0x181300),
        GuestAddr::new(0x183000),
        0x10000,
    );

    let mut cpu = Cpu::new(memory);
    cpu.regs_mut().set_pc(0x181000);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80034);
    cpu.regs_mut().set_reg(0, 0x800EC);
    cpu.regs_mut().set_reg(1, 0x20);

    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().pc(), 0x80034);
    assert_eq!(cpu.regs().reg(0), 0x182000);
    let helper = host.ext_helper_addr().unwrap();
    assert!(helper.get() >= 0x80000);
}
