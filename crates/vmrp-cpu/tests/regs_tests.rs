use vmrp_cpu::{CpuRegs, ExecutionMode};

#[test]
fn pc_sp_lr_aliases_round_trip() {
    let mut regs = CpuRegs::default();
    regs.set_sp(0x2000);
    regs.set_lr(0x3000);
    regs.set_pc(0x4000);
    assert_eq!(regs.sp(), 0x2000);
    assert_eq!(regs.lr(), 0x3000);
    assert_eq!(regs.pc(), 0x4000);
}

#[test]
fn thumb_mode_is_derived_from_cpsr_t_bit() {
    let mut regs = CpuRegs::default();
    regs.set_execution_mode(ExecutionMode::Thumb);
    assert_eq!(regs.execution_mode(), ExecutionMode::Thumb);
}
