use std::io::{Read, Write};
use std::net::{TcpListener, UdpSocket};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use std::thread;
use std::fs;

use vmrp_abi::ExtFile;
use vmrp_core::{GuestAddr, DEFAULT_LAYOUT};
use vmrp_cpu::{Cpu, ExecutionMode, MemoryBus, TestMemory};
use vmrp_platform::{
    ExtBootstrap, ExtHost, HostAppEvent, HostScreenRegion, HostTimerCommand,
    MR_EXT_FUNCTION_NEW_ADDR, MR_FAILED, SEND_APP_EVENT_ADDR,
};

fn real_ext_path() -> PathBuf {
    PathBuf::from(r"D:\opt\rust\vmrp\mrc\asm\cfunction.ext")
}

fn sky16_font_path() -> PathBuf {
    PathBuf::from(r"D:\opt\rust\vmrp\wasm\dist\fs\mythroad\system\gb16.uc2")
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

fn read_c_string(memory: &TestMemory, addr: u32) -> String {
    let mut bytes = Vec::new();
    let mut cursor = addr;
    loop {
        let byte = memory.read8(GuestAddr::new(cursor)).unwrap();
        if byte == 0 {
            break;
        }
        bytes.push(byte);
        cursor = cursor.wrapping_add(1);
    }
    String::from_utf8(bytes).unwrap()
}

fn write_c_string(memory: &mut TestMemory, addr: u32, value: &str) {
    for (index, byte) in value.as_bytes().iter().enumerate() {
        memory
            .write8(GuestAddr::new(addr.wrapping_add(index as u32)), *byte)
            .unwrap();
    }
    memory
        .write8(GuestAddr::new(addr.wrapping_add(value.len() as u32)), 0)
        .unwrap();
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

    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(cpu.regs().pc(), 0x80038);
    assert_eq!(cpu.regs().execution_mode(), ExecutionMode::Arm);
    let helper = host.ext_helper_addr().unwrap();
    assert!(helper.get() >= 0x80000);
}

#[test]
fn bootstrap_seeds_c_function_context_for_ext_type_store() {
    let (mut cpu, mut host) = new_bootstrapped_real_ext_cpu();
    step_n(&mut cpu, &mut host, 18);

    let context_addr = cpu.memory().read32(GuestAddr::new(0x80004)).unwrap();
    assert!(context_addr >= DEFAULT_LAYOUT.memory_manager_address().get());
    assert_eq!(cpu.memory().read32(GuestAddr::new(context_addr + 8)).unwrap(), 1);
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
    let allocated = cpu.regs().reg(0);
    assert!(allocated >= DEFAULT_LAYOUT.memory_manager_address().get());
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
fn bootstrap_does_not_point_unimplemented_slots_at_mr_free() {
    let (cpu, _host) = new_bootstrapped_real_ext_cpu();
    let memory = cpu.memory();

    let internal_table = memory.read32(GuestAddr::new(0x18005C)).unwrap();
    let unresolved_internal = memory
        .read32(GuestAddr::new(internal_table.wrapping_add(12 * 4)))
        .unwrap();
    let unresolved_mr_table = memory.read32(GuestAddr::new(0x180124)).unwrap();

    assert_ne!(unresolved_internal, 0x181140);
    assert_ne!(unresolved_mr_table, 0x181140);
}

#[test]
fn bootstrap_maps_legacy_file_slots_to_host_stubs() {
    let (cpu, _host) = new_bootstrapped_real_ext_cpu();
    let memory = cpu.memory();

    assert_eq!(memory.read32(GuestAddr::new(0x1800A0)).unwrap(), 0x181EC0);
    assert_eq!(memory.read32(GuestAddr::new(0x1800A4)).unwrap(), 0x181ED0);
    assert_eq!(memory.read32(GuestAddr::new(0x1800A8)).unwrap(), 0x181EE0);
    assert_eq!(memory.read32(GuestAddr::new(0x1800AC)).unwrap(), 0x181EF0);
    assert_eq!(memory.read32(GuestAddr::new(0x1800B0)).unwrap(), 0x181F00);
    assert_eq!(memory.read32(GuestAddr::new(0x1800B4)).unwrap(), 0x181F10);
    assert_eq!(memory.read32(GuestAddr::new(0x1800B8)).unwrap(), 0x181F20);
}

#[test]
fn bootstrap_maps_legacy_ui_and_sound_slots_to_host_stubs() {
    let (cpu, _host) = new_bootstrapped_real_ext_cpu();
    let memory = cpu.memory();

    assert_eq!(memory.read32(GuestAddr::new(0x1800D8)).unwrap(), 0x181590);
    assert_eq!(memory.read32(GuestAddr::new(0x1800EC)).unwrap(), 0x1815A0);
    assert_eq!(memory.read32(GuestAddr::new(0x1800F0)).unwrap(), 0x1818D0);
    assert_eq!(memory.read32(GuestAddr::new(0x1800F4)).unwrap(), 0x1818E0);
    assert_eq!(memory.read32(GuestAddr::new(0x1800F8)).unwrap(), 0x1818F0);
    assert_eq!(memory.read32(GuestAddr::new(0x1800FC)).unwrap(), 0x181C00);
    assert_eq!(memory.read32(GuestAddr::new(0x180100)).unwrap(), 0x181C10);
    assert_eq!(memory.read32(GuestAddr::new(0x180104)).unwrap(), 0x181C20);
    assert_eq!(memory.read32(GuestAddr::new(0x18010C)).unwrap(), 0x181C30);
    assert_eq!(memory.read32(GuestAddr::new(0x180110)).unwrap(), 0x181C40);
    assert_eq!(memory.read32(GuestAddr::new(0x1800DC)).unwrap(), 0x1815C0);
    assert_eq!(memory.read32(GuestAddr::new(0x1800E0)).unwrap(), 0x181600);
    assert_eq!(memory.read32(GuestAddr::new(0x1800E4)).unwrap(), 0x181640);
    assert_eq!(memory.read32(GuestAddr::new(0x1800E8)).unwrap(), 0x181680);
    assert_eq!(memory.read32(GuestAddr::new(0x180114)).unwrap(), 0x1816C0);
    assert_eq!(memory.read32(GuestAddr::new(0x180128)).unwrap(), 0x181800);
    assert_eq!(memory.read32(GuestAddr::new(0x18012C)).unwrap(), 0x181840);
    assert_eq!(memory.read32(GuestAddr::new(0x180130)).unwrap(), 0x181880);
    assert_eq!(memory.read32(GuestAddr::new(0x180134)).unwrap(), 0x1818C0);
    assert_eq!(memory.read32(GuestAddr::new(0x180138)).unwrap(), 0x181C50);
    assert_eq!(memory.read32(GuestAddr::new(0x18013C)).unwrap(), 0x181C60);
    assert_eq!(memory.read32(GuestAddr::new(0x1801D8)).unwrap(), 0x181CC0);
    assert_eq!(memory.read32(GuestAddr::new(0x1801DC)).unwrap(), 0x181D00);
    assert_eq!(memory.read32(GuestAddr::new(0x1801E8)).unwrap(), 0x181D80);
}

#[test]
fn bootstrap_maps_legacy_network_slots_to_host_stubs() {
    let (cpu, _host) = new_bootstrapped_real_ext_cpu();
    let memory = cpu.memory();

    assert_eq!(memory.read32(GuestAddr::new(0x180140)).unwrap(), 0x181340);
    assert_eq!(memory.read32(GuestAddr::new(0x180144)).unwrap(), 0x181940);
    assert_eq!(memory.read32(GuestAddr::new(0x180148)).unwrap(), 0x181980);
    assert_eq!(memory.read32(GuestAddr::new(0x18014C)).unwrap(), 0x181900);
    assert_eq!(memory.read32(GuestAddr::new(0x180150)).unwrap(), 0x1819C0);
    assert_eq!(memory.read32(GuestAddr::new(0x180154)).unwrap(), 0x181A00);
    assert_eq!(memory.read32(GuestAddr::new(0x180158)).unwrap(), 0x181A80);
    assert_eq!(memory.read32(GuestAddr::new(0x18015C)).unwrap(), 0x181AC0);
    assert_eq!(memory.read32(GuestAddr::new(0x180160)).unwrap(), 0x181B40);
    assert_eq!(memory.read32(GuestAddr::new(0x180164)).unwrap(), 0x181B00);
    assert_eq!(memory.read32(GuestAddr::new(0x180168)).unwrap(), 0x181B80);
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
fn bootstrap_seeds_mr_read_file_slot() {
    let (cpu, _host) = new_bootstrapped_real_ext_cpu();

    assert_eq!(
        cpu.memory().read32(GuestAddr::new(0x1801F4)).unwrap(),
        0x181400
    );
}

#[test]
fn bootstrap_maps_legacy_runtime_data_slots() {
    let (cpu, _host) = new_bootstrapped_real_ext_cpu();
    let memory = cpu.memory();

    let screen_buf_cell = memory.read32(GuestAddr::new(0x18016C)).unwrap();
    let screen_w_cell = memory.read32(GuestAddr::new(0x180170)).unwrap();
    let screen_h_cell = memory.read32(GuestAddr::new(0x180174)).unwrap();
    let screen_bit_cell = memory.read32(GuestAddr::new(0x180178)).unwrap();
    let pack_filename = memory.read32(GuestAddr::new(0x180190)).unwrap();
    let start_filename = memory.read32(GuestAddr::new(0x180194)).unwrap();
    let old_pack_filename = memory.read32(GuestAddr::new(0x180198)).unwrap();
    let old_start_filename = memory.read32(GuestAddr::new(0x18019C)).unwrap();
    let ram_file_cell = memory.read32(GuestAddr::new(0x1801A0)).unwrap();
    let ram_file_len_cell = memory.read32(GuestAddr::new(0x1801A4)).unwrap();
    let start_fileparameter = memory.read32(GuestAddr::new(0x180228)).unwrap();
    let mr_entry = memory.read32(GuestAddr::new(0x180240)).unwrap();

    for addr in [
        screen_buf_cell,
        screen_w_cell,
        screen_h_cell,
        screen_bit_cell,
        pack_filename,
        start_filename,
        old_pack_filename,
        old_start_filename,
        ram_file_cell,
        ram_file_len_cell,
        start_fileparameter,
        mr_entry,
    ] {
        assert_ne!(addr, 0);
        assert_ne!(addr, 0x181140);
    }

    assert_eq!(memory.read32(GuestAddr::new(screen_buf_cell)).unwrap(), 0);
    assert_eq!(memory.read32(GuestAddr::new(screen_w_cell)).unwrap(), 0);
    assert_eq!(memory.read32(GuestAddr::new(screen_h_cell)).unwrap(), 0);
    assert_eq!(memory.read32(GuestAddr::new(screen_bit_cell)).unwrap(), 0);
    assert_eq!(memory.read32(GuestAddr::new(ram_file_cell)).unwrap(), 0);
    assert_eq!(memory.read32(GuestAddr::new(ram_file_len_cell)).unwrap(), 0);
    assert_eq!(read_c_string(memory, pack_filename), "");
    assert_eq!(read_c_string(memory, start_filename), "");
    assert_eq!(read_c_string(memory, old_pack_filename), "");
    assert_eq!(read_c_string(memory, old_start_filename), "");
    assert_eq!(read_c_string(memory, start_fileparameter), "");
    assert_eq!(read_c_string(memory, mr_entry), "");
}

#[test]
fn bootstrap_maps_legacy_media_runtime_slots() {
    let (cpu, _host) = new_bootstrapped_real_ext_cpu();
    let memory = cpu.memory();

    let bitmap_addr = memory.read32(GuestAddr::new(0x18017C)).unwrap();
    let tile_addr = memory.read32(GuestAddr::new(0x180180)).unwrap();
    let map_addr = memory.read32(GuestAddr::new(0x180184)).unwrap();
    let sound_addr = memory.read32(GuestAddr::new(0x180188)).unwrap();
    let sprite_addr = memory.read32(GuestAddr::new(0x18018C)).unwrap();

    for addr in [bitmap_addr, tile_addr, map_addr, sound_addr, sprite_addr] {
        assert_ne!(addr, 0);
        assert_ne!(addr, 0x181140);
    }

    for offset in (0..((31 * 16) as u32)).step_by(4) {
        assert_eq!(
            memory.read32(GuestAddr::new(bitmap_addr.wrapping_add(offset))).unwrap(),
            0
        );
    }
    for offset in (0..((3 * 20) as u32)).step_by(4) {
        assert_eq!(
            memory.read32(GuestAddr::new(tile_addr.wrapping_add(offset))).unwrap(),
            0
        );
    }
    for offset in (0..((3 * 4) as u32)).step_by(4) {
        assert_eq!(
            memory.read32(GuestAddr::new(map_addr.wrapping_add(offset))).unwrap(),
            0
        );
    }
    for offset in (0..((5 * 12) as u32)).step_by(4) {
        assert_eq!(
            memory.read32(GuestAddr::new(sound_addr.wrapping_add(offset))).unwrap(),
            0
        );
    }
    for offset in (0..((10 * 4) as u32)).step_by(4) {
        assert_eq!(
            memory.read32(GuestAddr::new(sprite_addr.wrapping_add(offset))).unwrap(),
            0
        );
    }
}

#[test]
fn bootstrap_keeps_safe_stub_for_other_unresolved_function_slots() {
    let (cpu, _host) = new_bootstrapped_real_ext_cpu();

    assert_eq!(
        cpu.memory().read32(GuestAddr::new(0x180108)).unwrap(),
        0x181580
    );
}

#[test]
fn host_reads_registered_package_file_from_mrp_entries() {
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
    host.register_package_file("start.mr", vec![0x4D, 0x52, 0x50, 0x21]);

    for (index, byte) in b"start.mr\0".iter().enumerate() {
        memory
            .write8(GuestAddr::new(0x183800 + index as u32), *byte)
            .unwrap();
    }

    let mut cpu = Cpu::new(memory);
    cpu.regs_mut().set_pc(0x181400);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_reg(0, 0x183800);
    cpu.regs_mut().set_reg(1, 0x183900);

    assert!(host.handle(&mut cpu).unwrap());
    let data_ptr = cpu.regs().reg(0);
    assert_ne!(data_ptr, 0);
    assert_eq!(cpu.regs().pc(), 0x80000);
    assert_eq!(cpu.memory().read32(GuestAddr::new(0x183900)).unwrap(), 4);
    assert_eq!(cpu.memory().read8(GuestAddr::new(data_ptr)).unwrap(), 0x4D);
    assert_eq!(
        cpu.memory().read8(GuestAddr::new(data_ptr + 1)).unwrap(),
        0x52
    );
    assert_eq!(
        cpu.memory().read8(GuestAddr::new(data_ptr + 2)).unwrap(),
        0x50
    );
    assert_eq!(
        cpu.memory().read8(GuestAddr::new(data_ptr + 3)).unwrap(),
        0x21
    );
}

#[test]
fn host_mr_test_com1_updates_legacy_runtime_slots() {
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

    for (index, byte) in b"child.mrp\0".iter().enumerate() {
        memory
            .write8(GuestAddr::new(0x183A00 + index as u32), *byte)
            .unwrap();
    }
    for (index, byte) in b"arg=1\0".iter().enumerate() {
        memory
            .write8(GuestAddr::new(0x183B00 + index as u32), *byte)
            .unwrap();
    }

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
    cpu.regs_mut().set_pc(0x1813C0);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_reg(0, 0x190000);
    cpu.regs_mut().set_reg(1, 2);
    cpu.regs_mut().set_reg(2, 0x183900);
    cpu.regs_mut().set_reg(3, 12);
    assert!(host.handle(&mut cpu).unwrap());

    let ram_file_cell = cpu.memory().read32(GuestAddr::new(0x1801A0)).unwrap();
    let ram_file_len_cell = cpu.memory().read32(GuestAddr::new(0x1801A4)).unwrap();
    assert_eq!(
        cpu.memory().read32(GuestAddr::new(ram_file_cell)).unwrap(),
        0x183900
    );
    assert_eq!(
        cpu.memory()
            .read32(GuestAddr::new(ram_file_len_cell))
            .unwrap(),
        12
    );

    cpu.regs_mut().set_pc(0x1813C0);
    cpu.regs_mut().set_lr(0x80004);
    cpu.regs_mut().set_reg(1, 3);
    cpu.regs_mut().set_reg(2, 0x183A00);
    cpu.regs_mut().set_reg(3, 9);
    assert!(host.handle(&mut cpu).unwrap());

    let old_pack_filename = cpu.memory().read32(GuestAddr::new(0x180198)).unwrap();
    let old_start_filename = cpu.memory().read32(GuestAddr::new(0x18019C)).unwrap();
    assert_eq!(read_c_string(cpu.memory(), old_pack_filename), "child.mrp");
    assert_eq!(read_c_string(cpu.memory(), old_start_filename), "start.mr");

    cpu.regs_mut().set_pc(0x1813C0);
    cpu.regs_mut().set_lr(0x80008);
    cpu.regs_mut().set_reg(1, 4);
    cpu.regs_mut().set_reg(2, 0x183B00);
    cpu.regs_mut().set_reg(3, 5);
    assert!(host.handle(&mut cpu).unwrap());

    let start_fileparameter = cpu.memory().read32(GuestAddr::new(0x180228)).unwrap();
    assert_eq!(read_c_string(cpu.memory(), start_fileparameter), "arg=1");
}

#[test]
fn host_mr_test_com_updates_bi_flags() {
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
    let bi_cell = cpu.memory().read32(GuestAddr::new(0x18034C)).unwrap();

    cpu.regs_mut().set_pc(0x181380);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_reg(0, 0);
    cpu.regs_mut().set_reg(1, 3629);
    cpu.regs_mut().set_reg(2, 2913);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.memory().read32(GuestAddr::new(bi_cell)).unwrap(), 1);

    cpu.regs_mut().set_pc(0x181380);
    cpu.regs_mut().set_lr(0x80004);
    cpu.regs_mut().set_reg(0, 0);
    cpu.regs_mut().set_reg(1, 3921);
    cpu.regs_mut().set_reg(2, 98352);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.memory().read32(GuestAddr::new(bi_cell)).unwrap(), 3);

    cpu.regs_mut().set_pc(0x181380);
    cpu.regs_mut().set_lr(0x80008);
    cpu.regs_mut().set_reg(0, 0);
    cpu.regs_mut().set_reg(1, 3251);
    cpu.regs_mut().set_reg(2, 648826);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.memory().read32(GuestAddr::new(bi_cell)).unwrap(), 1);
}

#[test]
fn host_legacy_file_api_can_read_registered_package_entries() {
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
    host.register_package_file("folder.bmp", b"BMP!".to_vec());

    write_c_string(&mut memory, 0x183800, "folder.bmp");

    let mut cpu = Cpu::new(memory);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);

    cpu.regs_mut().set_pc(0x181EE0);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_reg(0, 0x183800);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0) as i32, 1);

    cpu.regs_mut().set_pc(0x181F20);
    cpu.regs_mut().set_lr(0x80004);
    cpu.regs_mut().set_reg(0, 0x183800);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 4);

    cpu.regs_mut().set_pc(0x181EC0);
    cpu.regs_mut().set_lr(0x80008);
    cpu.regs_mut().set_reg(0, 0x183800);
    cpu.regs_mut().set_reg(1, 1);
    assert!(host.handle(&mut cpu).unwrap());
    let fd = cpu.regs().reg(0) as i32;
    assert!(fd > 0);

    cpu.regs_mut().set_pc(0x181F00);
    cpu.regs_mut().set_lr(0x8000C);
    cpu.regs_mut().set_reg(0, fd as u32);
    cpu.regs_mut().set_reg(1, 0x183840);
    cpu.regs_mut().set_reg(2, 2);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 2);
    assert_eq!(cpu.memory().read8(GuestAddr::new(0x183840)).unwrap(), b'B');
    assert_eq!(cpu.memory().read8(GuestAddr::new(0x183841)).unwrap(), b'M');

    cpu.regs_mut().set_pc(0x181F10);
    cpu.regs_mut().set_lr(0x80010);
    cpu.regs_mut().set_reg(0, fd as u32);
    cpu.regs_mut().set_reg(1, 0);
    cpu.regs_mut().set_reg(2, 0);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);

    cpu.regs_mut().set_pc(0x181F00);
    cpu.regs_mut().set_lr(0x80014);
    cpu.regs_mut().set_reg(0, fd as u32);
    cpu.regs_mut().set_reg(1, 0x183844);
    cpu.regs_mut().set_reg(2, 4);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 4);
    assert_eq!(cpu.memory().read8(GuestAddr::new(0x183844)).unwrap(), b'B');
    assert_eq!(cpu.memory().read8(GuestAddr::new(0x183845)).unwrap(), b'M');
    assert_eq!(cpu.memory().read8(GuestAddr::new(0x183846)).unwrap(), b'P');
    assert_eq!(cpu.memory().read8(GuestAddr::new(0x183847)).unwrap(), b'!');

    cpu.regs_mut().set_pc(0x181ED0);
    cpu.regs_mut().set_lr(0x80018);
    cpu.regs_mut().set_reg(0, fd as u32);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);
}

#[test]
fn host_legacy_seek_returns_success_for_nonzero_offsets() {
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
    host.register_package_file("folder.bmp", b"BMP!".to_vec());

    write_c_string(&mut memory, 0x183800, "folder.bmp");

    let mut cpu = Cpu::new(memory);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);

    cpu.regs_mut().set_pc(0x181EC0);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_reg(0, 0x183800);
    cpu.regs_mut().set_reg(1, 1);
    assert!(host.handle(&mut cpu).unwrap());
    let fd = cpu.regs().reg(0) as i32;
    assert!(fd > 0);

    cpu.regs_mut().set_pc(0x181F10);
    cpu.regs_mut().set_lr(0x80004);
    cpu.regs_mut().set_reg(0, fd as u32);
    cpu.regs_mut().set_reg(1, 2);
    cpu.regs_mut().set_reg(2, 0);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);

    cpu.regs_mut().set_pc(0x181F00);
    cpu.regs_mut().set_lr(0x80008);
    cpu.regs_mut().set_reg(0, fd as u32);
    cpu.regs_mut().set_reg(1, 0x183840);
    cpu.regs_mut().set_reg(2, 2);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 2);
    assert_eq!(cpu.memory().read8(GuestAddr::new(0x183840)).unwrap(), b'P');
    assert_eq!(cpu.memory().read8(GuestAddr::new(0x183841)).unwrap(), b'!');
}

#[test]
fn host_mr_test_com_updates_sms_return_slots() {
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
    let sms_return_flag_cell = cpu.memory().read32(GuestAddr::new(0x18022C)).unwrap();
    let sms_return_val_cell = cpu.memory().read32(GuestAddr::new(0x180230)).unwrap();
    assert_ne!(sms_return_flag_cell, 0);
    assert_ne!(sms_return_val_cell, 0);
    assert_eq!(
        cpu.memory()
            .read32(GuestAddr::new(sms_return_flag_cell))
            .unwrap(),
        0
    );
    assert_eq!(
        cpu.memory()
            .read32(GuestAddr::new(sms_return_val_cell))
            .unwrap(),
        0
    );

    cpu.regs_mut().set_pc(0x181380);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_reg(0, 0);
    cpu.regs_mut().set_reg(1, 306);
    cpu.regs_mut().set_reg(2, 0x1234);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(
        cpu.memory()
            .read32(GuestAddr::new(sms_return_flag_cell))
            .unwrap(),
        1
    );
    assert_eq!(
        cpu.memory()
            .read32(GuestAddr::new(sms_return_val_cell))
            .unwrap(),
        0x1234
    );

    cpu.regs_mut().set_pc(0x181380);
    cpu.regs_mut().set_lr(0x80004);
    cpu.regs_mut().set_reg(0, 0);
    cpu.regs_mut().set_reg(1, 307);
    cpu.regs_mut().set_reg(2, 0);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(
        cpu.memory()
            .read32(GuestAddr::new(sms_return_flag_cell))
            .unwrap(),
        0
    );
    assert_eq!(
        cpu.memory()
            .read32(GuestAddr::new(sms_return_val_cell))
            .unwrap(),
        0x1234
    );
}

#[test]
fn host_mr_test_com_sleep_delays_for_requested_duration() {
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
    cpu.regs_mut().set_pc(0x181380);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_reg(0, 0);
    cpu.regs_mut().set_reg(1, 400);
    cpu.regs_mut().set_reg(2, 20);

    let started_at = Instant::now();
    assert!(host.handle(&mut cpu).unwrap());
    let elapsed = started_at.elapsed();

    assert!(elapsed >= Duration::from_millis(10));
    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(cpu.regs().pc(), 0x80000);
}

#[test]
fn host_mr_test_com_updates_screen_width_slot() {
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
    let screen_w_cell = cpu.memory().read32(GuestAddr::new(0x180170)).unwrap();
    assert_ne!(screen_w_cell, 0);
    assert_eq!(cpu.memory().read32(GuestAddr::new(screen_w_cell)).unwrap(), 0);

    cpu.regs_mut().set_pc(0x181380);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_reg(0, 0);
    cpu.regs_mut().set_reg(1, 401);
    cpu.regs_mut().set_reg(2, 176);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(cpu.memory().read32(GuestAddr::new(screen_w_cell)).unwrap(), 176);

    cpu.regs_mut().set_pc(0x181380);
    cpu.regs_mut().set_lr(0x80004);
    cpu.regs_mut().set_reg(0, 0);
    cpu.regs_mut().set_reg(1, 401);
    cpu.regs_mut().set_reg(2, 240);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 176);
    assert_eq!(cpu.memory().read32(GuestAddr::new(screen_w_cell)).unwrap(), 240);
}

#[test]
fn host_mr_test_com_updates_screen_height_slot() {
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
    let screen_h_cell = cpu.memory().read32(GuestAddr::new(0x180174)).unwrap();
    assert_ne!(screen_h_cell, 0);
    assert_eq!(cpu.memory().read32(GuestAddr::new(screen_h_cell)).unwrap(), 0);

    cpu.regs_mut().set_pc(0x181380);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_reg(0, 0);
    cpu.regs_mut().set_reg(1, 406);
    cpu.regs_mut().set_reg(2, 208);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(cpu.memory().read32(GuestAddr::new(screen_h_cell)).unwrap(), 208);

    cpu.regs_mut().set_pc(0x181380);
    cpu.regs_mut().set_lr(0x80004);
    cpu.regs_mut().set_reg(0, 0);
    cpu.regs_mut().set_reg(1, 406);
    cpu.regs_mut().set_reg(2, 320);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 208);
    assert_eq!(cpu.memory().read32(GuestAddr::new(screen_h_cell)).unwrap(), 320);
}

#[test]
fn host_mr_test_com_updates_timer_run_without_pause_slot() {
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
    let internal_table = cpu.memory().read32(GuestAddr::new(0x18005C)).unwrap();
    let timer_run_cell = cpu
        .memory()
        .read32(GuestAddr::new(internal_table.wrapping_add(0x18)))
        .unwrap();
    assert_ne!(timer_run_cell, 0);
    assert_eq!(cpu.memory().read32(GuestAddr::new(timer_run_cell)).unwrap(), 0);

    cpu.regs_mut().set_pc(0x181380);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_reg(0, 0);
    cpu.regs_mut().set_reg(1, 407);
    cpu.regs_mut().set_reg(2, 1);
    assert!(host.handle(&mut cpu).unwrap());

    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(cpu.memory().read32(GuestAddr::new(timer_run_cell)).unwrap(), 1);
}

#[test]
fn host_mr_test_com_closes_legacy_network_state() {
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
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);

    cpu.regs_mut().set_pc(0x181940);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_reg(0, 0);
    cpu.regs_mut().set_reg(1, 0);
    cpu.regs_mut().set_reg(2, 0);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);

    cpu.regs_mut().set_pc(0x1819C0);
    cpu.regs_mut().set_lr(0x80004);
    cpu.regs_mut().set_reg(0, 0);
    cpu.regs_mut().set_reg(1, 0);
    cpu.regs_mut().set_reg(2, 0);
    assert!(host.handle(&mut cpu).unwrap());
    let handle = cpu.regs().reg(0);
    assert!(handle >= 3000);

    cpu.regs_mut().set_pc(0x181380);
    cpu.regs_mut().set_lr(0x80008);
    cpu.regs_mut().set_reg(0, 0);
    cpu.regs_mut().set_reg(1, 405);
    cpu.regs_mut().set_reg(2, 0);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);

    cpu.regs_mut().set_pc(0x181A40);
    cpu.regs_mut().set_lr(0x8000C);
    cpu.regs_mut().set_reg(0, handle);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0) as i32, MR_FAILED);
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
fn host_handles_legacy_string_stubs() {
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

    for (index, byte) in b"abc\0".iter().enumerate() {
        memory
            .write8(GuestAddr::new(0x183300 + index as u32), *byte)
            .unwrap();
    }
    for (index, byte) in b"abd\0".iter().enumerate() {
        memory
            .write8(GuestAddr::new(0x183500 + index as u32), *byte)
            .unwrap();
    }

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
    let strcpy_pc = cpu.memory().read32(GuestAddr::new(0x180014)).unwrap();
    cpu.regs_mut().set_pc(strcpy_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_reg(0, 0x183400);
    cpu.regs_mut().set_reg(1, 0x183300);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(read_c_string(cpu.memory(), 0x183400), "abc");

    let strlen_pc = cpu.memory().read32(GuestAddr::new(0x18003C)).unwrap();
    cpu.regs_mut().set_pc(strlen_pc);
    cpu.regs_mut().set_lr(0x80004);
    cpu.regs_mut().set_reg(0, 0x183400);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 3);

    let strcmp_pc = cpu.memory().read32(GuestAddr::new(0x180028)).unwrap();
    cpu.regs_mut().set_pc(strcmp_pc);
    cpu.regs_mut().set_lr(0x80008);
    cpu.regs_mut().set_reg(0, 0x183400);
    cpu.regs_mut().set_reg(1, 0x183500);
    assert!(host.handle(&mut cpu).unwrap());
    assert!((cpu.regs().reg(0) as i32) < 0);
}

#[test]
fn host_handles_sprintf_basic_format_patterns() {
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

    for (index, byte) in b"%s%s\0".iter().enumerate() {
        memory
            .write8(GuestAddr::new(0x183600 + index as u32), *byte)
            .unwrap();
    }
    for (index, byte) in b"foo\0".iter().enumerate() {
        memory
            .write8(GuestAddr::new(0x183700 + index as u32), *byte)
            .unwrap();
    }
    for (index, byte) in b"bar\0".iter().enumerate() {
        memory
            .write8(GuestAddr::new(0x183740 + index as u32), *byte)
            .unwrap();
    }
    for (index, byte) in b"%c:/%s\0".iter().enumerate() {
        memory
            .write8(GuestAddr::new(0x183780 + index as u32), *byte)
            .unwrap();
    }
    for (index, byte) in b"dir\0".iter().enumerate() {
        memory
            .write8(GuestAddr::new(0x1837C0 + index as u32), *byte)
            .unwrap();
    }

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
    let sprintf_pc = cpu.memory().read32(GuestAddr::new(0x180044)).unwrap();
    cpu.regs_mut().set_pc(sprintf_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_reg(0, 0x183800);
    cpu.regs_mut().set_reg(1, 0x183600);
    cpu.regs_mut().set_reg(2, 0x183700);
    cpu.regs_mut().set_reg(3, 0x183740);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(read_c_string(cpu.memory(), 0x183800), "foobar");
    assert_eq!(cpu.regs().reg(0), 6);

    cpu.regs_mut().set_pc(sprintf_pc);
    cpu.regs_mut().set_lr(0x80004);
    cpu.regs_mut().set_reg(0, 0x183840);
    cpu.regs_mut().set_reg(1, 0x183780);
    cpu.regs_mut().set_reg(2, 'a' as u32);
    cpu.regs_mut().set_reg(3, 0x1837C0);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(read_c_string(cpu.memory(), 0x183840), "a:/dir");
    assert_eq!(cpu.regs().reg(0), 6);
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
    assert_eq!(cpu.regs().reg(0), 0);
    let context_addr = cpu.memory().read32(GuestAddr::new(0x80004)).unwrap();
    assert!(context_addr >= DEFAULT_LAYOUT.memory_manager_address().get());
    let helper = host.ext_helper_addr().unwrap();
    assert!(helper.get() >= 0x80000);
}

#[test]
fn host_send_app_event_stub_emits_timer_commands() {
    let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x900000);
    memory.write32(GuestAddr::new(0x280000), 150).unwrap();

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
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80040);
    cpu.regs_mut().set_sp(0x280000);

    cpu.regs_mut().set_pc(SEND_APP_EVENT_ADDR.get());
    cpu.regs_mut().set_reg(0, 0);
    cpu.regs_mut().set_reg(1, 0x281000);
    cpu.regs_mut().set_reg(2, 0);
    cpu.regs_mut().set_reg(3, 0x282000);

    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().pc(), 0x80040);
    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(host.take_timer_command(), Some(HostTimerCommand::Start(150)));

    cpu.regs_mut().set_pc(SEND_APP_EVENT_ADDR.get());
    cpu.regs_mut().set_lr(0x80044);
    cpu.regs_mut().set_reg(2, 1);
    cpu.regs_mut().set_reg(3, 0x282000);

    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().pc(), 0x80044);
    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(host.take_timer_command(), Some(HostTimerCommand::Stop));
}

#[test]
fn host_send_app_event_stub_supports_plugin_internal_timer_commands() {
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
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_pc(SEND_APP_EVENT_ADDR.get());
    cpu.regs_mut().set_lr(0x80048);
    cpu.regs_mut().set_reg(0, 0);
    cpu.regs_mut().set_reg(1, 0);
    cpu.regs_mut().set_reg(2, 0x282000);
    cpu.regs_mut().set_reg(3, 150);

    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().pc(), 0x80048);
    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(host.take_timer_command(), Some(HostTimerCommand::Start(150)));

    cpu.regs_mut().set_pc(SEND_APP_EVENT_ADDR.get());
    cpu.regs_mut().set_lr(0x8004C);
    cpu.regs_mut().set_reg(0, 0);
    cpu.regs_mut().set_reg(1, 1);
    cpu.regs_mut().set_reg(2, 0x282000);
    cpu.regs_mut().set_reg(3, 0);

    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().pc(), 0x8004C);
    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(host.take_timer_command(), Some(HostTimerCommand::Stop));
}

#[test]
fn host_send_app_event_stub_emits_plugin_app_events() {
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
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_pc(SEND_APP_EVENT_ADDR.get());
    cpu.regs_mut().set_lr(0x80050);
    cpu.regs_mut().set_reg(0, 1);
    cpu.regs_mut().set_reg(1, 100);
    cpu.regs_mut().set_reg(2, 0);
    cpu.regs_mut().set_reg(3, 0);

    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().pc(), 0x80050);
    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(host.take_timer_command(), None);
    assert_eq!(
        host.take_app_event(),
        Some(HostAppEvent {
            code: 100,
            p0: 0,
            p1: 0,
        })
    );
    assert_eq!(host.take_app_event(), None);
}

#[test]
fn host_ext_function_new_stub_targets_active_plugin_context_and_helper() {
    let memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x900000);
    let code_base = GuestAddr::new(0x190000);
    let plugin_context = GuestAddr::new(0x280400);

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
    host.begin_plugin_ext_load(code_base, plugin_context);

    let mut cpu = Cpu::new(memory);
    cpu.regs_mut().set_pc(MR_EXT_FUNCTION_NEW_ADDR.get());
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80044);
    cpu.regs_mut().set_reg(0, 0x191234);
    cpu.regs_mut().set_reg(1, 0x14);

    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().pc(), 0x80044);
    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(
        cpu.memory().read32(GuestAddr::new(code_base.get().wrapping_add(4))).unwrap(),
        plugin_context.get()
    );
    assert_eq!(host.c_function_p_addr().get(), plugin_context.get());
    assert_eq!(host.ext_helper_addr().unwrap().get(), 0x191234);
}

#[test]
fn host_handles_legacy_ui_and_sound_stubs() {
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

    write_c_string(&mut memory, 0x183200, "seed");
    let mut cpu = Cpu::new(memory);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);

    for pc in [0x1815C0, 0x181600, 0x181640, 0x181680] {
        cpu.regs_mut().set_pc(pc);
        cpu.regs_mut().set_lr(0x80040);
        cpu.regs_mut().set_reg(0, 1);
        cpu.regs_mut().set_reg(1, 0x183200);
        cpu.regs_mut().set_reg(2, 4);
        cpu.regs_mut().set_reg(3, 0);
        assert!(host.handle(&mut cpu).unwrap());
        assert_eq!(cpu.regs().reg(0), 0);
        assert_eq!(cpu.regs().pc(), 0x80040);
    }

    cpu.regs_mut().set_pc(0x1816C0);
    cpu.regs_mut().set_lr(0x80044);
    cpu.regs_mut().set_reg(0, 0x183200);
    cpu.regs_mut().set_reg(1, 0x183200);
    cpu.regs_mut().set_reg(2, 0);
    cpu.regs_mut().set_reg(3, 0);
    assert!(host.handle(&mut cpu).unwrap());
    let dialog = cpu.regs().reg(0) as i32;
    assert!(dialog > 0);
    assert_eq!(cpu.regs().pc(), 0x80044);

    cpu.regs_mut().set_pc(0x181740);
    cpu.regs_mut().set_lr(0x80046);
    cpu.regs_mut().set_reg(0, dialog as u32);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(cpu.regs().pc(), 0x80046);

    cpu.regs_mut().set_pc(0x181700);
    cpu.regs_mut().set_lr(0x80048);
    cpu.regs_mut().set_reg(0, dialog as u32);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(cpu.regs().pc(), 0x80048);

    cpu.regs_mut().set_pc(0x181780);
    cpu.regs_mut().set_lr(0x8004A);
    cpu.regs_mut().set_reg(0, 0x183200);
    cpu.regs_mut().set_reg(1, 0x183200);
    cpu.regs_mut().set_reg(2, 0);
    cpu.regs_mut().set_reg(3, 0);
    assert!(host.handle(&mut cpu).unwrap());
    let text = cpu.regs().reg(0) as i32;
    assert!(text > 0);
    assert_eq!(cpu.regs().pc(), 0x8004A);

    cpu.regs_mut().set_pc(0x181800);
    cpu.regs_mut().set_lr(0x8004C);
    cpu.regs_mut().set_reg(0, text as u32);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(cpu.regs().pc(), 0x8004C);

    cpu.regs_mut().set_pc(0x1817C0);
    cpu.regs_mut().set_lr(0x8004E);
    cpu.regs_mut().set_reg(0, text as u32);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(cpu.regs().pc(), 0x8004E);

    cpu.regs_mut().set_pc(0x181840);
    cpu.regs_mut().set_lr(0x80050);
    cpu.regs_mut().set_reg(0, 0x183200);
    cpu.regs_mut().set_reg(1, 0x183200);
    cpu.regs_mut().set_reg(2, 0);
    cpu.regs_mut().set_reg(3, 16);
    assert!(host.handle(&mut cpu).unwrap());
    let edit = cpu.regs().reg(0) as i32;
    assert!(edit > 0);
    assert_eq!(cpu.regs().pc(), 0x80050);

    cpu.regs_mut().set_pc(0x1818C0);
    cpu.regs_mut().set_lr(0x80054);
    cpu.regs_mut().set_reg(0, edit as u32);
    assert!(host.handle(&mut cpu).unwrap());
    let text_ptr = cpu.regs().reg(0);
    assert_ne!(text_ptr, 0);
    assert_eq!(read_c_string(cpu.memory(), text_ptr), "seed");
    assert_eq!(cpu.regs().pc(), 0x80054);

    cpu.regs_mut().set_pc(0x181880);
    cpu.regs_mut().set_lr(0x80058);
    cpu.regs_mut().set_reg(0, edit as u32);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(cpu.regs().pc(), 0x80058);
}

#[test]
fn host_handles_legacy_draw_point_and_dispup_ex_stubs() {
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
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);

    cpu.regs_mut().set_pc(0x181D00);
    cpu.regs_mut().set_lr(0x80060);
    cpu.regs_mut().set_reg(0, 5);
    cpu.regs_mut().set_reg(1, 7);
    cpu.regs_mut().set_reg(2, 0x7BEF);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(cpu.regs().pc(), 0x80060);
    assert_eq!(host.screen_buffer()[7 * 240 + 5], 0x7BEF);
    assert_eq!(
        host.take_dirty_region(),
        Some(HostScreenRegion {
            x: 5,
            y: 7,
            w: 1,
            h: 1,
        })
    );

    cpu.regs_mut().set_pc(0x181CC0);
    cpu.regs_mut().set_lr(0x80064);
    cpu.regs_mut().set_reg(0, 3);
    cpu.regs_mut().set_reg(1, 4);
    cpu.regs_mut().set_reg(2, 20);
    cpu.regs_mut().set_reg(3, 30);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(cpu.regs().pc(), 0x80064);
    assert_eq!(
        host.take_dirty_region(),
        Some(HostScreenRegion {
            x: 3,
            y: 4,
            w: 20,
            h: 30,
        })
    );
}

#[test]
fn host_handles_legacy_draw_rect_stub() {
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
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_pc(0x181D80);
    cpu.regs_mut().set_lr(0x80068);
    cpu.regs_mut().set_sp(0x280000);
    cpu.regs_mut().set_reg(0, 2);
    cpu.regs_mut().set_reg(1, 3);
    cpu.regs_mut().set_reg(2, 4);
    cpu.regs_mut().set_reg(3, 2);
    cpu.memory_mut().write32(GuestAddr::new(0x280000), 255).unwrap();
    cpu.memory_mut().write32(GuestAddr::new(0x280004), 0).unwrap();
    cpu.memory_mut().write32(GuestAddr::new(0x280008), 0).unwrap();

    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(cpu.regs().pc(), 0x80068);
    for yy in 3..5usize {
        for xx in 2..6usize {
            assert_eq!(host.screen_buffer()[yy * 240 + xx], 0xF800);
        }
    }
    assert_eq!(
        host.take_dirty_region(),
        Some(HostScreenRegion {
            x: 2,
            y: 3,
            w: 4,
            h: 2,
        })
    );
}

#[test]
fn host_handles_legacy_draw_bitmap_ex_copy_stub() {
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
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);

    cpu.memory_mut().write16(GuestAddr::new(0x184000), 0x1111).unwrap();
    cpu.memory_mut().write16(GuestAddr::new(0x184002), 0x2222).unwrap();
    cpu.memory_mut().write16(GuestAddr::new(0x184004), 0x3333).unwrap();
    cpu.memory_mut().write16(GuestAddr::new(0x184006), 0x4444).unwrap();

    cpu.memory_mut().write32(GuestAddr::new(0x183200), 0x184000).unwrap();
    cpu.memory_mut().write16(GuestAddr::new(0x183204), 2).unwrap();
    cpu.memory_mut().write16(GuestAddr::new(0x183206), 2).unwrap();
    cpu.memory_mut().write16(GuestAddr::new(0x183208), 0).unwrap();
    cpu.memory_mut().write16(GuestAddr::new(0x18320A), 0).unwrap();

    cpu.memory_mut().write32(GuestAddr::new(0x183240), 0).unwrap();
    cpu.memory_mut().write16(GuestAddr::new(0x183244), 240).unwrap();
    cpu.memory_mut().write16(GuestAddr::new(0x183246), 320).unwrap();
    cpu.memory_mut().write16(GuestAddr::new(0x183248), 6).unwrap();
    cpu.memory_mut().write16(GuestAddr::new(0x18324A), 9).unwrap();

    cpu.memory_mut().write16(GuestAddr::new(0x183280), 0x0100).unwrap();
    cpu.memory_mut().write16(GuestAddr::new(0x183282), 0).unwrap();
    cpu.memory_mut().write16(GuestAddr::new(0x183284), 0).unwrap();
    cpu.memory_mut().write16(GuestAddr::new(0x183286), 0x0100).unwrap();
    cpu.memory_mut().write16(GuestAddr::new(0x183288), 2).unwrap();

    cpu.regs_mut().set_pc(0x181D40);
    cpu.regs_mut().set_lr(0x8006C);
    cpu.regs_mut().set_sp(0x280000);
    cpu.regs_mut().set_reg(0, 0x183200);
    cpu.regs_mut().set_reg(1, 0x183240);
    cpu.regs_mut().set_reg(2, 2);
    cpu.regs_mut().set_reg(3, 2);
    cpu.memory_mut().write32(GuestAddr::new(0x280000), 0x183280).unwrap();
    cpu.memory_mut().write32(GuestAddr::new(0x280004), 0).unwrap();

    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(cpu.regs().pc(), 0x8006C);
    assert_eq!(host.screen_buffer()[9 * 240 + 6], 0x1111);
    assert_eq!(host.screen_buffer()[9 * 240 + 7], 0x2222);
    assert_eq!(host.screen_buffer()[10 * 240 + 6], 0x3333);
    assert_eq!(host.screen_buffer()[10 * 240 + 7], 0x4444);
    assert_eq!(
        host.take_dirty_region(),
        Some(HostScreenRegion {
            x: 6,
            y: 9,
            w: 2,
            h: 2,
        })
    );
}

#[test]
fn host_handles_legacy_draw_bitmap_stub() {
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
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);

    cpu.memory_mut().write16(GuestAddr::new(0x184000), 0x1111).unwrap();
    cpu.memory_mut().write16(GuestAddr::new(0x184002), 0x2222).unwrap();
    cpu.memory_mut().write16(GuestAddr::new(0x184004), 0x3333).unwrap();
    cpu.memory_mut().write16(GuestAddr::new(0x184006), 0x4444).unwrap();

    cpu.regs_mut().set_pc(0x181D20);
    cpu.regs_mut().set_lr(0x8006A);
    cpu.regs_mut().set_sp(0x280000);
    cpu.regs_mut().set_reg(0, 0x184000);
    cpu.regs_mut().set_reg(1, 8);
    cpu.regs_mut().set_reg(2, 11);
    cpu.regs_mut().set_reg(3, 2);
    cpu.memory_mut().write32(GuestAddr::new(0x280000), 2).unwrap();

    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(cpu.regs().pc(), 0x8006A);
    assert_eq!(host.screen_buffer()[11 * 240 + 8], 0x1111);
    assert_eq!(host.screen_buffer()[11 * 240 + 9], 0x2222);
    assert_eq!(host.screen_buffer()[12 * 240 + 8], 0x3333);
    assert_eq!(host.screen_buffer()[12 * 240 + 9], 0x4444);
    assert_eq!(
        host.take_dirty_region(),
        Some(HostScreenRegion {
            x: 8,
            y: 11,
            w: 2,
            h: 2,
        })
    );
}

#[test]
fn host_handles_legacy_bitmap_check_stub() {
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
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);

    cpu.regs_mut().set_pc(0x181D00);
    cpu.regs_mut().set_lr(0x8006C);
    cpu.regs_mut().set_reg(0, 5);
    cpu.regs_mut().set_reg(1, 6);
    cpu.regs_mut().set_reg(2, 0xAAAA);
    assert!(host.handle(&mut cpu).unwrap());

    cpu.memory_mut().write16(GuestAddr::new(0x184000), 0x1111).unwrap();
    cpu.memory_mut().write16(GuestAddr::new(0x184002), 0x0000).unwrap();
    cpu.regs_mut().set_pc(0x181E00);
    cpu.regs_mut().set_lr(0x80070);
    cpu.regs_mut().set_sp(0x280000);
    cpu.regs_mut().set_reg(0, 0x184000);
    cpu.regs_mut().set_reg(1, 5);
    cpu.regs_mut().set_reg(2, 6);
    cpu.regs_mut().set_reg(3, 2);
    cpu.memory_mut().write32(GuestAddr::new(0x280000), 1).unwrap();
    cpu.memory_mut().write32(GuestAddr::new(0x280004), 0).unwrap();
    cpu.memory_mut().write32(GuestAddr::new(0x280008), 0xAAAA).unwrap();

    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);

    cpu.regs_mut().set_pc(0x181D00);
    cpu.regs_mut().set_lr(0x80074);
    cpu.regs_mut().set_reg(0, 5);
    cpu.regs_mut().set_reg(1, 6);
    cpu.regs_mut().set_reg(2, 0xBBBB);
    assert!(host.handle(&mut cpu).unwrap());

    cpu.regs_mut().set_pc(0x181E00);
    cpu.regs_mut().set_lr(0x80078);
    cpu.regs_mut().set_sp(0x280100);
    cpu.regs_mut().set_reg(0, 0x184000);
    cpu.regs_mut().set_reg(1, 5);
    cpu.regs_mut().set_reg(2, 6);
    cpu.regs_mut().set_reg(3, 2);
    cpu.memory_mut().write32(GuestAddr::new(0x280100), 1).unwrap();
    cpu.memory_mut().write32(GuestAddr::new(0x280104), 0).unwrap();
    cpu.memory_mut().write32(GuestAddr::new(0x280108), 0xAAAA).unwrap();

    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 1);
}

#[test]
fn host_handles_legacy_wstrlen_stub() {
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
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);

    cpu.memory_mut().write8(GuestAddr::new(0x184100), 0x00).unwrap();
    cpu.memory_mut().write8(GuestAddr::new(0x184101), b'A').unwrap();
    cpu.memory_mut().write8(GuestAddr::new(0x184102), 0x4E).unwrap();
    cpu.memory_mut().write8(GuestAddr::new(0x184103), 0x2D).unwrap();
    cpu.memory_mut().write8(GuestAddr::new(0x184104), 0x00).unwrap();
    cpu.memory_mut().write8(GuestAddr::new(0x184105), 0x00).unwrap();

    cpu.regs_mut().set_pc(0x181E80);
    cpu.regs_mut().set_lr(0x8007C);
    cpu.regs_mut().set_reg(0, 0x184100);

    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 4);
    assert_eq!(cpu.regs().pc(), 0x8007C);
}

#[test]
fn host_handles_legacy_draw_text_ascii_stub() {
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
    host.set_working_dir(r"D:\opt\rust\vmrp\wasm\dist\fs\mythroad");
    let mut cpu = Cpu::new(memory);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);

    write_c_string(cpu.memory_mut(), 0x184200, "A");
    cpu.regs_mut().set_pc(0x181DC0);
    cpu.regs_mut().set_lr(0x80080);
    cpu.regs_mut().set_sp(0x280000);
    cpu.regs_mut().set_reg(0, 0x184200);
    cpu.regs_mut().set_reg(1, 10);
    cpu.regs_mut().set_reg(2, 12);
    cpu.regs_mut().set_reg(3, 0x00);
    cpu.memory_mut().write32(GuestAddr::new(0x280000), 0xFF).unwrap();
    cpu.memory_mut().write32(GuestAddr::new(0x280004), 0x00).unwrap();
    cpu.memory_mut().write32(GuestAddr::new(0x280008), 0x00).unwrap();
    cpu.memory_mut().write32(GuestAddr::new(0x28000C), 0).unwrap();

    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(cpu.regs().pc(), 0x80080);

    let glyph = fs::read(sky16_font_path()).unwrap();
    let glyph = &glyph[(b'A' as usize) * 32..(b'A' as usize) * 32 + 32];
    let expected_color = 0x07E0u16;
    let mut lit = 0usize;
    for row in 0..16usize {
        let data = ((glyph[row * 2] as u16) << 8) | glyph[row * 2 + 1] as u16;
        for col in 0..16usize {
            let bit = data & (1 << (15 - col)) != 0;
            let pixel = host.screen_buffer()[(12 + row) * 240 + (10 + col)];
            if bit {
                lit += 1;
                assert_eq!(pixel, expected_color);
            }
        }
    }
    assert!(lit > 0);
    assert_eq!(
        host.take_dirty_region(),
        Some(HostScreenRegion {
            x: 10,
            y: 12,
            w: 8,
            h: 16,
        })
    );
}

#[test]
fn host_handles_legacy_draw_text_ex_ascii_clip_stub() {
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
    host.set_working_dir(r"D:\opt\rust\vmrp\wasm\dist\fs\mythroad");
    let mut cpu = Cpu::new(memory);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);

    write_c_string(cpu.memory_mut(), 0x184240, "AB");
    cpu.regs_mut().set_pc(0x181E40);
    cpu.regs_mut().set_lr(0x80084);
    cpu.regs_mut().set_sp(0x280000);
    cpu.regs_mut().set_reg(0, 0x184240);
    cpu.regs_mut().set_reg(1, 10);
    cpu.regs_mut().set_reg(2, 12);

    cpu.memory_mut().write16(GuestAddr::new(0x280000), 0).unwrap();
    cpu.memory_mut().write16(GuestAddr::new(0x280002), 0).unwrap();
    cpu.memory_mut().write16(GuestAddr::new(0x280004), 8).unwrap();
    cpu.memory_mut().write16(GuestAddr::new(0x280006), 16).unwrap();
    cpu.memory_mut().write8(GuestAddr::new(0x280008), 0xFF).unwrap();
    cpu.memory_mut().write8(GuestAddr::new(0x280009), 0x00).unwrap();
    cpu.memory_mut().write8(GuestAddr::new(0x28000A), 0x00).unwrap();
    cpu.memory_mut().write32(GuestAddr::new(0x28000C), 0).unwrap();
    cpu.memory_mut().write32(GuestAddr::new(0x280010), 0).unwrap();

    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 2);
    assert_eq!(cpu.regs().pc(), 0x80084);

    let expected_color = 0xF800u16;
    let glyph = fs::read(sky16_font_path()).unwrap();
    let glyph = &glyph[(b'A' as usize) * 32..(b'A' as usize) * 32 + 32];
    let mut lit = 0usize;
    for row in 0..16usize {
        let data = ((glyph[row * 2] as u16) << 8) | glyph[row * 2 + 1] as u16;
        for col in 0..8usize {
            let bit = data & (1 << (15 - col)) != 0;
            let pixel = host.screen_buffer()[(12 + row) * 240 + (10 + col)];
            if bit {
                lit += 1;
                assert_eq!(pixel, expected_color);
            }
        }
        assert_eq!(host.screen_buffer()[(12 + row) * 240 + 18], 0);
    }
    assert!(lit > 0);
    assert_eq!(
        host.take_dirty_region(),
        Some(HostScreenRegion {
            x: 10,
            y: 12,
            w: 8,
            h: 16,
        })
    );
}

#[test]
fn host_handles_legacy_network_stubs() {
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
    write_c_string(&mut memory, 0x183200, "example.com");
    write_c_string(&mut memory, 0x183240, "10086");
    write_c_string(&mut memory, 0x183280, "cmwap");

    let mut cpu = Cpu::new(memory);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);

    cpu.regs_mut().set_pc(0x1818D0);
    cpu.regs_mut().set_lr(0x80050);
    cpu.regs_mut().set_reg(0, 0x183240);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);

    cpu.regs_mut().set_pc(0x1818E0);
    cpu.regs_mut().set_lr(0x80052);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);

    cpu.regs_mut().set_pc(0x1818F0);
    cpu.regs_mut().set_lr(0x80053);
    cpu.regs_mut().set_reg(0, 0x183280);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);

    cpu.regs_mut().set_pc(0x181940);
    cpu.regs_mut().set_lr(0x80054);
    cpu.regs_mut().set_reg(0, 0);
    cpu.regs_mut().set_reg(1, 0);
    cpu.regs_mut().set_reg(2, 0);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);

    cpu.regs_mut().set_pc(0x181980);
    cpu.regs_mut().set_lr(0x80058);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);

    cpu.regs_mut().set_pc(0x181B80);
    cpu.regs_mut().set_lr(0x8005C);
    cpu.regs_mut().set_reg(0, 0);
    cpu.regs_mut().set_reg(1, 0x183200);
    cpu.regs_mut().set_reg(2, 11);
    cpu.regs_mut().set_reg(3, 0x7F000001);
    cpu.memory_mut().write32(GuestAddr::new(0x280000), 80).unwrap();
    cpu.regs_mut().set_sp(0x280000);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0) as i32, -1);
    assert_eq!(cpu.regs().pc(), 0x8005C);
}

#[test]
fn host_handles_legacy_exit_and_sms_stubs() {
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
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);

    cpu.regs_mut().set_pc(0x1815A0);
    cpu.regs_mut().set_lr(0x8004C);
    cpu.regs_mut().set_reg(0, 0x183200);
    cpu.regs_mut().set_reg(1, 0x183240);
    cpu.regs_mut().set_reg(2, 0);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);
    assert!(!host.exit_requested());

    cpu.regs_mut().set_pc(0x181590);
    cpu.regs_mut().set_lr(0x8004E);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);
    assert!(host.exit_requested());
}

#[test]
fn host_legacy_tcp_socket_can_connect_send_and_recv() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 4];
        stream.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"ping");
        stream.write_all(b"pong").unwrap();
    });

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

    write_c_string(&mut memory, 0x183200, "ping");

    let mut cpu = Cpu::new(memory);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);

    cpu.regs_mut().set_pc(0x181940);
    cpu.regs_mut().set_lr(0x80054);
    cpu.regs_mut().set_reg(0, 0);
    cpu.regs_mut().set_reg(1, 0);
    cpu.regs_mut().set_reg(2, 0);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);

    cpu.regs_mut().set_pc(0x1819C0);
    cpu.regs_mut().set_lr(0x80058);
    cpu.regs_mut().set_reg(0, 0);
    cpu.regs_mut().set_reg(1, 0);
    assert!(host.handle(&mut cpu).unwrap());
    let handle = cpu.regs().reg(0) as i32;
    assert!(handle > 0);

    cpu.regs_mut().set_pc(0x181A00);
    cpu.regs_mut().set_lr(0x8005A);
    cpu.regs_mut().set_reg(0, handle as u32);
    cpu.regs_mut().set_reg(1, 0x7F00_0001);
    cpu.regs_mut().set_reg(2, port as u32);
    cpu.regs_mut().set_reg(3, 0);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);

    cpu.regs_mut().set_pc(0x181A40);
    cpu.regs_mut().set_lr(0x8005C);
    cpu.regs_mut().set_reg(0, handle as u32);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);

    cpu.regs_mut().set_pc(0x181B00);
    cpu.regs_mut().set_lr(0x80060);
    cpu.regs_mut().set_reg(0, handle as u32);
    cpu.regs_mut().set_reg(1, 0x183200);
    cpu.regs_mut().set_reg(2, 4);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 4);

    cpu.regs_mut().set_pc(0x181AC0);
    cpu.regs_mut().set_lr(0x80064);
    cpu.regs_mut().set_reg(0, handle as u32);
    cpu.regs_mut().set_reg(1, 0x183240);
    cpu.regs_mut().set_reg(2, 4);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 4);
    assert_eq!(read_c_string(cpu.memory(), 0x183240), "pong");

    cpu.regs_mut().set_pc(0x181A80);
    cpu.regs_mut().set_lr(0x80068);
    cpu.regs_mut().set_reg(0, handle as u32);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);

    server.join().unwrap();
}

#[test]
fn host_legacy_send_invalid_guest_buffer_returns_failed_instead_of_host_fault() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (_stream, _) = listener.accept().unwrap();
    });

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
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);

    cpu.regs_mut().set_pc(0x181940);
    cpu.regs_mut().set_lr(0x80054);
    cpu.regs_mut().set_reg(0, 0);
    cpu.regs_mut().set_reg(1, 0);
    cpu.regs_mut().set_reg(2, 0);
    assert!(host.handle(&mut cpu).unwrap());

    cpu.regs_mut().set_pc(0x1819C0);
    cpu.regs_mut().set_lr(0x80058);
    cpu.regs_mut().set_reg(0, 0);
    cpu.regs_mut().set_reg(1, 0);
    assert!(host.handle(&mut cpu).unwrap());
    let handle = cpu.regs().reg(0) as i32;
    assert!(handle > 0);

    cpu.regs_mut().set_pc(0x181A00);
    cpu.regs_mut().set_lr(0x8005A);
    cpu.regs_mut().set_reg(0, handle as u32);
    cpu.regs_mut().set_reg(1, 0x7F00_0001);
    cpu.regs_mut().set_reg(2, port as u32);
    cpu.regs_mut().set_reg(3, 0);
    assert!(host.handle(&mut cpu).unwrap());

    cpu.regs_mut().set_pc(0x181B00);
    cpu.regs_mut().set_lr(0x80060);
    cpu.regs_mut().set_reg(0, handle as u32);
    cpu.regs_mut().set_reg(1, 0);
    cpu.regs_mut().set_reg(2, 4);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0) as i32, MR_FAILED);

    cpu.regs_mut().set_pc(0x181A80);
    cpu.regs_mut().set_lr(0x80068);
    cpu.regs_mut().set_reg(0, handle as u32);
    assert!(host.handle(&mut cpu).unwrap());

    server.join().unwrap();
}

#[test]
fn host_legacy_recv_invalid_guest_buffer_returns_failed_instead_of_host_fault() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.write_all(b"pong").unwrap();
    });

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
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);

    cpu.regs_mut().set_pc(0x181940);
    cpu.regs_mut().set_lr(0x80054);
    cpu.regs_mut().set_reg(0, 0);
    cpu.regs_mut().set_reg(1, 0);
    cpu.regs_mut().set_reg(2, 0);
    assert!(host.handle(&mut cpu).unwrap());

    cpu.regs_mut().set_pc(0x1819C0);
    cpu.regs_mut().set_lr(0x80058);
    cpu.regs_mut().set_reg(0, 0);
    cpu.regs_mut().set_reg(1, 0);
    assert!(host.handle(&mut cpu).unwrap());
    let handle = cpu.regs().reg(0) as i32;
    assert!(handle > 0);

    cpu.regs_mut().set_pc(0x181A00);
    cpu.regs_mut().set_lr(0x8005A);
    cpu.regs_mut().set_reg(0, handle as u32);
    cpu.regs_mut().set_reg(1, 0x7F00_0001);
    cpu.regs_mut().set_reg(2, port as u32);
    cpu.regs_mut().set_reg(3, 0);
    assert!(host.handle(&mut cpu).unwrap());

    cpu.regs_mut().set_pc(0x181AC0);
    cpu.regs_mut().set_lr(0x80064);
    cpu.regs_mut().set_reg(0, handle as u32);
    cpu.regs_mut().set_reg(1, 0);
    cpu.regs_mut().set_reg(2, 4);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0) as i32, MR_FAILED);

    cpu.regs_mut().set_pc(0x181A80);
    cpu.regs_mut().set_lr(0x80068);
    cpu.regs_mut().set_reg(0, handle as u32);
    assert!(host.handle(&mut cpu).unwrap());

    server.join().unwrap();
}

#[test]
fn host_legacy_udp_socket_can_sendto_and_recvfrom() {
    let server = UdpSocket::bind(("127.0.0.1", 0)).unwrap();
    let port = server.local_addr().unwrap().port();
    let worker = thread::spawn(move || {
        let mut buf = [0u8; 4];
        let (len, peer) = server.recv_from(&mut buf).unwrap();
        assert_eq!(len, 4);
        assert_eq!(&buf, b"ping");
        server.send_to(b"pong", peer).unwrap();
    });

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

    write_c_string(&mut memory, 0x183200, "ping");

    let mut cpu = Cpu::new(memory);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);

    cpu.regs_mut().set_pc(0x181940);
    cpu.regs_mut().set_lr(0x80070);
    cpu.regs_mut().set_reg(0, 0);
    cpu.regs_mut().set_reg(1, 0);
    cpu.regs_mut().set_reg(2, 0);
    assert!(host.handle(&mut cpu).unwrap());

    cpu.regs_mut().set_pc(0x1819C0);
    cpu.regs_mut().set_lr(0x80074);
    cpu.regs_mut().set_reg(0, 1);
    cpu.regs_mut().set_reg(1, 1);
    assert!(host.handle(&mut cpu).unwrap());
    let handle = cpu.regs().reg(0) as i32;
    assert!(handle > 0);

    cpu.regs_mut().set_pc(0x181B80);
    cpu.regs_mut().set_lr(0x80078);
    cpu.regs_mut().set_sp(0x280000);
    cpu.regs_mut().set_reg(0, handle as u32);
    cpu.regs_mut().set_reg(1, 0x183200);
    cpu.regs_mut().set_reg(2, 4);
    cpu.regs_mut().set_reg(3, 0x7F00_0001);
    cpu.memory_mut()
        .write32(GuestAddr::new(0x280000), port as u32)
        .unwrap();
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 4);

    cpu.regs_mut().set_pc(0x181B40);
    cpu.regs_mut().set_lr(0x8007C);
    cpu.regs_mut().set_sp(0x280100);
    cpu.regs_mut().set_reg(0, handle as u32);
    cpu.regs_mut().set_reg(1, 0x183240);
    cpu.regs_mut().set_reg(2, 4);
    cpu.regs_mut().set_reg(3, 0x183280);
    cpu.memory_mut()
        .write32(GuestAddr::new(0x280100), 0x183284)
        .unwrap();
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 4);
    assert_eq!(read_c_string(cpu.memory(), 0x183240), "pong");
    assert_eq!(
        cpu.memory().read32(GuestAddr::new(0x183280)).unwrap(),
        0x7F00_0001
    );
    assert_eq!(
        cpu.memory().read16(GuestAddr::new(0x183284)).unwrap(),
        port
    );

    cpu.regs_mut().set_pc(0x181A80);
    cpu.regs_mut().set_lr(0x80080);
    cpu.regs_mut().set_reg(0, handle as u32);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);

    worker.join().unwrap();
}

#[test]
fn host_legacy_sendto_invalid_guest_stack_returns_failed_instead_of_host_fault() {
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
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);

    cpu.regs_mut().set_pc(0x181940);
    cpu.regs_mut().set_lr(0x80070);
    cpu.regs_mut().set_reg(0, 0);
    cpu.regs_mut().set_reg(1, 0);
    cpu.regs_mut().set_reg(2, 0);
    assert!(host.handle(&mut cpu).unwrap());

    cpu.regs_mut().set_pc(0x1819C0);
    cpu.regs_mut().set_lr(0x80074);
    cpu.regs_mut().set_reg(0, 1);
    cpu.regs_mut().set_reg(1, 1);
    assert!(host.handle(&mut cpu).unwrap());
    let handle = cpu.regs().reg(0) as i32;
    assert!(handle > 0);

    cpu.regs_mut().set_pc(0x181B80);
    cpu.regs_mut().set_lr(0x80078);
    cpu.regs_mut().set_sp(0);
    cpu.regs_mut().set_reg(0, handle as u32);
    cpu.regs_mut().set_reg(1, 0);
    cpu.regs_mut().set_reg(2, 4);
    cpu.regs_mut().set_reg(3, 0x7F00_0001);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0) as i32, MR_FAILED);
}

#[test]
fn dsm_network_callbacks_delegate_to_legacy_network_behaviour() {
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
    let dsm_table = GuestAddr::new(0x184000);
    host.install_dsm_require_funcs(&mut memory, dsm_table, 0).unwrap();
    write_c_string(&mut memory, 0x183200, "example.com");

    let get_host_by_name = memory.read32(GuestAddr::new(dsm_table.get() + 0x6C)).unwrap();
    let init_network = memory.read32(GuestAddr::new(dsm_table.get() + 0x70)).unwrap();
    let socket = memory.read32(GuestAddr::new(dsm_table.get() + 0x78)).unwrap();
    let close_socket = memory.read32(GuestAddr::new(dsm_table.get() + 0x84)).unwrap();

    let mut cpu = Cpu::new(memory);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);

    cpu.regs_mut().set_pc(get_host_by_name);
    cpu.regs_mut().set_lr(0x80070);
    cpu.regs_mut().set_reg(0, 0x183200);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0x7F00_0001);
    assert_eq!(cpu.regs().pc(), 0x80070);

    cpu.regs_mut().set_pc(init_network);
    cpu.regs_mut().set_lr(0x80074);
    cpu.regs_mut().set_reg(0, 0);
    cpu.regs_mut().set_reg(1, 0);
    cpu.regs_mut().set_reg(2, 0);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(cpu.regs().pc(), 0x80074);

    cpu.regs_mut().set_pc(socket);
    cpu.regs_mut().set_lr(0x80078);
    cpu.regs_mut().set_reg(0, 0);
    cpu.regs_mut().set_reg(1, 0);
    cpu.regs_mut().set_reg(2, 0);
    assert!(host.handle(&mut cpu).unwrap());
    let handle = cpu.regs().reg(0) as i32;
    assert!(handle > 0);
    assert_eq!(cpu.regs().pc(), 0x80078);

    cpu.regs_mut().set_pc(close_socket);
    cpu.regs_mut().set_lr(0x8007C);
    cpu.regs_mut().set_reg(0, handle as u32);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(cpu.regs().pc(), 0x8007C);
}

#[test]
fn dsm_file_callbacks_can_read_registered_package_entries() {
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
    host.register_package_file("folder.bmp", b"BMP!".to_vec());

    let dsm_table = GuestAddr::new(0x184000);
    host.install_dsm_require_funcs(&mut memory, dsm_table, 0).unwrap();
    write_c_string(&mut memory, 0x183200, "folder.bmp");

    let info = memory.read32(GuestAddr::new(dsm_table.get() + 0x44)).unwrap();
    let open = memory.read32(GuestAddr::new(dsm_table.get() + 0x30)).unwrap();
    let read = memory.read32(GuestAddr::new(dsm_table.get() + 0x38)).unwrap();
    let seek = memory.read32(GuestAddr::new(dsm_table.get() + 0x40)).unwrap();
    let close = memory.read32(GuestAddr::new(dsm_table.get() + 0x34)).unwrap();
    let get_len = memory.read32(GuestAddr::new(dsm_table.get() + 0x64)).unwrap();

    let mut cpu = Cpu::new(memory);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);

    cpu.regs_mut().set_pc(info);
    cpu.regs_mut().set_lr(0x80070);
    cpu.regs_mut().set_reg(0, 0x183200);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0) as i32, 1);

    cpu.regs_mut().set_pc(get_len);
    cpu.regs_mut().set_lr(0x80074);
    cpu.regs_mut().set_reg(0, 0x183200);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 4);

    cpu.regs_mut().set_pc(open);
    cpu.regs_mut().set_lr(0x80078);
    cpu.regs_mut().set_reg(0, 0x183200);
    cpu.regs_mut().set_reg(1, 0);
    assert!(host.handle(&mut cpu).unwrap());
    let fd = cpu.regs().reg(0) as i32;
    assert!(fd > 0);

    cpu.regs_mut().set_pc(read);
    cpu.regs_mut().set_lr(0x8007C);
    cpu.regs_mut().set_reg(0, fd as u32);
    cpu.regs_mut().set_reg(1, 0x183240);
    cpu.regs_mut().set_reg(2, 2);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 2);
    assert_eq!(cpu.memory().read8(GuestAddr::new(0x183240)).unwrap(), b'B');
    assert_eq!(cpu.memory().read8(GuestAddr::new(0x183241)).unwrap(), b'M');

    cpu.regs_mut().set_pc(seek);
    cpu.regs_mut().set_lr(0x80080);
    cpu.regs_mut().set_reg(0, fd as u32);
    cpu.regs_mut().set_reg(1, 0);
    cpu.regs_mut().set_reg(2, 0);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);

    cpu.regs_mut().set_pc(read);
    cpu.regs_mut().set_lr(0x80084);
    cpu.regs_mut().set_reg(0, fd as u32);
    cpu.regs_mut().set_reg(1, 0x183244);
    cpu.regs_mut().set_reg(2, 4);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 4);
    assert_eq!(cpu.memory().read8(GuestAddr::new(0x183244)).unwrap(), b'B');
    assert_eq!(cpu.memory().read8(GuestAddr::new(0x183245)).unwrap(), b'M');
    assert_eq!(cpu.memory().read8(GuestAddr::new(0x183246)).unwrap(), b'P');
    assert_eq!(cpu.memory().read8(GuestAddr::new(0x183247)).unwrap(), b'!');

    cpu.regs_mut().set_pc(close);
    cpu.regs_mut().set_lr(0x80088);
    cpu.regs_mut().set_reg(0, fd as u32);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);
}

#[test]
fn dsm_ui_callbacks_delegate_to_legacy_ui_behaviour() {
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
    let dsm_table = GuestAddr::new(0x184000);
    host.install_dsm_require_funcs(&mut memory, dsm_table, 0).unwrap();
    write_c_string(&mut memory, 0x183200, "seed");

    let dialog_create = memory.read32(GuestAddr::new(dsm_table.get() + 0xA8)).unwrap();
    let dialog_refresh = memory.read32(GuestAddr::new(dsm_table.get() + 0xB0)).unwrap();
    let dialog_release = memory.read32(GuestAddr::new(dsm_table.get() + 0xAC)).unwrap();
    let edit_create = memory.read32(GuestAddr::new(dsm_table.get() + 0xC0)).unwrap();
    let edit_get_text = memory.read32(GuestAddr::new(dsm_table.get() + 0xC8)).unwrap();
    let edit_release = memory.read32(GuestAddr::new(dsm_table.get() + 0xC4)).unwrap();

    let mut cpu = Cpu::new(memory);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);

    cpu.regs_mut().set_pc(dialog_create);
    cpu.regs_mut().set_lr(0x80080);
    cpu.regs_mut().set_reg(0, 0x183200);
    cpu.regs_mut().set_reg(1, 0x183200);
    assert!(host.handle(&mut cpu).unwrap());
    let dialog = cpu.regs().reg(0) as i32;
    assert!(dialog > 0);
    assert_eq!(cpu.regs().pc(), 0x80080);

    cpu.regs_mut().set_pc(dialog_refresh);
    cpu.regs_mut().set_lr(0x80084);
    cpu.regs_mut().set_reg(0, dialog as u32);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(cpu.regs().pc(), 0x80084);

    cpu.regs_mut().set_pc(dialog_release);
    cpu.regs_mut().set_lr(0x80088);
    cpu.regs_mut().set_reg(0, dialog as u32);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(cpu.regs().pc(), 0x80088);

    cpu.regs_mut().set_pc(edit_create);
    cpu.regs_mut().set_lr(0x8008C);
    cpu.regs_mut().set_reg(0, 0x183200);
    cpu.regs_mut().set_reg(1, 0x183200);
    cpu.regs_mut().set_reg(2, 0);
    cpu.regs_mut().set_reg(3, 16);
    assert!(host.handle(&mut cpu).unwrap());
    let edit = cpu.regs().reg(0) as i32;
    assert!(edit > 0);
    assert_eq!(cpu.regs().pc(), 0x8008C);

    cpu.regs_mut().set_pc(edit_get_text);
    cpu.regs_mut().set_lr(0x80090);
    cpu.regs_mut().set_reg(0, edit as u32);
    assert!(host.handle(&mut cpu).unwrap());
    let text_ptr = cpu.regs().reg(0);
    assert_ne!(text_ptr, 0);
    assert_eq!(read_c_string(cpu.memory(), text_ptr), "seed");
    assert_eq!(cpu.regs().pc(), 0x80090);

    cpu.regs_mut().set_pc(edit_release);
    cpu.regs_mut().set_lr(0x80094);
    cpu.regs_mut().set_reg(0, edit as u32);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(cpu.regs().pc(), 0x80094);
}

#[test]
fn host_legacy_menu_and_window_callbacks_delegate_to_ui_state() {
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

    write_c_string(&mut memory, 0x183200, "Main");
    write_c_string(&mut memory, 0x183240, "Start");

    let mut cpu = Cpu::new(memory);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);

    cpu.regs_mut().set_pc(0x181C00);
    cpu.regs_mut().set_lr(0x80098);
    cpu.regs_mut().set_reg(0, 0x183200);
    cpu.regs_mut().set_reg(1, 2);
    assert!(host.handle(&mut cpu).unwrap());
    let menu = cpu.regs().reg(0) as i32;
    assert!(menu > 0);
    assert_eq!(cpu.regs().pc(), 0x80098);

    cpu.regs_mut().set_pc(0x181C10);
    cpu.regs_mut().set_lr(0x8009C);
    cpu.regs_mut().set_reg(0, menu as u32);
    cpu.regs_mut().set_reg(1, 0x183240);
    cpu.regs_mut().set_reg(2, 0);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(cpu.regs().pc(), 0x8009C);

    cpu.regs_mut().set_pc(0x181C20);
    cpu.regs_mut().set_lr(0x800A0);
    cpu.regs_mut().set_reg(0, menu as u32);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);

    cpu.regs_mut().set_pc(0x181C40);
    cpu.regs_mut().set_lr(0x800A4);
    cpu.regs_mut().set_reg(0, menu as u32);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);

    cpu.regs_mut().set_pc(0x181C30);
    cpu.regs_mut().set_lr(0x800A8);
    cpu.regs_mut().set_reg(0, menu as u32);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);

    cpu.regs_mut().set_pc(0x181C50);
    cpu.regs_mut().set_lr(0x800AC);
    assert!(host.handle(&mut cpu).unwrap());
    let win = cpu.regs().reg(0) as i32;
    assert!(win > 0);

    cpu.regs_mut().set_pc(0x181C60);
    cpu.regs_mut().set_lr(0x800B0);
    cpu.regs_mut().set_reg(0, win as u32);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(cpu.regs().pc(), 0x800B0);
}
