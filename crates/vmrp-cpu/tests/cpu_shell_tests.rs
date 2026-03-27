use vmrp_core::GuestAddr;
use vmrp_cpu::{Cpu, ExecutionMode, TestMemory};

#[test]
fn arm_fetch_reads_and_executes_32_bit_opcode() {
    let mut mem = TestMemory::with_ram(GuestAddr::new(0x80000), 0x1000);
    vmrp_cpu::MemoryBus::write32(&mut mem, GuestAddr::new(0x80000), 0xE1A0_0000).unwrap();

    let mut cpu = Cpu::new(mem);
    cpu.regs_mut().set_pc(0x80000);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_reg(0, 0x1234_5678);

    let step = cpu.step().unwrap();

    assert_eq!(step.trace.opcode, 0xE1A0_0000);
    assert_eq!(cpu.regs().reg(0), 0x1234_5678);
    assert_eq!(cpu.regs().pc(), 0x80004);
}
