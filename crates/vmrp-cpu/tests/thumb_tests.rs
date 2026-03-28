use vmrp_core::GuestAddr;
use vmrp_cpu::{Cpu, ExecutionMode, MemoryBus, TestMemory};

fn new_thumb_cpu(opcode: u16) -> Cpu<TestMemory> {
    let mut mem = TestMemory::with_ram(GuestAddr::new(0x80000), 0x1000);
    mem.write16(GuestAddr::new(0x80000), opcode).unwrap();

    let mut cpu = Cpu::new(mem);
    cpu.regs_mut().set_pc(0x80000);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Thumb);
    cpu
}

#[test]
fn thumb_mov_immediate_updates_destination_register() {
    let mut cpu = new_thumb_cpu(0x2001);

    cpu.step().unwrap();

    assert_eq!(cpu.regs().reg(0), 1);
}

#[test]
fn thumb_add_updates_registers() {
    let mut cpu = new_thumb_cpu(0x3003);
    cpu.regs_mut().set_reg(0, 2);

    cpu.step().unwrap();

    assert_eq!(cpu.regs().reg(0), 5);
}

#[test]
fn thumb_state_advances_pc_by_two_bytes() {
    let mut cpu = new_thumb_cpu(0x2000);

    cpu.step().unwrap();

    assert_eq!(cpu.regs().pc(), 0x80002);
}

#[test]
fn thumb_adjust_sp_subtracts_immediate_times_four() {
    let mut cpu = new_thumb_cpu(0xB083);
    cpu.regs_mut().set_sp(0x9000);

    cpu.step().unwrap();

    assert_eq!(cpu.regs().sp(), 0x8FF4);
    assert_eq!(cpu.regs().pc(), 0x80002);
}

#[test]
fn thumb_lsl_immediate_shifts_register() {
    let mut cpu = new_thumb_cpu(0x005B);
    cpu.regs_mut().set_reg(3, 0x21);

    cpu.step().unwrap();

    assert_eq!(cpu.regs().reg(3), 0x42);
    assert_eq!(cpu.regs().pc(), 0x80002);
}

#[test]
fn thumb_ldrb_register_offset_loads_byte() {
    let mut cpu = new_thumb_cpu(0x5C5B);
    cpu.regs_mut().set_reg(3, 0x80080);
    cpu.regs_mut().set_reg(1, 2);
    cpu.memory_mut().write8(GuestAddr::new(0x80082), 0xAB).unwrap();

    cpu.step().unwrap();

    assert_eq!(cpu.regs().reg(3), 0xAB);
    assert_eq!(cpu.regs().pc(), 0x80002);
}

#[test]
fn thumb_ldrb_immediate_loads_byte() {
    let mut cpu = new_thumb_cpu(0x7C18);
    cpu.regs_mut().set_reg(3, 0x80080);
    cpu.memory_mut().write8(GuestAddr::new(0x80090), 0xCD).unwrap();

    cpu.step().unwrap();

    assert_eq!(cpu.regs().reg(0), 0xCD);
    assert_eq!(cpu.regs().pc(), 0x80002);
}

#[test]
fn thumb_stmia_stores_registers_and_writes_back_base() {
    let mut cpu = new_thumb_cpu(0xC010);
    cpu.regs_mut().set_reg(0, 0x80080);
    cpu.regs_mut().set_reg(4, 0x1234_5678);

    cpu.step().unwrap();

    assert_eq!(cpu.memory().read32(GuestAddr::new(0x80080)).unwrap(), 0x1234_5678);
    assert_eq!(cpu.regs().reg(0), 0x80084);
    assert_eq!(cpu.regs().pc(), 0x80002);
}

#[test]
fn thumb_str_sp_relative_stores_word() {
    let mut cpu = new_thumb_cpu(0x9102);
    cpu.regs_mut().set_sp(0x80080);
    cpu.regs_mut().set_reg(1, 0xCAFE_BABE);

    cpu.step().unwrap();

    assert_eq!(cpu.memory().read32(GuestAddr::new(0x80088)).unwrap(), 0xCAFE_BABE);
    assert_eq!(cpu.regs().pc(), 0x80002);
}

#[test]
fn thumb_adr_uses_aligned_pc_base() {
    let mut cpu = new_thumb_cpu(0xA301);

    cpu.step().unwrap();

    assert_eq!(cpu.regs().reg(3), 0x80008);
    assert_eq!(cpu.regs().pc(), 0x80002);
}

#[test]
fn thumb_ldr_sp_relative_loads_word() {
    let mut cpu = new_thumb_cpu(0x9904);
    cpu.regs_mut().set_sp(0x80080);
    cpu.memory_mut().write32(GuestAddr::new(0x80090), 0x0BAD_F00D).unwrap();

    cpu.step().unwrap();

    assert_eq!(cpu.regs().reg(1), 0x0BAD_F00D);
    assert_eq!(cpu.regs().pc(), 0x80002);
}

#[test]
fn thumb_blx_register_aligns_arm_target_to_word_boundary() {
    let mut cpu = new_thumb_cpu(0x4790);
    cpu.regs_mut().set_reg(2, 0x80022);

    cpu.step().unwrap();

    assert_eq!(cpu.regs().lr(), 0x80003);
    assert_eq!(cpu.regs().pc(), 0x80020);
    assert_eq!(cpu.regs().execution_mode(), ExecutionMode::Arm);
}

#[test]
fn thumb_pop_with_pc_restores_register_and_returns() {
    let mut cpu = new_thumb_cpu(0xBD10);
    cpu.regs_mut().set_sp(0x80080);
    cpu.memory_mut().write32(GuestAddr::new(0x80080), 0x1111_2222).unwrap();
    cpu.memory_mut().write32(GuestAddr::new(0x80084), 0x80021).unwrap();

    cpu.step().unwrap();

    assert_eq!(cpu.regs().reg(4), 0x1111_2222);
    assert_eq!(cpu.regs().sp(), 0x80088);
    assert_eq!(cpu.regs().pc(), 0x80020);
    assert_eq!(cpu.regs().execution_mode(), ExecutionMode::Thumb);
}






