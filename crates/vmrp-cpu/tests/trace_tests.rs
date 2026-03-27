use vmrp_core::GuestAddr;
use vmrp_cpu::{Cpu, ExecutionMode, MemoryBus, TestMemory};

#[test]
fn step_trace_reports_pc_mode_opcode_and_register_write() {
    let mut mem = TestMemory::with_ram(GuestAddr::new(0x80000), 0x1000);
    mem.write32(GuestAddr::new(0x80000), 0xE3A0_0001).unwrap();

    let mut cpu = Cpu::new(mem);
    cpu.regs_mut().set_pc(0x80000);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);

    let step = cpu.step().unwrap();

    assert_eq!(step.trace.pc, 0x80000);
    assert_eq!(step.trace.mode, ExecutionMode::Arm);
    assert_eq!(step.trace.opcode, 0xE3A0_0001);
    assert_eq!(step.trace.register_writes.len(), 2);
    assert_eq!(step.trace.register_writes[0].index, 0);
    assert_eq!(step.trace.register_writes[0].value, 1);
}
