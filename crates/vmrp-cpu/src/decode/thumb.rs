use super::DecodedInstruction;

pub fn decode(opcode: u16) -> DecodedInstruction {
    match opcode & 0xF800 {
        0x2000 => DecodedInstruction::ThumbMovImmediate {
            rd: ((opcode >> 8) & 0x7) as usize,
            immediate: (opcode & 0xFF) as u32,
        },
        0x3000 => DecodedInstruction::ThumbAddImmediate {
            rd: ((opcode >> 8) & 0x7) as usize,
            immediate: (opcode & 0xFF) as u32,
        },
        _ => DecodedInstruction::Unknown {
            opcode: opcode as u32,
        },
    }
}
