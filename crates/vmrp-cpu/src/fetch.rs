use vmrp_core::GuestAddr;

use crate::MemoryAccessError;
use crate::{CpuRegs, ExecutionMode, MemoryBus};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FetchedOpcode {
    pub pc: u32,
    pub mode: ExecutionMode,
    pub opcode: u32,
}

pub fn fetch_opcode<B: MemoryBus>(
    memory: &B,
    regs: &CpuRegs,
) -> Result<FetchedOpcode, MemoryAccessError> {
    let pc = regs.pc();
    let mode = regs.execution_mode();
    let opcode = match mode {
        ExecutionMode::Arm => memory.read32(GuestAddr::new(pc))?,
        ExecutionMode::Thumb => memory.read16(GuestAddr::new(pc))? as u32,
    };

    Ok(FetchedOpcode { pc, mode, opcode })
}
