use vmrp_core::GuestAddr;
use vmrp_cpu::{Cpu, CpuError, ExecutionMode, MemoryBus, TestMemory};

fn new_arm_cpu(opcode: u32) -> Cpu<TestMemory> {
    let mut mem = TestMemory::with_ram(GuestAddr::new(0x80000), 0x1000);
    mem.write32(GuestAddr::new(0x80000), opcode).unwrap();

    let mut cpu = Cpu::new(mem);
    cpu.regs_mut().set_pc(0x80000);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu
}

#[test]
fn branch_updates_pc_relative_to_arm_semantics() {
    let mut cpu = new_arm_cpu(0xEA00_0001);

    cpu.step().unwrap();

    assert_eq!(cpu.regs().pc(), 0x8000C);
}

#[test]
fn branch_with_link_sets_lr_before_jump() {
    let mut cpu = new_arm_cpu(0xEB00_0001);

    cpu.step().unwrap();

    assert_eq!(cpu.regs().lr(), 0x80004);
    assert_eq!(cpu.regs().pc(), 0x8000C);
}

#[test]
fn eq_branch_taken_when_zero_set() {
    let mut cpu = new_arm_cpu(0x0A00_0036);
    cpu.regs_mut().cpsr_mut().set_zero(true);

    cpu.step().unwrap();

    assert_eq!(cpu.regs().pc(), 0x800E0);
}

#[test]
fn eq_branch_not_taken_when_zero_clear() {
    let mut cpu = new_arm_cpu(0x0A00_0036);
    cpu.regs_mut().cpsr_mut().set_zero(false);

    cpu.step().unwrap();

    assert_eq!(cpu.regs().pc(), 0x80004);
}

#[test]
fn blx_register_sets_lr_and_pc_in_arm_mode() {
    let mut cpu = new_arm_cpu(0xE12F_FF32);
    cpu.regs_mut().set_reg(2, 0x80020);

    cpu.step().unwrap();

    assert_eq!(cpu.regs().lr(), 0x80004);
    assert_eq!(cpu.regs().pc(), 0x80020);
    assert_eq!(cpu.regs().execution_mode(), ExecutionMode::Arm);
}

#[test]
fn blx_register_switches_to_thumb_when_target_lsb_set() {
    let mut cpu = new_arm_cpu(0xE12F_FF32);
    cpu.regs_mut().set_reg(2, 0x80021);

    cpu.step().unwrap();

    assert_eq!(cpu.regs().lr(), 0x80004);
    assert_eq!(cpu.regs().pc(), 0x80020);
    assert_eq!(cpu.regs().execution_mode(), ExecutionMode::Thumb);
}

#[test]
fn run_until_stops_after_step_limit() {
    let mut cpu = new_arm_cpu(0xEAFF_FFFE);

    let err = cpu.run_until(3).unwrap_err();

    assert!(matches!(
        err,
        CpuError::StepLimitExceeded {
            steps: 3,
            last_pc: 0x80000
        }
    ));
}

#[test]
fn unknown_arm_opcode_is_skipped_when_condition_fails() {
    let mut cpu = new_arm_cpu(0x908F_F108);
    cpu.regs_mut().cpsr_mut().set_carry(true);
    cpu.regs_mut().cpsr_mut().set_zero(false);

    cpu.step().unwrap();

    assert_eq!(cpu.regs().pc(), 0x80004);
}

#[test]
fn arm_block_transfer_load_pc_switches_to_thumb_when_lsb_set() {
    let mut cpu = new_arm_cpu(0xE8BD_8008);
    cpu.regs_mut().set_sp(0x80080);
    cpu.memory_mut()
        .write32(GuestAddr::new(0x80080), 0x1111_2222)
        .unwrap();
    cpu.memory_mut()
        .write32(GuestAddr::new(0x80084), 0x80021)
        .unwrap();

    cpu.step().unwrap();

    assert_eq!(cpu.regs().pc(), 0x80020);
    assert_eq!(cpu.regs().execution_mode(), ExecutionMode::Thumb);
}

#[test]
fn arm_block_transfer_load_pc_aligns_to_word_boundary() {
    let mut cpu = new_arm_cpu(0xE8BD_8008);
    cpu.regs_mut().set_sp(0x80080);
    cpu.memory_mut()
        .write32(GuestAddr::new(0x80080), 0x1111_2222)
        .unwrap();
    cpu.memory_mut()
        .write32(GuestAddr::new(0x80084), 0x80022)
        .unwrap();

    cpu.step().unwrap();

    assert_eq!(cpu.regs().reg(3), 0x1111_2222);
    assert_eq!(cpu.regs().pc(), 0x80020);
}

#[test]
fn bx_register_aligns_arm_target_to_word_boundary() {
    let mut cpu = new_arm_cpu(0xE12F_FF12);
    cpu.regs_mut().set_reg(2, 0x80022);

    cpu.step().unwrap();

    assert_eq!(cpu.regs().pc(), 0x80020);
    assert_eq!(cpu.regs().execution_mode(), ExecutionMode::Arm);
}

#[test]
fn blx_immediate_switches_to_thumb_and_branches() {
    let mut cpu = new_arm_cpu(0xFA00_0001);

    cpu.step().unwrap();

    assert_eq!(cpu.regs().lr(), 0x80004);
    assert_eq!(cpu.regs().pc(), 0x8000C);
    assert_eq!(cpu.regs().execution_mode(), ExecutionMode::Thumb);
}
