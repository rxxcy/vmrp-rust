use super::{Condition, DecodedInstruction, ThumbAluOp, ThumbHiOp, ThumbOperand};
use crate::decode::RegisterShift;

pub fn decode(opcode: u16) -> DecodedInstruction {
    match opcode & 0xF800 {
        0x0000 | 0x0800 | 0x1000 => decode_shift_immediate(opcode),
        0x1800 => decode_add_sub(opcode),
        0x2000 => DecodedInstruction::ThumbMovImmediate {
            rd: ((opcode >> 8) & 0x7) as usize,
            immediate: (opcode & 0xFF) as u32,
        },
        0x2800 => DecodedInstruction::ThumbCmpImmediate {
            rn: ((opcode >> 8) & 0x7) as usize,
            immediate: (opcode & 0xFF) as u32,
        },
        0x3000 => DecodedInstruction::ThumbAddImmediate {
            rd: ((opcode >> 8) & 0x7) as usize,
            immediate: (opcode & 0xFF) as u32,
        },
        0x3800 => DecodedInstruction::ThumbSubImmediate {
            rd: ((opcode >> 8) & 0x7) as usize,
            immediate: (opcode & 0xFF) as u32,
        },
        0x4800 => DecodedInstruction::ThumbLiteralLoad {
            rd: ((opcode >> 8) & 0x7) as usize,
            offset: ((opcode & 0xFF) as u32) << 2,
        },
        0x9000 | 0x9800 => DecodedInstruction::ThumbLoadStoreSpRelative {
            load: ((opcode >> 11) & 1) != 0,
            rd: ((opcode >> 8) & 0x7) as usize,
            offset: ((opcode & 0xFF) as u32) << 2,
        },
        0xA000 | 0xA800 => DecodedInstruction::ThumbLoadAddress {
            sp: ((opcode >> 11) & 1) != 0,
            rd: ((opcode >> 8) & 0x7) as usize,
            offset: ((opcode & 0xFF) as u32) << 2,
        },
        0xE000 => {
            let raw = ((opcode & 0x7FF) as i32) << 1;
            let offset = (raw << 20) >> 20;
            DecodedInstruction::ThumbBranch { offset }
        }
        0xF000 => {
            let raw = ((opcode & 0x7FF) as i32) << 12;
            let offset = (raw << 9) >> 9;
            DecodedInstruction::ThumbLongBranchPrefix { offset }
        }
        0xF800 => DecodedInstruction::ThumbLongBranchSuffix {
            exchange: false,
            offset: ((opcode & 0x7FF) as u32) << 1,
        },
        _ => decode_fallback(opcode),
    }
}

fn decode_fallback(opcode: u16) -> DecodedInstruction {
    if opcode & 0xFC00 == 0x4000 {
        return decode_alu_register(opcode);
    }

    if opcode & 0xFC00 == 0x4400 {
        return decode_hi_register(opcode);
    }

    if opcode & 0xF200 == 0x5000 {
        return DecodedInstruction::ThumbLoadStoreRegisterOffset {
            load: ((opcode >> 11) & 1) != 0,
            byte: ((opcode >> 10) & 1) != 0,
            offset: ((opcode >> 6) & 0x7) as usize,
            base: ((opcode >> 3) & 0x7) as usize,
            rd: (opcode & 0x7) as usize,
        };
    }

    if opcode & 0xF000 == 0x7000 {
        return DecodedInstruction::ThumbLoadStoreByteImmediate {
            load: ((opcode >> 11) & 1) != 0,
            base: ((opcode >> 3) & 0x7) as usize,
            rd: (opcode & 0x7) as usize,
            offset: ((opcode >> 6) & 0x1F) as u32,
        };
    }

    if opcode & 0xF000 == 0x6000 {
        return DecodedInstruction::ThumbLoadStoreWordImmediate {
            load: ((opcode >> 11) & 1) != 0,
            base: ((opcode >> 3) & 0x7) as usize,
            rd: (opcode & 0x7) as usize,
            offset: (((opcode >> 6) & 0x1F) as u32) << 2,
        };
    }

    if opcode & 0xFE00 == 0xB400 {
        return DecodedInstruction::ThumbPush {
            register_mask: (opcode & 0xFF) as u8,
            include_lr: ((opcode >> 8) & 1) != 0,
        };
    }

    if opcode & 0xFE00 == 0xBC00 {
        return DecodedInstruction::ThumbPop {
            register_mask: (opcode & 0xFF) as u8,
            include_pc: ((opcode >> 8) & 1) != 0,
        };
    }

    if opcode & 0xFF00 == 0xB000 {
        return DecodedInstruction::ThumbAdjustSp {
            subtract: ((opcode >> 7) & 1) != 0,
            immediate: ((opcode & 0x7F) as u32) << 2,
        };
    }

    if opcode & 0xF000 == 0xC000 {
        return DecodedInstruction::ThumbLoadStoreMultiple {
            load: ((opcode >> 11) & 1) != 0,
            base: ((opcode >> 8) & 0x7) as usize,
            register_mask: (opcode & 0xFF) as u8,
        };
    }

    if opcode & 0xF000 == 0xD000 && ((opcode >> 8) & 0xF) != 0xF {
        let cond = Condition::from_bits(((opcode >> 8) & 0xF) as u8);
        let offset = (((opcode & 0xFF) as i8 as i32) << 1) as i32;
        return DecodedInstruction::ThumbConditionalBranch {
            condition: cond,
            offset,
        };
    }

    if opcode & 0xF800 == 0xE800 {
        return DecodedInstruction::ThumbLongBranchSuffix {
            exchange: true,
            offset: ((opcode & 0x7FF) as u32) << 1,
        };
    }

    DecodedInstruction::Unknown {
        opcode: opcode as u32,
    }
}

fn decode_add_sub(opcode: u16) -> DecodedInstruction {
    let sub = ((opcode >> 9) & 1) != 0;
    let immediate = ((opcode >> 10) & 1) != 0;
    let rs = ((opcode >> 3) & 0x7) as usize;
    let rd = (opcode & 0x7) as usize;
    let operand = if immediate {
        ThumbOperand::Immediate(((opcode >> 6) & 0x7) as u32)
    } else {
        ThumbOperand::Register(((opcode >> 6) & 0x7) as usize)
    };

    DecodedInstruction::ThumbAddSub {
        sub,
        rd,
        rs,
        operand,
    }
}

fn decode_alu_register(opcode: u16) -> DecodedInstruction {
    let op = (opcode >> 6) & 0xF;
    let rs = ((opcode >> 3) & 0x7) as usize;
    let rd = (opcode & 0x7) as usize;

    match op {
        0xA => DecodedInstruction::ThumbAluRegister {
            op: ThumbAluOp::Cmp,
            rd,
            rs,
        },
        0xF => DecodedInstruction::ThumbAluRegister {
            op: ThumbAluOp::Mvn,
            rd,
            rs,
        },
        _ => DecodedInstruction::Unknown {
            opcode: opcode as u32,
        },
    }
}

fn decode_hi_register(opcode: u16) -> DecodedInstruction {
    let op = (opcode >> 8) & 0x3;
    let h1 = ((opcode >> 7) & 1) as usize;
    let h2 = ((opcode >> 6) & 1) as usize;
    let rs = (((opcode >> 3) & 0x7) as usize) | (h2 << 3);
    let rd = ((opcode & 0x7) as usize) | (h1 << 3);

    let op = match op {
        0 => ThumbHiOp::Add,
        1 => ThumbHiOp::Cmp,
        2 => ThumbHiOp::Mov,
        3 => {
            if h1 == 0 {
                ThumbHiOp::Bx
            } else {
                ThumbHiOp::Blx
            }
        }
        _ => unreachable!(),
    };

    DecodedInstruction::ThumbHiRegisterOp { op, rd, rs }
}

fn decode_shift_immediate(opcode: u16) -> DecodedInstruction {
    let shift = match (opcode >> 11) & 0x3 {
        0 => RegisterShift::Lsl(((opcode >> 6) & 0x1F) as u8),
        1 => {
            let imm = ((opcode >> 6) & 0x1F) as u8;
            RegisterShift::Lsr(if imm == 0 { 32 } else { imm })
        }
        2 => {
            let imm = ((opcode >> 6) & 0x1F) as u8;
            RegisterShift::Asr(if imm == 0 { 32 } else { imm })
        }
        _ => unreachable!(),
    };

    DecodedInstruction::ThumbShiftImmediate {
        rd: (opcode & 0x7) as usize,
        rs: ((opcode >> 3) & 0x7) as usize,
        shift,
    }
}
