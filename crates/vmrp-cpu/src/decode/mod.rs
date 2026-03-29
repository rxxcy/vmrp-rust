use crate::ExecutionMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Condition {
    Eq,
    Ne,
    Al,
    Other(u8),
}

impl Condition {
    pub fn from_bits(bits: u8) -> Self {
        match bits {
            0x0 => Self::Eq,
            0x1 => Self::Ne,
            0xE => Self::Al,
            other => Self::Other(other),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataProcessingOp {
    Mov,
    And,
    Orr,
    Add,
    Sub,
    Cmp,
    Cmn,
    Mvn,
    Bic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegisterShift {
    Lsl(u8),
    Lsr(u8),
    Asr(u8),
    Ror(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThumbOperand {
    Immediate(u32),
    Register(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThumbAluOp {
    Cmp,
    Mvn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThumbHiOp {
    Add,
    Cmp,
    Mov,
    Bx,
    Blx,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodedInstruction {
    BlockTransfer {
        load: bool,
        pre_index: bool,
        add_offset: bool,
        write_back: bool,
        base: usize,
        register_mask: u16,
    },
    Branch {
        condition: Condition,
        link: bool,
        offset: i32,
    },
    BranchExchange {
        link: bool,
        register: usize,
    },
    BranchLinkExchangeImmediate {
        offset: i32,
    },
    DataProcessingImmediate {
        op: DataProcessingOp,
        set_flags: bool,
        rn: usize,
        rd: usize,
        immediate: u32,
    },
    DataProcessingRegister {
        op: DataProcessingOp,
        set_flags: bool,
        rn: usize,
        rd: usize,
        rm: usize,
        shift: RegisterShift,
    },
    MultiplyLong {
        signed: bool,
        accumulate: bool,
        set_flags: bool,
        rd_hi: usize,
        rd_lo: usize,
        rm: usize,
        rs: usize,
    },
    SingleDataTransferImmediate {
        load: bool,
        byte: bool,
        base: usize,
        rd: usize,
        offset: u32,
        add_offset: bool,
        pre_index: bool,
        write_back: bool,
    },
    SingleDataTransferRegister {
        load: bool,
        byte: bool,
        base: usize,
        rd: usize,
        rm: usize,
        shift: RegisterShift,
        add_offset: bool,
        pre_index: bool,
        write_back: bool,
    },
    ThumbAddImmediate {
        rd: usize,
        immediate: u32,
    },
    ThumbAddSub {
        sub: bool,
        rd: usize,
        rs: usize,
        operand: ThumbOperand,
    },
    ThumbAdjustSp {
        subtract: bool,
        immediate: u32,
    },
    ThumbAluRegister {
        op: ThumbAluOp,
        rd: usize,
        rs: usize,
    },
    ThumbBranch {
        offset: i32,
    },
    ThumbCmpImmediate {
        rn: usize,
        immediate: u32,
    },
    ThumbConditionalBranch {
        condition: Condition,
        offset: i32,
    },
    ThumbShiftImmediate {
        rd: usize,
        rs: usize,
        shift: RegisterShift,
    },
    ThumbHiRegisterOp {
        op: ThumbHiOp,
        rd: usize,
        rs: usize,
    },
    ThumbLiteralLoad {
        rd: usize,
        offset: u32,
    },
    ThumbLoadAddress {
        sp: bool,
        rd: usize,
        offset: u32,
    },
    ThumbLoadStoreRegisterOffset {
        load: bool,
        byte: bool,
        offset: usize,
        base: usize,
        rd: usize,
    },
    ThumbLoadStoreMultiple {
        load: bool,
        base: usize,
        register_mask: u8,
    },
    ThumbLoadStoreSpRelative {
        load: bool,
        rd: usize,
        offset: u32,
    },
    ThumbLoadStoreByteImmediate {
        load: bool,
        base: usize,
        rd: usize,
        offset: u32,
    },
    ThumbLoadStoreWordImmediate {
        load: bool,
        base: usize,
        rd: usize,
        offset: u32,
    },
    ThumbLongBranchPrefix {
        offset: i32,
    },
    ThumbLongBranchSuffix {
        exchange: bool,
        offset: u32,
    },
    ThumbMovImmediate {
        rd: usize,
        immediate: u32,
    },
    ThumbPop {
        register_mask: u8,
        include_pc: bool,
    },
    ThumbPush {
        register_mask: u8,
        include_lr: bool,
    },
    ThumbSubImmediate {
        rd: usize,
        immediate: u32,
    },
    Unknown {
        opcode: u32,
    },
}

pub fn decode_arm_opcode(opcode: u32) -> DecodedInstruction {
    arm::decode(opcode)
}

pub fn decode_opcode(mode: ExecutionMode, opcode: u32) -> DecodedInstruction {
    match mode {
        ExecutionMode::Arm => arm::decode(opcode),
        ExecutionMode::Thumb => thumb::decode(opcode as u16),
    }
}

pub mod arm;
pub mod thumb;




