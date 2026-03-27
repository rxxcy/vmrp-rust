use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use vmrp_core::{GuestAddr, DEFAULT_LAYOUT};
use vmrp_cpu::{Cpu, ExecutionMode, MemoryBus, TestMemory};
use vmrp_platform::{ExtHost, FLAG_USE_UTF8_EDIT};

fn make_temp_dir() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("vmrp-platform-test-{nonce}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn new_host() -> ExtHost {
    ExtHost::new(
        GuestAddr::new(0x181000),
        GuestAddr::new(0x182000),
        GuestAddr::new(0x181100),
        GuestAddr::new(0x181140),
        GuestAddr::new(0x181180),
        GuestAddr::new(0x181200),
        GuestAddr::new(0x181300),
        GuestAddr::new(0x183000),
        0x20000,
    )
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
fn install_dsm_require_funcs_sets_flags_and_callbacks() {
    let mut host = new_host();
    let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x200000);

    host.install_dsm_require_funcs(&mut memory, GuestAddr::new(0x190000), FLAG_USE_UTF8_EDIT)
        .unwrap();

    assert_ne!(memory.read32(GuestAddr::new(0x190030)).unwrap(), 0);
    assert_ne!(memory.read32(GuestAddr::new(0x190038)).unwrap(), 0);
    assert_eq!(memory.read32(GuestAddr::new(0x1900CC)).unwrap(), FLAG_USE_UTF8_EDIT);
}

#[test]
fn dsm_file_callbacks_open_read_close() {
    let dir = make_temp_dir();
    let file_path = dir.join("sample.bin");
    fs::write(&file_path, b"ABCD").unwrap();

    let mut host = new_host();
    host.set_working_dir(&dir);

    let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x200000);
    host.install_dsm_require_funcs(&mut memory, GuestAddr::new(0x190000), FLAG_USE_UTF8_EDIT)
        .unwrap();
    write_c_string(&mut memory, 0x191000, "sample.bin");

    let mut cpu = Cpu::new(memory);

    let open_pc = cpu.memory().read32(GuestAddr::new(0x190030)).unwrap();
    cpu.regs_mut().set_pc(open_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_reg(0, 0x191000);
    cpu.regs_mut().set_reg(1, 1);
    assert!(host.handle(&mut cpu).unwrap());
    let fd = cpu.regs().reg(0) as i32;
    assert!(fd >= 0);

    let read_pc = cpu.memory().read32(GuestAddr::new(0x190038)).unwrap();
    cpu.regs_mut().set_pc(read_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80004);
    cpu.regs_mut().set_reg(0, fd as u32);
    cpu.regs_mut().set_reg(1, 0x191100);
    cpu.regs_mut().set_reg(2, 4);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 4);
    assert_eq!(cpu.memory().read32(GuestAddr::new(0x191100)).unwrap(), 0x44434241);

    let close_pc = cpu.memory().read32(GuestAddr::new(0x190034)).unwrap();
    cpu.regs_mut().set_pc(close_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80008);
    cpu.regs_mut().set_reg(0, fd as u32);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0) as i32, 0);

    let _ = fs::remove_file(file_path);
    let _ = fs::remove_dir_all(dir);
}
