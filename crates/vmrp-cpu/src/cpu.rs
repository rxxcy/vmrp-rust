use crate::decode::decode_opcode;
use crate::execute::{execute_instruction, StepResult};
use crate::fetch::fetch_opcode;
use crate::{CpuRegs, ExecutionMode, MemoryAccessError, MemoryBus};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuError {
    Memory(MemoryAccessError),
    StepLimitExceeded {
        steps: usize,
        last_pc: u32,
    },
    UnimplementedInstruction {
        pc: u32,
        mode: ExecutionMode,
        opcode: u32,
    },
}

impl From<MemoryAccessError> for CpuError {
    fn from(value: MemoryAccessError) -> Self {
        Self::Memory(value)
    }
}

#[derive(Clone, Debug)]
pub struct Cpu<B> {
    memory: B,
    regs: CpuRegs,
}

impl<B> Cpu<B> {
    pub fn new(memory: B) -> Self {
        Self {
            memory,
            regs: CpuRegs::default(),
        }
    }

    pub fn regs(&self) -> &CpuRegs {
        &self.regs
    }

    pub fn regs_mut(&mut self) -> &mut CpuRegs {
        &mut self.regs
    }

    pub fn memory(&self) -> &B {
        &self.memory
    }

    pub fn memory_mut(&mut self) -> &mut B {
        &mut self.memory
    }
}

impl<B: MemoryBus> Cpu<B> {
    pub fn step(&mut self) -> Result<StepResult, CpuError> {
        let fetched = fetch_opcode(&self.memory, &self.regs)?;
        let decoded = decode_opcode(fetched.mode, fetched.opcode);
        execute_instruction(&mut self.memory, &mut self.regs, decoded, fetched.pc, fetched.mode, fetched.opcode)
    }

    pub fn run_until(&mut self, max_steps: usize) -> Result<(), CpuError> {
        for _ in 0..max_steps {
            self.step()?;
        }

        Err(CpuError::StepLimitExceeded {
            steps: max_steps,
            last_pc: self.regs.pc(),
        })
    }
}


