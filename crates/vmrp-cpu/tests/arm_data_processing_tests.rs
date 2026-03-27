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
fn mov_immediate_updates_destination_register() {
    let mut cpu = new_arm_cpu(0xE3A0_0001);

    cpu.step().unwrap();

    assert_eq!(cpu.regs().reg(0), 1);
    assert_eq!(cpu.regs().pc(), 0x80004);
}

#[test]
fn mov_register_updates_destination_register() {
    let mut cpu = new_arm_cpu(0xE1A0_4000);
    cpu.regs_mut().set_reg(0, 0x1234_5678);

    cpu.step().unwrap();

    assert_eq!(cpu.regs().reg(4), 0x1234_5678);
    assert_eq!(cpu.regs().pc(), 0x80004);
}

#[test]
fn add_immediate_updates_destination_register() {
    let mut cpu = new_arm_cpu(0xE280_0003);
    cpu.regs_mut().set_reg(0, 2);

    cpu.step().unwrap();

    assert_eq!(cpu.regs().reg(0), 5);
    assert_eq!(cpu.regs().pc(), 0x80004);
}

#[test]
fn sub_register_updates_destination_register() {
    let mut cpu = new_arm_cpu(0xE041_5000);
    cpu.regs_mut().set_reg(1, 0x104);
    cpu.regs_mut().set_reg(0, 4);

    cpu.step().unwrap();

    assert_eq!(cpu.regs().reg(5), 0x100);
    assert_eq!(cpu.regs().pc(), 0x80004);
}

#[test]
fn cmp_updates_zero_flag() {
    let mut cpu = new_arm_cpu(0xE350_0003);
    cpu.regs_mut().set_reg(0, 3);

    cpu.step().unwrap();

    assert!(cpu.regs().cpsr().zero());
    assert_eq!(cpu.regs().pc(), 0x80004);
}

#[test]
fn cmn_updates_zero_flag_from_addition_result() {
    let mut cpu = new_arm_cpu(0xE370_0001);
    cpu.regs_mut().set_reg(0, 0xFFFF_FFFF);

    cpu.step().unwrap();

    assert!(cpu.regs().cpsr().zero());
    assert_eq!(cpu.regs().pc(), 0x80004);
}

#[test]
fn cmp_sets_carry_when_no_borrow() {
    let mut cpu = new_arm_cpu(0xE350_0003);
    cpu.regs_mut().set_reg(0, 5);

    cpu.step().unwrap();

    assert!(cpu.regs().cpsr().carry());
    assert_eq!(cpu.regs().pc(), 0x80004);
}

#[test]
fn cmp_clears_carry_when_borrow() {
    let mut cpu = new_arm_cpu(0xE350_0003);
    cpu.regs_mut().set_reg(0, 2);

    cpu.step().unwrap();

    assert!(!cpu.regs().cpsr().carry());
    assert_eq!(cpu.regs().pc(), 0x80004);
}

#[test]
fn add_register_with_shifted_operand_can_write_pc() {
    let mut cpu = new_arm_cpu(0xE08F_F108);
    cpu.regs_mut().set_reg(8, 1);

    cpu.step().unwrap();

    assert_eq!(cpu.regs().pc(), 0x8000C);
}

#[test]
fn and_immediate_masks_bits() {
    let mut cpu = new_arm_cpu(0xE202_30FF);
    cpu.regs_mut().set_reg(2, 0xABCD_1234);

    cpu.step().unwrap();

    assert_eq!(cpu.regs().reg(3), 0x34);
    assert_eq!(cpu.regs().pc(), 0x80004);
}
