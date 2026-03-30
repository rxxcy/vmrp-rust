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
        cpu.memory().read32(GuestAddr::new(0x180144)).unwrap(),
        0x181140
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
    assert_eq!(cpu.regs().reg(0), 0x182000);
    let helper = host.ext_helper_addr().unwrap();
    assert!(helper.get() >= 0x80000);
}
