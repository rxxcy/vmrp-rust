use vmrp_core::GuestAddr;
use vmrp_cpu::{Cpu, ExecutionMode, MemoryBus, TestMemory};

fn new_arm_cpu(opcode: u32) -> Cpu<TestMemory> {
    let mut mem = TestMemory::with_ram(GuestAddr::new(0x80000), 0x20000);
    mem.write32(GuestAddr::new(0x80000), opcode).unwrap();

    let mut cpu = Cpu::new(mem);
    cpu.regs_mut().set_pc(0x80000);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu
}

#[test]
fn push_like_block_transfer_updates_sp_and_stores_registers() {
    let mut cpu = new_arm_cpu(0xE92D4038);
    cpu.regs_mut().set_sp(0x90000);
    cpu.regs_mut().set_reg(3, 0x33);
    cpu.regs_mut().set_reg(4, 0x44);
    cpu.regs_mut().set_reg(5, 0x55);
    cpu.regs_mut().set_lr(0xEEEE);

    cpu.step().unwrap();

    let mem = cpu.memory();
    assert_eq!(cpu.regs().sp(), 0x8FFF0);
    assert_eq!(mem.read32(GuestAddr::new(0x8FFF0)).unwrap(), 0x33);
    assert_eq!(mem.read32(GuestAddr::new(0x8FFF4)).unwrap(), 0x44);
    assert_eq!(mem.read32(GuestAddr::new(0x8FFF8)).unwrap(), 0x55);
    assert_eq!(mem.read32(GuestAddr::new(0x8FFFC)).unwrap(), 0xEEEE);
}

#[test]
fn pop_like_block_transfer_restores_registers_and_pc() {
    let mut cpu = new_arm_cpu(0xE8BD8010);
    cpu.regs_mut().set_sp(0x8FFF8);
    cpu.memory_mut()
        .write32(GuestAddr::new(0x8FFF8), 0x4444_4444)
        .unwrap();
    cpu.memory_mut()
        .write32(GuestAddr::new(0x8FFFC), 0x81234)
        .unwrap();

    cpu.step().unwrap();

    assert_eq!(cpu.regs().reg(4), 0x4444_4444);
    assert_eq!(cpu.regs().pc(), 0x81234);
    assert_eq!(cpu.regs().sp(), 0x90000);
}

#[test]
fn literal_load_reads_word_from_pc_relative_address() {
    let mut cpu = new_arm_cpu(0xE59F410C);
    cpu.memory_mut()
        .write32(GuestAddr::new(0x80114), 0x1122_3344)
        .unwrap();

    cpu.step().unwrap();

    assert_eq!(cpu.regs().reg(4), 0x1122_3344);
    assert_eq!(cpu.regs().pc(), 0x80004);
}

#[test]
fn offset_load_reads_word_from_base_minus_offset() {
    let mut cpu = new_arm_cpu(0xE5141008);
    cpu.regs_mut().set_reg(4, 0x88010);
    cpu.memory_mut()
        .write32(GuestAddr::new(0x88008), 0xAABB_CCDD)
        .unwrap();

    cpu.step().unwrap();

    assert_eq!(cpu.regs().reg(1), 0xAABB_CCDD);
    assert_eq!(cpu.regs().pc(), 0x80004);
}

#[test]
fn offset_store_writes_word_to_base_plus_offset() {
    let mut cpu = new_arm_cpu(0xE5810008);
    cpu.regs_mut().set_reg(0, 1);
    cpu.regs_mut().set_reg(1, 0x88000);

    cpu.step().unwrap();

    assert_eq!(cpu.memory().read32(GuestAddr::new(0x88008)).unwrap(), 1);
    assert_eq!(cpu.regs().pc(), 0x80004);
}

#[test]
fn byte_post_index_store_writes_only_low_byte_and_updates_base() {
    let mut cpu = new_arm_cpu(0xE4C13001);
    cpu.regs_mut().set_reg(1, 0x88000);
    cpu.regs_mut().set_reg(3, 0xAABB_CCDD);
    cpu.memory_mut().write8(GuestAddr::new(0x88000), 0x11).unwrap();
    cpu.memory_mut().write8(GuestAddr::new(0x88001), 0x22).unwrap();
    cpu.memory_mut().write8(GuestAddr::new(0x88002), 0x33).unwrap();
    cpu.memory_mut().write8(GuestAddr::new(0x88003), 0x44).unwrap();

    cpu.step().unwrap();

    assert_eq!(cpu.memory().read8(GuestAddr::new(0x88000)).unwrap(), 0xDD);
    assert_eq!(cpu.memory().read8(GuestAddr::new(0x88001)).unwrap(), 0x22);
    assert_eq!(cpu.memory().read8(GuestAddr::new(0x88002)).unwrap(), 0x33);
    assert_eq!(cpu.memory().read8(GuestAddr::new(0x88003)).unwrap(), 0x44);
    assert_eq!(cpu.regs().reg(1), 0x88001);
    assert_eq!(cpu.regs().pc(), 0x80004);
}

#[test]
fn conditional_register_offset_byte_store_writes_low_byte_when_condition_passes() {
    let mut cpu = new_arm_cpu(0x37C10002);
    cpu.regs_mut().set_reg(0, 0x1234_5678);
    cpu.regs_mut().set_reg(1, 0x88000);
    cpu.regs_mut().set_reg(2, 2);
    cpu.regs_mut().cpsr_mut().set_carry(false);

    cpu.step().unwrap();

    assert_eq!(cpu.memory().read8(GuestAddr::new(0x88002)).unwrap(), 0x78);
    assert_eq!(cpu.regs().reg(1), 0x88000);
    assert_eq!(cpu.regs().pc(), 0x80004);
}

#[test]
fn conditional_post_index_store_updates_memory_and_base_when_taken() {
    let mut cpu = new_arm_cpu(0x14804004);
    cpu.regs_mut().set_reg(0, 0x88010);
    cpu.regs_mut().set_reg(4, 0xAABB_CCDD);
    cpu.regs_mut().cpsr_mut().set_zero(false);

    cpu.step().unwrap();

    assert_eq!(
        cpu.memory().read32(GuestAddr::new(0x88010)).unwrap(),
        0xAABB_CCDD
    );
    assert_eq!(cpu.regs().reg(0), 0x88014);
    assert_eq!(cpu.regs().pc(), 0x80004);
}

#[test]
fn conditional_post_index_store_skips_when_condition_fails() {
    let mut cpu = new_arm_cpu(0x14804004);
    cpu.regs_mut().set_reg(0, 0x88010);
    cpu.regs_mut().set_reg(4, 0xAABB_CCDD);
    cpu.regs_mut().cpsr_mut().set_zero(true);

    cpu.step().unwrap();

    assert_eq!(cpu.memory().read32(GuestAddr::new(0x88010)).unwrap(), 0);
    assert_eq!(cpu.regs().reg(0), 0x88010);
    assert_eq!(cpu.regs().pc(), 0x80004);
}

#[test]
fn register_add_uses_arm_visible_pc() {
    let mut cpu = new_arm_cpu(0xE08F4004);
    cpu.regs_mut().set_reg(4, 0x100);

    cpu.step().unwrap();

    assert_eq!(cpu.regs().reg(4), 0x80108);
    assert_eq!(cpu.regs().pc(), 0x80004);
}

#[test]
fn ne_branch_taken_when_zero_clear() {
    let mut cpu = new_arm_cpu(0x1A00000F);
    cpu.regs_mut().cpsr_mut().set_zero(false);

    cpu.step().unwrap();

    assert_eq!(cpu.regs().pc(), 0x80044);
}

#[test]
fn ne_branch_not_taken_when_zero_set() {
    let mut cpu = new_arm_cpu(0x1A00000F);
    cpu.regs_mut().cpsr_mut().set_zero(true);

    cpu.step().unwrap();

    assert_eq!(cpu.regs().pc(), 0x80004);
}

#[test]
fn ldmia_without_writeback_loads_registers_and_advances_pc() {
    let mut cpu = new_arm_cpu(0xE8960007);
    cpu.regs_mut().set_reg(6, 0x88000);
    cpu.memory_mut()
        .write32(GuestAddr::new(0x88000), 0x11)
        .unwrap();
    cpu.memory_mut()
        .write32(GuestAddr::new(0x88004), 0x22)
        .unwrap();
    cpu.memory_mut()
        .write32(GuestAddr::new(0x88008), 0x33)
        .unwrap();

    cpu.step().unwrap();

    assert_eq!(cpu.regs().reg(0), 0x11);
    assert_eq!(cpu.regs().reg(1), 0x22);
    assert_eq!(cpu.regs().reg(2), 0x33);
    assert_eq!(cpu.regs().reg(6), 0x88000);
    assert_eq!(cpu.regs().pc(), 0x80004);
}

#[test]
fn stmib_sp_without_writeback_stores_r2_r3() {
    let mut cpu = new_arm_cpu(0xE98D000C);
    cpu.regs_mut().set_sp(0x90000);
    cpu.regs_mut().set_reg(2, 0x2222_2222);
    cpu.regs_mut().set_reg(3, 0x3333_3333);

    cpu.step().unwrap();

    assert_eq!(
        cpu.memory().read32(GuestAddr::new(0x90004)).unwrap(),
        0x2222_2222
    );
    assert_eq!(
        cpu.memory().read32(GuestAddr::new(0x90008)).unwrap(),
        0x3333_3333
    );
    assert_eq!(cpu.regs().sp(), 0x90000);
    assert_eq!(cpu.regs().pc(), 0x80004);
}

#[test]
fn smull_writes_high_and_low_words() {
    let mut cpu = new_arm_cpu(0xE0C32190);
    cpu.regs_mut().set_reg(0, 0xFFFF_0000);
    cpu.regs_mut().set_reg(1, 0x0000_0100);

    cpu.step().unwrap();

    assert_eq!(cpu.regs().reg(2), 0xFF00_0000);
    assert_eq!(cpu.regs().reg(3), 0xFFFF_FFFF);
    assert_eq!(cpu.regs().pc(), 0x80004);
}


