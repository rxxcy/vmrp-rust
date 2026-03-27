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
