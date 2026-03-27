const SP_INDEX: usize = 13;
const LR_INDEX: usize = 14;
const PC_INDEX: usize = 15;
const CPSR_T_MASK: u32 = 1 << 5;
const CPSR_V_MASK: u32 = 1 << 28;
const CPSR_C_MASK: u32 = 1 << 29;
const CPSR_Z_MASK: u32 = 1 << 30;
const CPSR_N_MASK: u32 = 1 << 31;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionMode {
    Arm,
    Thumb,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cpsr {
    bits: u32,
}

impl Cpsr {
    pub const fn bits(self) -> u32 {
        self.bits
    }

    pub fn set_bits(&mut self, bits: u32) {
        self.bits = bits;
    }

    pub fn execution_mode(self) -> ExecutionMode {
        if self.bits & CPSR_T_MASK != 0 {
            ExecutionMode::Thumb
        } else {
            ExecutionMode::Arm
        }
    }

    pub fn set_execution_mode(&mut self, mode: ExecutionMode) {
        match mode {
            ExecutionMode::Arm => self.bits &= !CPSR_T_MASK,
            ExecutionMode::Thumb => self.bits |= CPSR_T_MASK,
        }
    }

    pub fn zero(self) -> bool {
        self.bits & CPSR_Z_MASK != 0
    }

    pub fn negative(self) -> bool {
        self.bits & CPSR_N_MASK != 0
    }

    pub fn carry(self) -> bool {
        self.bits & CPSR_C_MASK != 0
    }

    pub fn overflow(self) -> bool {
        self.bits & CPSR_V_MASK != 0
    }

    pub fn set_zero(&mut self, value: bool) {
        if value {
            self.bits |= CPSR_Z_MASK;
        } else {
            self.bits &= !CPSR_Z_MASK;
        }
    }

    pub fn set_negative(&mut self, value: bool) {
        if value {
            self.bits |= CPSR_N_MASK;
        } else {
            self.bits &= !CPSR_N_MASK;
        }
    }

    pub fn set_carry(&mut self, value: bool) {
        if value {
            self.bits |= CPSR_C_MASK;
        } else {
            self.bits &= !CPSR_C_MASK;
        }
    }

    pub fn set_overflow(&mut self, value: bool) {
        if value {
            self.bits |= CPSR_V_MASK;
        } else {
            self.bits &= !CPSR_V_MASK;
        }
    }

    pub fn update_nz(&mut self, value: u32) {
        self.set_zero(value == 0);
        self.set_negative(value & CPSR_N_MASK != 0);
    }

    pub fn update_nzcv_add(&mut self, lhs: u32, rhs: u32, result: u32) {
        self.update_nz(result);
        self.set_carry((lhs as u64 + rhs as u64) > (u32::MAX as u64));
        let overflow = ((lhs ^ result) & (rhs ^ result) & 0x8000_0000) != 0;
        self.set_overflow(overflow);
    }

    pub fn update_nzcv_sub(&mut self, lhs: u32, rhs: u32, result: u32) {
        self.update_nz(result);
        self.set_carry(lhs >= rhs);
        let overflow = ((lhs ^ rhs) & (lhs ^ result) & 0x8000_0000) != 0;
        self.set_overflow(overflow);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CpuRegs {
    regs: [u32; 16],
    cpsr: Cpsr,
}

impl Default for CpuRegs {
    fn default() -> Self {
        Self {
            regs: [0; 16],
            cpsr: Cpsr::default(),
        }
    }
}

impl CpuRegs {
    pub fn reg(&self, index: usize) -> u32 {
        self.regs[index]
    }

    pub fn set_reg(&mut self, index: usize, value: u32) {
        self.regs[index] = value;
    }

    pub fn sp(&self) -> u32 {
        self.reg(SP_INDEX)
    }

    pub fn set_sp(&mut self, value: u32) {
        self.set_reg(SP_INDEX, value);
    }

    pub fn lr(&self) -> u32 {
        self.reg(LR_INDEX)
    }

    pub fn set_lr(&mut self, value: u32) {
        self.set_reg(LR_INDEX, value);
    }

    pub fn pc(&self) -> u32 {
        self.reg(PC_INDEX)
    }

    pub fn set_pc(&mut self, value: u32) {
        self.set_reg(PC_INDEX, value);
    }

    pub fn cpsr(&self) -> Cpsr {
        self.cpsr
    }

    pub fn cpsr_mut(&mut self) -> &mut Cpsr {
        &mut self.cpsr
    }

    pub fn set_cpsr(&mut self, cpsr: Cpsr) {
        self.cpsr = cpsr;
    }

    pub fn execution_mode(&self) -> ExecutionMode {
        self.cpsr.execution_mode()
    }

    pub fn set_execution_mode(&mut self, mode: ExecutionMode) {
        self.cpsr.set_execution_mode(mode);
    }
}
