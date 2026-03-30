use vmrp_core::GuestAddr;
use vmrp_cpu::{Cpu, ExecutionMode, MemoryBus, TestMemory};

fn new_arm_cpu(opcode: u32) -> Cpu<TestMemory> {
    let mut mem = TestMemory::with_ram(GuestAddr::new(0x80000), 0x1000);
    mem.write32(GuestAddr::new(0x80000), opcode).unwrap();

    let mut cpu = Cpu::new(mem);
    cpu.regs_mut().set_pc(0x80000);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu
}

#[test]
fn arm_ldr_word_from_unaligned_address_rotates_loaded_value() {
    let mut cpu = new_arm_cpu(0xE591_0000);
    cpu.regs_mut().set_reg(1, 0x80005);

    cpu.memory_mut()
        .write8(GuestAddr::new(0x80004), 0x04)
        .unwrap();
    cpu.memory_mut()
        .write8(GuestAddr::new(0x80005), 0x05)
        .unwrap();
    cpu.memory_mut()
        .write8(GuestAddr::new(0x80006), 0x06)
        .unwrap();
    cpu.memory_mut()
        .write8(GuestAddr::new(0x80007), 0x07)
        .unwrap();

    cpu.step().unwrap();

    assert_eq!(cpu.regs().reg(0), 0x0407_0605);
    assert_eq!(cpu.regs().pc(), 0x80004);
}

#[test]
fn arm_str_word_to_unaligned_address_uses_aligned_word_address() {
    let mut cpu = new_arm_cpu(0xE581_0000);
    cpu.regs_mut().set_reg(0, 0x1122_3344);
    cpu.regs_mut().set_reg(1, 0x80005);

    cpu.step().unwrap();

    assert_eq!(
        cpu.memory().read32(GuestAddr::new(0x80004)).unwrap(),
        0x1122_3344
    );
    assert_eq!(cpu.regs().pc(), 0x80004);
}

#[test]
fn arm_ldrh_immediate_loads_halfword() {
    let mut cpu = new_arm_cpu(0xE1D3_20B8);
    cpu.regs_mut().set_reg(3, 0x80010);

    cpu.memory_mut()
        .write16(GuestAddr::new(0x80018), 0x1234)
        .unwrap();

    cpu.step().unwrap();

    assert_eq!(cpu.regs().reg(2), 0x1234);
    assert_eq!(cpu.regs().pc(), 0x80004);
}

#[test]
fn arm_ldrsh_immediate_sign_extends_halfword() {
    let mut cpu = new_arm_cpu(0xE1D3_20F8);
    cpu.regs_mut().set_reg(3, 0x80010);

    cpu.memory_mut()
        .write16(GuestAddr::new(0x80018), 0xFF80)
        .unwrap();

    cpu.step().unwrap();

    assert_eq!(cpu.regs().reg(2), 0xFFFF_FF80);
    assert_eq!(cpu.regs().pc(), 0x80004);
}

#[test]
fn arm_strh_immediate_stores_halfword() {
    let mut cpu = new_arm_cpu(0xE1C3_20B8);
    cpu.regs_mut().set_reg(2, 0xABCD_1234);
    cpu.regs_mut().set_reg(3, 0x80010);

    cpu.step().unwrap();

    assert_eq!(
        cpu.memory().read16(GuestAddr::new(0x80018)).unwrap(),
        0x1234
    );
    assert_eq!(cpu.regs().pc(), 0x80004);
}
