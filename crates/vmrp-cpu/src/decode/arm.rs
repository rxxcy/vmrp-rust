use super::{Condition, DataProcessingOp, DecodedInstruction, RegisterShift};

const BLOCK_TRANSFER_TAG: u32 = 0b100 << 25;
const DATA_PROCESSING_IMMEDIATE_TAG: u32 = 0b001 << 25;
const SINGLE_DATA_TRANSFER_IMMEDIATE_TAG: u32 = 0b010 << 25;
const BRANCH_TAG: u32 = 0b101 << 25;
const BX_MASK: u32 = 0x0FFF_FFF0;
const BX_VALUE: u32 = 0x012F_FF10;
const BLX_VALUE: u32 = 0x012F_FF30;
const MULTIPLY_LONG_MASK: u32 = 0x0F80_00F0;
const MULTIPLY_LONG_VALUE: u32 = 0x0080_0090;

pub fn decode(opcode: u32) -> DecodedInstruction {
    if opcode & BX_MASK == BX_VALUE {
        return DecodedInstruction::BranchExchange {
            link: false,
            register: (opcode & 0xF) as usize,
        };
    }

    if opcode & BX_MASK == BLX_VALUE {
        return DecodedInstruction::BranchExchange {
            link: true,
            register: (opcode & 0xF) as usize,
        };
    }

    if opcode & MULTIPLY_LONG_MASK == MULTIPLY_LONG_VALUE {
        return decode_multiply_long(opcode);
    }

    match opcode & (0b111 << 25) {
        BLOCK_TRANSFER_TAG => decode_block_transfer(opcode),
        DATA_PROCESSING_IMMEDIATE_TAG => decode_data_processing_immediate(opcode),
        SINGLE_DATA_TRANSFER_IMMEDIATE_TAG => decode_single_data_transfer_immediate(opcode),
        BRANCH_TAG => decode_branch(opcode),
        _ => decode_data_processing_register(opcode),
    }
}


fn decode_multiply_long(opcode: u32) -> DecodedInstruction {
    DecodedInstruction::MultiplyLong {
        signed: ((opcode >> 22) & 1) != 0,
        accumulate: ((opcode >> 21) & 1) != 0,
        set_flags: ((opcode >> 20) & 1) != 0,
        rd_hi: ((opcode >> 16) & 0xF) as usize,
        rd_lo: ((opcode >> 12) & 0xF) as usize,
        rs: ((opcode >> 8) & 0xF) as usize,
        rm: (opcode & 0xF) as usize,
    }
}
fn decode_block_transfer(opcode: u32) -> DecodedInstruction {
    DecodedInstruction::BlockTransfer {
        load: ((opcode >> 20) & 1) != 0,
        pre_index: ((opcode >> 24) & 1) != 0,
        add_offset: ((opcode >> 23) & 1) != 0,
        write_back: ((opcode >> 21) & 1) != 0,
        base: ((opcode >> 16) & 0xF) as usize,
        register_mask: (opcode & 0xFFFF) as u16,
    }
}

fn decode_data_processing_immediate(opcode: u32) -> DecodedInstruction {
    let op = match (opcode >> 21) & 0xF {
        0b1101 => DataProcessingOp::Mov,
        0b0000 => DataProcessingOp::And,
        0b1100 => DataProcessingOp::Orr,
        0b0100 => DataProcessingOp::Add,
        0b0010 => DataProcessingOp::Sub,
        0b1010 => DataProcessingOp::Cmp,
        0b1011 => DataProcessingOp::Cmn,
        _ => return DecodedInstruction::Unknown { opcode },
    };

    let set_flags = ((opcode >> 20) & 1) != 0;
    let rn = ((opcode >> 16) & 0xF) as usize;
    let rd = ((opcode >> 12) & 0xF) as usize;
    let rotate = (((opcode >> 8) & 0xF) * 2) as u32;
    let imm8 = opcode & 0xFF;
    let immediate = imm8.rotate_right(rotate);

    DecodedInstruction::DataProcessingImmediate {
        op,
        set_flags,
        rn,
        rd,
        immediate,
    }
}

fn decode_data_processing_register(opcode: u32) -> DecodedInstruction {
    if ((opcode >> 26) & 0x3) != 0 || ((opcode >> 25) & 1) != 0 {
        return DecodedInstruction::Unknown { opcode };
    }

    let op = match (opcode >> 21) & 0xF {
        0b0000 => DataProcessingOp::And,
        0b1100 => DataProcessingOp::Orr,
        0b0100 => DataProcessingOp::Add,
        0b0010 => DataProcessingOp::Sub,
        0b1101 => DataProcessingOp::Mov,
        _ => return DecodedInstruction::Unknown { opcode },
    };

    // Register-specified shift is not implemented yet.
    if ((opcode >> 4) & 1) != 0 {
        return DecodedInstruction::Unknown { opcode };
    }

    let shift_imm = ((opcode >> 7) & 0x1F) as u8;
    let shift = match (opcode >> 5) & 0x3 {
        0b00 => RegisterShift::Lsl(shift_imm),
        0b01 => RegisterShift::Lsr(if shift_imm == 0 { 32 } else { shift_imm }),
        0b10 => RegisterShift::Asr(if shift_imm == 0 { 32 } else { shift_imm }),
        0b11 => RegisterShift::Ror(shift_imm),
        _ => return DecodedInstruction::Unknown { opcode },
    };

    DecodedInstruction::DataProcessingRegister {
        op,
        set_flags: ((opcode >> 20) & 1) != 0,
        rn: ((opcode >> 16) & 0xF) as usize,
        rd: ((opcode >> 12) & 0xF) as usize,
        rm: (opcode & 0xF) as usize,
        shift,
    }
}

fn decode_single_data_transfer_immediate(opcode: u32) -> DecodedInstruction {
    DecodedInstruction::SingleDataTransferImmediate {
        load: ((opcode >> 20) & 1) != 0,
        base: ((opcode >> 16) & 0xF) as usize,
        rd: ((opcode >> 12) & 0xF) as usize,
        offset: opcode & 0xFFF,
        add_offset: ((opcode >> 23) & 1) != 0,
        pre_index: ((opcode >> 24) & 1) != 0,
        write_back: ((opcode >> 21) & 1) != 0,
    }
}

fn decode_branch(opcode: u32) -> DecodedInstruction {
    let link = ((opcode >> 24) & 1) != 0;
    let offset = (((opcode & 0x00FF_FFFF) << 8) as i32) >> 6;

    DecodedInstruction::Branch {
        condition: Condition::from_bits(((opcode >> 28) & 0xF) as u8),
        link,
        offset,
    }
}
