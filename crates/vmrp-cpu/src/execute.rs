use crate::cpu::CpuError;
use crate::decode::{DataProcessingOp, DecodedInstruction, RegisterShift};
use crate::trace::{RegisterWrite, StepTrace};
use crate::{Cpsr, CpuRegs, ExecutionMode, MemoryBus};
use vmrp_core::GuestAddr;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepResult {
    pub trace: StepTrace,
}

pub fn execute_instruction<B: MemoryBus>(
    memory: &mut B,
    regs: &mut CpuRegs,
    instruction: DecodedInstruction,
    pc: u32,
    mode: ExecutionMode,
    opcode: u32,
) -> Result<StepResult, CpuError> {
    let mut register_writes = Vec::new();

    if mode == ExecutionMode::Arm {
        let cond = ((opcode >> 28) & 0xF) as u8;
        if !arm_condition_passed(regs.cpsr(), cond) {
            let next_pc = pc.wrapping_add(4);
            regs.set_pc(next_pc);
            register_writes.push(RegisterWrite {
                index: 15,
                value: next_pc,
            });
            return Ok(StepResult {
                trace: StepTrace {
                    pc,
                    mode,
                    opcode,
                    register_writes,
                },
            });
        }
    }

    match instruction {
        DecodedInstruction::BlockTransfer {
            load,
            pre_index,
            add_offset,
            write_back,
            base,
            register_mask,
        } => {
            let registers: Vec<usize> = (0..16)
                .filter(|index| (register_mask & (1 << index)) != 0)
                .collect();
            let count = registers.len() as u32;

            let base_value = regs.reg(base);
            let start = if add_offset {
                if pre_index {
                    base_value.wrapping_add(4)
                } else {
                    base_value
                }
            } else if pre_index {
                base_value.wrapping_sub(count * 4)
            } else {
                base_value.wrapping_sub((count.saturating_sub(1)) * 4)
            };

            let mut loaded_pc = false;
            for (slot, index) in registers.iter().enumerate() {
                let addr = GuestAddr::new(start.wrapping_add((slot as u32) * 4));
                if load {
                    let value = memory.read32(addr)?;
                    if *index == 15 {
                        regs.set_pc(value);
                        loaded_pc = true;
                    } else {
                        regs.set_reg(*index, value);
                    }
                    register_writes.push(RegisterWrite {
                        index: *index,
                        value,
                    });
                } else {
                    let value = regs.reg(*index);
                    memory.write32(addr, value)?;
                }
            }

            if write_back {
                let final_base = if add_offset {
                    base_value.wrapping_add(count * 4)
                } else {
                    base_value.wrapping_sub(count * 4)
                };
                regs.set_reg(base, final_base);
                register_writes.push(RegisterWrite {
                    index: base,
                    value: final_base,
                });
            }

            if !(load && loaded_pc) {
                let next_pc = pc.wrapping_add(4);
                regs.set_pc(next_pc);
                register_writes.push(RegisterWrite {
                    index: 15,
                    value: next_pc,
                });
            }
        }
        DecodedInstruction::BranchExchange { link, register } => {
            let target = regs.reg(register);
            if link {
                let lr = match mode {
                    ExecutionMode::Arm => pc.wrapping_add(4),
                    ExecutionMode::Thumb => pc.wrapping_add(2),
                };
                regs.set_lr(lr);
                register_writes.push(RegisterWrite {
                    index: 14,
                    value: lr,
                });
            }

            let next_mode = if target & 1 != 0 {
                ExecutionMode::Thumb
            } else {
                ExecutionMode::Arm
            };
            let next_pc = target & !1;
            regs.set_execution_mode(next_mode);
            regs.set_pc(next_pc);
            register_writes.push(RegisterWrite {
                index: 15,
                value: next_pc,
            });
        }
        DecodedInstruction::DataProcessingRegister {
            op,
            set_flags,
            rn,
            rd,
            rm,
            shift,
        } => {
            let left = if rn == 15 && mode == ExecutionMode::Arm {
                pc.wrapping_add(8)
            } else {
                regs.reg(rn)
            };
            let rm_value = if rm == 15 && mode == ExecutionMode::Arm {
                pc.wrapping_add(8)
            } else {
                regs.reg(rm)
            };
            let (right, shifter_carry) = apply_register_shift(rm_value, shift, regs.cpsr().carry());

            match op {
                DataProcessingOp::And => {
                    let result = left & right;
                    write_result_and_advance(regs, rd, result, pc.wrapping_add(4), &mut register_writes);
                    if set_flags {
                        regs.cpsr_mut().update_nz(result);
                        if let Some(carry) = shifter_carry {
                            regs.cpsr_mut().set_carry(carry);
                        }
                    }
                }
                DataProcessingOp::Orr => {
                    let result = left | right;
                    write_result_and_advance(regs, rd, result, pc.wrapping_add(4), &mut register_writes);
                    if set_flags {
                        regs.cpsr_mut().update_nz(result);
                        if let Some(carry) = shifter_carry {
                            regs.cpsr_mut().set_carry(carry);
                        }
                    }
                }
                DataProcessingOp::Add => {
                    let result = left.wrapping_add(right);
                    write_result_and_advance(regs, rd, result, pc.wrapping_add(4), &mut register_writes);
                    if set_flags {
                        regs.cpsr_mut().update_nzcv_add(left, right, result);
                    }
                }
                DataProcessingOp::Sub => {
                    let result = left.wrapping_sub(right);
                    write_result_and_advance(regs, rd, result, pc.wrapping_add(4), &mut register_writes);
                    if set_flags {
                        regs.cpsr_mut().update_nzcv_sub(left, right, result);
                    }
                }
                DataProcessingOp::Mov => {
                    write_result_and_advance(regs, rd, right, pc.wrapping_add(4), &mut register_writes);
                    if set_flags {
                        regs.cpsr_mut().update_nz(right);
                        if let Some(carry) = shifter_carry {
                            regs.cpsr_mut().set_carry(carry);
                        }
                    }
                }
                _ => {
                    return Err(CpuError::UnimplementedInstruction { pc, mode, opcode });
                }
            }
        }
        DecodedInstruction::MultiplyLong {
            signed,
            accumulate,
            set_flags,
            rd_hi,
            rd_lo,
            rm,
            rs,
        } => {
            let lhs = regs.reg(rm);
            let rhs = regs.reg(rs);
            let mut result = if signed {
                ((lhs as i32 as i64) as i128) * ((rhs as i32 as i64) as i128)
            } else {
                (lhs as u64 as i128) * (rhs as u64 as i128)
            };

            if accumulate {
                let acc = ((regs.reg(rd_hi) as u64) << 32) | (regs.reg(rd_lo) as u64);
                result += acc as i128;
            }

            let result_u64 = result as u64;
            let lo = result_u64 as u32;
            let hi = (result_u64 >> 32) as u32;
            regs.set_reg(rd_lo, lo);
            regs.set_reg(rd_hi, hi);
            register_writes.push(RegisterWrite { index: rd_lo, value: lo });
            register_writes.push(RegisterWrite { index: rd_hi, value: hi });

            if set_flags {
                regs.cpsr_mut().set_negative((result_u64 & 0x8000_0000_0000_0000) != 0);
                regs.cpsr_mut().set_zero(result_u64 == 0);
            }

            let next_pc = pc.wrapping_add(4);
            regs.set_pc(next_pc);
            register_writes.push(RegisterWrite {
                index: 15,
                value: next_pc,
            });
        }
        DecodedInstruction::SingleDataTransferImmediate {
            load,
            base,
            rd,
            offset,
            add_offset,
            pre_index,
            write_back,
        } => {
            let base_addr = if base == 15 && mode == ExecutionMode::Arm {
                pc.wrapping_add(8)
            } else {
                regs.reg(base)
            };
            let offset_addr = if add_offset {
                base_addr.wrapping_add(offset)
            } else {
                base_addr.wrapping_sub(offset)
            };
            let address = if pre_index { offset_addr } else { base_addr };

            if load {
                let value = memory.read32(GuestAddr::new(address))?;
                if rd == 15 {
                    regs.set_pc(value);
                    register_writes.push(RegisterWrite { index: 15, value });
                } else {
                    regs.set_reg(rd, value);
                    register_writes.push(RegisterWrite { index: rd, value });
                }
            } else {
                let value = regs.reg(rd);
                memory.write32(GuestAddr::new(address), value)?;
            }

            if !pre_index || write_back {
                regs.set_reg(base, offset_addr);
                register_writes.push(RegisterWrite {
                    index: base,
                    value: offset_addr,
                });
            }

            if !(load && rd == 15) {
                let next_pc = pc.wrapping_add(4);
                regs.set_pc(next_pc);
                register_writes.push(RegisterWrite {
                    index: 15,
                    value: next_pc,
                });
            }
        }
        DecodedInstruction::Branch {
            condition: _,
            link,
            offset,
        } => {
            if link {
                let lr = pc.wrapping_add(4);
                regs.set_lr(lr);
                register_writes.push(RegisterWrite {
                    index: 14,
                    value: lr,
                });
            }

            let target = (pc as i64) + 8 + (offset as i64);
            let next_pc = target as u32;
            regs.set_pc(next_pc);
            register_writes.push(RegisterWrite {
                index: 15,
                value: next_pc,
            });
        }
        DecodedInstruction::DataProcessingImmediate {
            op,
            set_flags,
            rn,
            rd,
            immediate,
        } => {
            match op {
                DataProcessingOp::Mov => {
                    write_result_and_advance(
                        regs,
                        rd,
                        immediate,
                        pc.wrapping_add(4),
                        &mut register_writes,
                    );
                    if set_flags {
                        regs.cpsr_mut().update_nz(immediate);
                    }
                }
                DataProcessingOp::And => {
                    let left = regs.reg(rn);
                    let result = left & immediate;
                    write_result_and_advance(regs, rd, result, pc.wrapping_add(4), &mut register_writes);
                    if set_flags {
                        regs.cpsr_mut().update_nz(result);
                    }
                }
                DataProcessingOp::Orr => {
                    let left = regs.reg(rn);
                    let result = left | immediate;
                    write_result_and_advance(regs, rd, result, pc.wrapping_add(4), &mut register_writes);
                    if set_flags {
                        regs.cpsr_mut().update_nz(result);
                    }
                }
                DataProcessingOp::Add => {
                    let left = regs.reg(rn);
                    let result = left.wrapping_add(immediate);
                    write_result_and_advance(regs, rd, result, pc.wrapping_add(4), &mut register_writes);
                    if set_flags {
                        regs.cpsr_mut().update_nzcv_add(left, immediate, result);
                    }
                }
                DataProcessingOp::Sub => {
                    let left = regs.reg(rn);
                    let result = left.wrapping_sub(immediate);
                    write_result_and_advance(regs, rd, result, pc.wrapping_add(4), &mut register_writes);
                    if set_flags {
                        regs.cpsr_mut().update_nzcv_sub(left, immediate, result);
                    }
                }
                DataProcessingOp::Cmp => {
                    let left = regs.reg(rn);
                    let result = left.wrapping_sub(immediate);
                    regs.cpsr_mut().update_nzcv_sub(left, immediate, result);
                    let next_pc = pc.wrapping_add(4);
                    regs.set_pc(next_pc);
                    register_writes.push(RegisterWrite {
                        index: 15,
                        value: next_pc,
                    });
                }
                DataProcessingOp::Cmn => {
                    let left = regs.reg(rn);
                    let result = left.wrapping_add(immediate);
                    regs.cpsr_mut().update_nzcv_add(left, immediate, result);
                    let next_pc = pc.wrapping_add(4);
                    regs.set_pc(next_pc);
                    register_writes.push(RegisterWrite {
                        index: 15,
                        value: next_pc,
                    });
                }
            }
        }
        DecodedInstruction::ThumbAddImmediate { rd, immediate } => {
            let result = regs.reg(rd).wrapping_add(immediate);
            regs.set_reg(rd, result);
            register_writes.push(RegisterWrite {
                index: rd,
                value: result,
            });
            regs.cpsr_mut().update_nz(result);
            let next_pc = pc.wrapping_add(2);
            regs.set_pc(next_pc);
            register_writes.push(RegisterWrite {
                index: 15,
                value: next_pc,
            });
        }
        DecodedInstruction::ThumbMovImmediate { rd, immediate } => {
            regs.set_reg(rd, immediate);
            register_writes.push(RegisterWrite {
                index: rd,
                value: immediate,
            });
            regs.cpsr_mut().update_nz(immediate);
            let next_pc = pc.wrapping_add(2);
            regs.set_pc(next_pc);
            register_writes.push(RegisterWrite {
                index: 15,
                value: next_pc,
            });
        }
        DecodedInstruction::Unknown { opcode } => {
            return Err(CpuError::UnimplementedInstruction { pc, mode, opcode });
        }
    }

    Ok(StepResult {
        trace: StepTrace {
            pc,
            mode,
            opcode,
            register_writes,
        },
    })
}

fn write_result_and_advance(
    regs: &mut CpuRegs,
    rd: usize,
    value: u32,
    next_pc: u32,
    register_writes: &mut Vec<RegisterWrite>,
) {
    if rd == 15 {
        regs.set_pc(value);
        register_writes.push(RegisterWrite { index: 15, value });
    } else {
        regs.set_reg(rd, value);
        register_writes.push(RegisterWrite { index: rd, value });
        regs.set_pc(next_pc);
        register_writes.push(RegisterWrite {
            index: 15,
            value: next_pc,
        });
    }
}

fn apply_register_shift(value: u32, shift: RegisterShift, carry_in: bool) -> (u32, Option<bool>) {
    match shift {
        RegisterShift::Lsl(0) => (value, None),
        RegisterShift::Lsl(n) if n < 32 => {
            let out = value.wrapping_shl(n as u32);
            let carry = ((value >> (32 - n)) & 1) != 0;
            (out, Some(carry))
        }
        RegisterShift::Lsl(32) => (0, Some((value & 1) != 0)),
        RegisterShift::Lsl(_) => (0, Some(false)),

        RegisterShift::Lsr(32) => (0, Some(((value >> 31) & 1) != 0)),
        RegisterShift::Lsr(n) if n < 32 => {
            let out = value >> n;
            let carry = ((value >> (n - 1)) & 1) != 0;
            (out, Some(carry))
        }
        RegisterShift::Lsr(_) => (0, Some(false)),

        RegisterShift::Asr(n) if n >= 32 => {
            if (value & 0x8000_0000) != 0 {
                (u32::MAX, Some(true))
            } else {
                (0, Some(false))
            }
        }
        RegisterShift::Asr(n) => {
            let out = ((value as i32) >> n) as u32;
            let carry = ((value >> (n - 1)) & 1) != 0;
            (out, Some(carry))
        }

        RegisterShift::Ror(0) => {
            let out = ((carry_in as u32) << 31) | (value >> 1);
            let carry = (value & 1) != 0;
            (out, Some(carry))
        }
        RegisterShift::Ror(n) => {
            let rot = (n as u32) % 32;
            let out = if rot == 0 { value } else { value.rotate_right(rot) };
            let carry = ((out >> 31) & 1) != 0;
            (out, Some(carry))
        }
    }
}

fn arm_condition_passed(cpsr: Cpsr, cond: u8) -> bool {
    match cond {
        0x0 => cpsr.zero(),
        0x1 => !cpsr.zero(),
        0x2 => cpsr.carry(),
        0x3 => !cpsr.carry(),
        0x4 => cpsr.negative(),
        0x5 => !cpsr.negative(),
        0x6 => cpsr.overflow(),
        0x7 => !cpsr.overflow(),
        0x8 => cpsr.carry() && !cpsr.zero(),
        0x9 => !cpsr.carry() || cpsr.zero(),
        0xA => cpsr.negative() == cpsr.overflow(),
        0xB => cpsr.negative() != cpsr.overflow(),
        0xC => !cpsr.zero() && (cpsr.negative() == cpsr.overflow()),
        0xD => cpsr.zero() || (cpsr.negative() != cpsr.overflow()),
        0xE => true,
        _ => false,
    }
}
