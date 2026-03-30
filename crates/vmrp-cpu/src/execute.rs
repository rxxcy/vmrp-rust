use crate::cpu::CpuError;
use crate::decode::{
    Condition, DataProcessingOp, DecodedInstruction, RegisterShift, ThumbAluOp, ThumbHiOp,
    ThumbOperand,
};
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

    if mode == ExecutionMode::Arm
        && !matches!(
            instruction,
            DecodedInstruction::BranchLinkExchangeImmediate { .. }
        )
    {
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
                    let loaded_value = if *index == 15 {
                        let next_mode = if value & 1 != 0 {
                            ExecutionMode::Thumb
                        } else {
                            ExecutionMode::Arm
                        };
                        let next_pc = if next_mode == ExecutionMode::Thumb {
                            value & !1
                        } else {
                            value & !3
                        };
                        regs.set_execution_mode(next_mode);
                        regs.set_pc(next_pc);
                        loaded_pc = true;
                        next_pc
                    } else {
                        regs.set_reg(*index, value);
                        value
                    };
                    register_writes.push(RegisterWrite {
                        index: *index,
                        value: loaded_value,
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
        DecodedInstruction::BranchLinkExchangeImmediate { offset } => {
            let lr = pc.wrapping_add(4);
            regs.set_lr(lr);
            register_writes.push(RegisterWrite {
                index: 14,
                value: lr,
            });

            let target = ((pc as i64) + 8 + (offset as i64)) as u32;
            regs.set_execution_mode(ExecutionMode::Thumb);
            regs.set_pc(target & !1);
            register_writes.push(RegisterWrite {
                index: 15,
                value: target & !1,
            });
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
            let next_pc = if next_mode == ExecutionMode::Thumb {
                target & !1
            } else {
                target & !3
            };
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
            let (right, shifter_carry) = apply_register_shift(rm_value, shift, regs.cpsr().carry(), regs);

            match op {
                DataProcessingOp::And => {
                    let result = left & right;
                    write_result_and_advance(
                        regs,
                        rd,
                        result,
                        pc.wrapping_add(4),
                        &mut register_writes,
                    );
                    if set_flags {
                        regs.cpsr_mut().update_nz(result);
                        if let Some(carry) = shifter_carry {
                            regs.cpsr_mut().set_carry(carry);
                        }
                    }
                }
                DataProcessingOp::Eor => {
                    let result = left ^ right;
                    write_result_and_advance(
                        regs,
                        rd,
                        result,
                        pc.wrapping_add(4),
                        &mut register_writes,
                    );
                    if set_flags {
                        regs.cpsr_mut().update_nz(result);
                        if let Some(carry) = shifter_carry {
                            regs.cpsr_mut().set_carry(carry);
                        }
                    }
                }
            DataProcessingOp::Tst => {
                    let result = left & right;
                    regs.cpsr_mut().update_nz(result);
                    if let Some(carry) = shifter_carry {
                        regs.cpsr_mut().set_carry(carry);
                    }
                    let next_pc = pc.wrapping_add(4);
                    regs.set_pc(next_pc);
                    register_writes.push(RegisterWrite {
                        index: 15,
                        value: next_pc,
                    });
                }
                DataProcessingOp::Orr => {
                    let result = left | right;
                    write_result_and_advance(
                        regs,
                        rd,
                        result,
                        pc.wrapping_add(4),
                        &mut register_writes,
                    );
                    if set_flags {
                        regs.cpsr_mut().update_nz(result);
                        if let Some(carry) = shifter_carry {
                            regs.cpsr_mut().set_carry(carry);
                        }
                    }
                }
                DataProcessingOp::Bic => {
                    let result = left & !right;
                    write_result_and_advance(
                        regs,
                        rd,
                        result,
                        pc.wrapping_add(4),
                        &mut register_writes,
                    );
                    if set_flags {
                        regs.cpsr_mut().update_nz(result);
                        if let Some(carry) = shifter_carry {
                            regs.cpsr_mut().set_carry(carry);
                        }
                    }
                }
                DataProcessingOp::Add => {
                    let result = left.wrapping_add(right);
                    write_result_and_advance(
                        regs,
                        rd,
                        result,
                        pc.wrapping_add(4),
                        &mut register_writes,
                    );
                    if set_flags {
                        regs.cpsr_mut().update_nzcv_add(left, right, result);
                    }
                }
                DataProcessingOp::Adc => {
                    let (result, carry, overflow) = add_with_carry(left, right, regs.cpsr().carry());
                    write_result_and_advance(
                        regs,
                        rd,
                        result,
                        pc.wrapping_add(4),
                        &mut register_writes,
                    );
                    if set_flags {
                        regs.cpsr_mut().update_nz(result);
                        regs.cpsr_mut().set_carry(carry);
                        regs.cpsr_mut().set_overflow(overflow);
                    }
                }
                DataProcessingOp::Sub => {
                    let result = left.wrapping_sub(right);
                    write_result_and_advance(
                        regs,
                        rd,
                        result,
                        pc.wrapping_add(4),
                        &mut register_writes,
                    );
                    if set_flags {
                        regs.cpsr_mut().update_nzcv_sub(left, right, result);
                    }
                }
                DataProcessingOp::Rsb => {
                    let result = right.wrapping_sub(left);
                    write_result_and_advance(
                        regs,
                        rd,
                        result,
                        pc.wrapping_add(4),
                        &mut register_writes,
                    );
                    if set_flags {
                        regs.cpsr_mut().update_nzcv_sub(right, left, result);
                    }
                }
                DataProcessingOp::Mov => {
                    write_result_and_advance(
                        regs,
                        rd,
                        right,
                        pc.wrapping_add(4),
                        &mut register_writes,
                    );
                    if set_flags {
                        regs.cpsr_mut().update_nz(right);
                        if let Some(carry) = shifter_carry {
                            regs.cpsr_mut().set_carry(carry);
                        }
                    }
                }
                DataProcessingOp::Mvn => {
                    let result = !right;
                    write_result_and_advance(
                        regs,
                        rd,
                        result,
                        pc.wrapping_add(4),
                        &mut register_writes,
                    );
                    if set_flags {
                        regs.cpsr_mut().update_nz(result);
                        if let Some(carry) = shifter_carry {
                            regs.cpsr_mut().set_carry(carry);
                        }
                    }
                }
                DataProcessingOp::Cmp => {
                    let result = left.wrapping_sub(right);
                    regs.cpsr_mut().update_nzcv_sub(left, right, result);
                    let next_pc = pc.wrapping_add(4);
                    regs.set_pc(next_pc);
                    register_writes.push(RegisterWrite {
                        index: 15,
                        value: next_pc,
                    });
                }
                DataProcessingOp::Cmn => {
                    let result = left.wrapping_add(right);
                    regs.cpsr_mut().update_nzcv_add(left, right, result);
                    let next_pc = pc.wrapping_add(4);
                    regs.set_pc(next_pc);
                    register_writes.push(RegisterWrite {
                        index: 15,
                        value: next_pc,
                    });
                }
            }
        }
        DecodedInstruction::Multiply {
            accumulate,
            set_flags,
            rd,
            rn,
            rm,
            rs,
        } => {
            let lhs = regs.reg(rm);
            let rhs = regs.reg(rs);
            let mut result = lhs.wrapping_mul(rhs);
            if accumulate {
                result = result.wrapping_add(regs.reg(rn));
            }

            write_result_and_advance(
                regs,
                rd,
                result,
                pc.wrapping_add(4),
                &mut register_writes,
            );

            if set_flags {
                regs.cpsr_mut().update_nz(result);
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
            register_writes.push(RegisterWrite {
                index: rd_lo,
                value: lo,
            });
            register_writes.push(RegisterWrite {
                index: rd_hi,
                value: hi,
            });

            if set_flags {
                regs.cpsr_mut()
                    .set_negative((result_u64 & 0x8000_0000_0000_0000) != 0);
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
            byte,
            base,
            rd,
            offset,
            add_offset,
            pre_index,
            write_back,
        } => {
            execute_arm_single_data_transfer(
                memory,
                regs,
                pc,
                mode,
                load,
                byte,
                base,
                rd,
                offset,
                add_offset,
                pre_index,
                write_back,
                &mut register_writes,
            )?;
        }
        DecodedInstruction::SingleDataTransferRegister {
            load,
            byte,
            base,
            rd,
            rm,
            shift,
            add_offset,
            pre_index,
            write_back,
        } => {
            let rm_value = if rm == 15 && mode == ExecutionMode::Arm {
                pc.wrapping_add(8)
            } else {
                regs.reg(rm)
            };
            let (offset, _) = apply_register_shift(rm_value, shift, regs.cpsr().carry(), regs);
            execute_arm_single_data_transfer(
                memory,
                regs,
                pc,
                mode,
                load,
                byte,
                base,
                rd,
                offset,
                add_offset,
                pre_index,
                write_back,
                &mut register_writes,
            )?;
        }
        DecodedInstruction::HalfwordTransferImmediate {
            load,
            signed,
            halfword,
            base,
            rd,
            offset,
            add_offset,
            pre_index,
            write_back,
        } => {
            execute_arm_halfword_transfer(
                memory,
                regs,
                pc,
                mode,
                load,
                signed,
                halfword,
                base,
                rd,
                offset,
                add_offset,
                pre_index,
                write_back,
                &mut register_writes,
            )?;
        }
        DecodedInstruction::HalfwordTransferRegister {
            load,
            signed,
            halfword,
            base,
            rd,
            rm,
            add_offset,
            pre_index,
            write_back,
        } => {
            let offset = if rm == 15 && mode == ExecutionMode::Arm {
                pc.wrapping_add(8)
            } else {
                regs.reg(rm)
            };
            execute_arm_halfword_transfer(
                memory,
                regs,
                pc,
                mode,
                load,
                signed,
                halfword,
                base,
                rd,
                offset,
                add_offset,
                pre_index,
                write_back,
                &mut register_writes,
            )?;
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
            let immediate_carry = arm_immediate_carry(opcode, regs.cpsr().carry());
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
                    regs.cpsr_mut().set_carry(immediate_carry);
                }
            }
            DataProcessingOp::Mvn => {
                let result = !immediate;
                write_result_and_advance(
                    regs,
                    rd,
                    result,
                    pc.wrapping_add(4),
                    &mut register_writes,
                );
                if set_flags {
                    regs.cpsr_mut().update_nz(result);
                    regs.cpsr_mut().set_carry(immediate_carry);
                }
            }
            DataProcessingOp::And => {
                let left = regs.reg(rn);
                let result = left & immediate;
                write_result_and_advance(
                    regs,
                    rd,
                    result,
                    pc.wrapping_add(4),
                    &mut register_writes,
                );
                if set_flags {
                    regs.cpsr_mut().update_nz(result);
                    regs.cpsr_mut().set_carry(immediate_carry);
                }
            }
            DataProcessingOp::Eor => {
                let left = regs.reg(rn);
                let result = left ^ immediate;
                write_result_and_advance(
                    regs,
                    rd,
                    result,
                    pc.wrapping_add(4),
                    &mut register_writes,
                );
                if set_flags {
                    regs.cpsr_mut().update_nz(result);
                    regs.cpsr_mut().set_carry(immediate_carry);
                }
            }
            DataProcessingOp::Tst => {
                let left = regs.reg(rn);
                let result = left & immediate;
                regs.cpsr_mut().update_nz(result);
                regs.cpsr_mut().set_carry(immediate_carry);
                let next_pc = pc.wrapping_add(4);
                regs.set_pc(next_pc);
                register_writes.push(RegisterWrite {
                    index: 15,
                    value: next_pc,
                });
            }
            DataProcessingOp::Orr => {
                let left = regs.reg(rn);
                let result = left | immediate;
                write_result_and_advance(
                    regs,
                    rd,
                    result,
                    pc.wrapping_add(4),
                    &mut register_writes,
                );
                if set_flags {
                    regs.cpsr_mut().update_nz(result);
                    regs.cpsr_mut().set_carry(immediate_carry);
                }
            }
            DataProcessingOp::Bic => {
                let left = regs.reg(rn);
                let result = left & !immediate;
                write_result_and_advance(
                    regs,
                    rd,
                    result,
                    pc.wrapping_add(4),
                    &mut register_writes,
                );
                if set_flags {
                    regs.cpsr_mut().update_nz(result);
                    regs.cpsr_mut().set_carry(immediate_carry);
                }
            }
            DataProcessingOp::Add => {
                let left = regs.reg(rn);
                let result = left.wrapping_add(immediate);
                write_result_and_advance(
                    regs,
                    rd,
                    result,
                    pc.wrapping_add(4),
                    &mut register_writes,
                );
                if set_flags {
                    regs.cpsr_mut().update_nzcv_add(left, immediate, result);
                }
            }
            DataProcessingOp::Adc => {
                let left = regs.reg(rn);
                let (result, carry, overflow) = add_with_carry(left, immediate, regs.cpsr().carry());
                write_result_and_advance(
                    regs,
                    rd,
                    result,
                    pc.wrapping_add(4),
                    &mut register_writes,
                );
                if set_flags {
                    regs.cpsr_mut().update_nz(result);
                    regs.cpsr_mut().set_carry(carry);
                    regs.cpsr_mut().set_overflow(overflow);
                }
            }
            DataProcessingOp::Sub => {
                let left = regs.reg(rn);
                let result = left.wrapping_sub(immediate);
                write_result_and_advance(
                    regs,
                    rd,
                    result,
                    pc.wrapping_add(4),
                    &mut register_writes,
                );
                if set_flags {
                    regs.cpsr_mut().update_nzcv_sub(left, immediate, result);
                }
            }
            DataProcessingOp::Rsb => {
                let left = regs.reg(rn);
                let result = immediate.wrapping_sub(left);
                write_result_and_advance(
                    regs,
                    rd,
                    result,
                    pc.wrapping_add(4),
                    &mut register_writes,
                );
                if set_flags {
                    regs.cpsr_mut().update_nzcv_sub(immediate, left, result);
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
        },
        DecodedInstruction::ThumbAddSub {
            sub,
            rd,
            rs,
            operand,
        } => {
            let left = regs.reg(rs);
            let right = match operand {
                ThumbOperand::Immediate(value) => value,
                ThumbOperand::Register(index) => regs.reg(index),
            };
            let result = if sub {
                left.wrapping_sub(right)
            } else {
                left.wrapping_add(right)
            };
            regs.set_reg(rd, result);
            register_writes.push(RegisterWrite {
                index: rd,
                value: result,
            });
            if sub {
                regs.cpsr_mut().update_nzcv_sub(left, right, result);
            } else {
                regs.cpsr_mut().update_nzcv_add(left, right, result);
            }
            let next_pc = pc.wrapping_add(2);
            regs.set_pc(next_pc);
            register_writes.push(RegisterWrite {
                index: 15,
                value: next_pc,
            });
        }
        DecodedInstruction::ThumbAdjustSp {
            subtract,
            immediate,
        } => {
            let sp = regs.sp();
            let next_sp = if subtract {
                sp.wrapping_sub(immediate)
            } else {
                sp.wrapping_add(immediate)
            };
            regs.set_sp(next_sp);
            register_writes.push(RegisterWrite {
                index: 13,
                value: next_sp,
            });
            let next_pc = pc.wrapping_add(2);
            regs.set_pc(next_pc);
            register_writes.push(RegisterWrite {
                index: 15,
                value: next_pc,
            });
        }
        DecodedInstruction::ThumbAluRegister { op, rd, rs } => {
            let left = regs.reg(rd);
            let right = regs.reg(rs);
            match op {
                ThumbAluOp::Cmp => {
                    let result = left.wrapping_sub(right);
                    regs.cpsr_mut().update_nzcv_sub(left, right, result);
                }
                ThumbAluOp::Mvn => {
                    let result = !right;
                    regs.set_reg(rd, result);
                    register_writes.push(RegisterWrite {
                        index: rd,
                        value: result,
                    });
                    regs.cpsr_mut().update_nz(result);
                }
            }
            let next_pc = pc.wrapping_add(2);
            regs.set_pc(next_pc);
            register_writes.push(RegisterWrite {
                index: 15,
                value: next_pc,
            });
        }
        DecodedInstruction::ThumbShiftImmediate { rd, rs, shift } => {
            let value = regs.reg(rs);
            let (result, shifter_carry) = apply_register_shift(value, shift, regs.cpsr().carry(), regs);
            regs.set_reg(rd, result);
            register_writes.push(RegisterWrite {
                index: rd,
                value: result,
            });
            regs.cpsr_mut().update_nz(result);
            if let Some(carry) = shifter_carry {
                regs.cpsr_mut().set_carry(carry);
            }
            let next_pc = pc.wrapping_add(2);
            regs.set_pc(next_pc);
            register_writes.push(RegisterWrite {
                index: 15,
                value: next_pc,
            });
        }
        DecodedInstruction::ThumbCmpImmediate { rn, immediate } => {
            let left = regs.reg(rn);
            let result = left.wrapping_sub(immediate);
            regs.cpsr_mut().update_nzcv_sub(left, immediate, result);
            let next_pc = pc.wrapping_add(2);
            regs.set_pc(next_pc);
            register_writes.push(RegisterWrite {
                index: 15,
                value: next_pc,
            });
        }
        DecodedInstruction::ThumbLongBranchPrefix { offset } => {
            let base = pc.wrapping_add(4);
            let value = ((base as i64) + (offset as i64)) as u32;
            regs.set_lr(value);
            register_writes.push(RegisterWrite { index: 14, value });
            let next_pc = pc.wrapping_add(2);
            regs.set_pc(next_pc);
            register_writes.push(RegisterWrite {
                index: 15,
                value: next_pc,
            });
        }
        DecodedInstruction::ThumbLongBranchSuffix { exchange, offset } => {
            let target = regs.lr().wrapping_add(offset);
            let lr = pc.wrapping_add(2) | 1;
            regs.set_lr(lr);
            register_writes.push(RegisterWrite {
                index: 14,
                value: lr,
            });
            let next_mode = if exchange {
                ExecutionMode::Arm
            } else {
                ExecutionMode::Thumb
            };
            let next_pc = if exchange { target & !3 } else { target & !1 };
            regs.set_execution_mode(next_mode);
            regs.set_pc(next_pc);
            register_writes.push(RegisterWrite {
                index: 15,
                value: next_pc,
            });
        }
        DecodedInstruction::ThumbBranch { offset } => {
            let next_pc = ((pc as i64) + 4 + (offset as i64)) as u32;
            regs.set_pc(next_pc);
            register_writes.push(RegisterWrite {
                index: 15,
                value: next_pc,
            });
        }
        DecodedInstruction::ThumbConditionalBranch { condition, offset } => {
            let next_pc = if thumb_condition_passed(regs.cpsr(), condition) {
                ((pc as i64) + 4 + (offset as i64)) as u32
            } else {
                pc.wrapping_add(2)
            };
            regs.set_pc(next_pc);
            register_writes.push(RegisterWrite {
                index: 15,
                value: next_pc,
            });
        }
        DecodedInstruction::ThumbHiRegisterOp { op, rd, rs } => {
            let right = if rs == 15 {
                pc.wrapping_add(4)
            } else {
                regs.reg(rs)
            };
            let left = if rd == 15 {
                pc.wrapping_add(4)
            } else {
                regs.reg(rd)
            };
            match op {
                ThumbHiOp::Add => {
                    let result = left.wrapping_add(right);
                    if rd == 15 {
                        regs.set_pc(result & !1);
                        register_writes.push(RegisterWrite {
                            index: 15,
                            value: result & !1,
                        });
                    } else {
                        regs.set_reg(rd, result);
                        register_writes.push(RegisterWrite {
                            index: rd,
                            value: result,
                        });
                        let next_pc = pc.wrapping_add(2);
                        regs.set_pc(next_pc);
                        register_writes.push(RegisterWrite {
                            index: 15,
                            value: next_pc,
                        });
                    }
                }
                ThumbHiOp::Cmp => {
                    let result = left.wrapping_sub(right);
                    regs.cpsr_mut().update_nzcv_sub(left, right, result);
                    let next_pc = pc.wrapping_add(2);
                    regs.set_pc(next_pc);
                    register_writes.push(RegisterWrite {
                        index: 15,
                        value: next_pc,
                    });
                }
                ThumbHiOp::Mov => {
                    if rd == 15 {
                        regs.set_pc(right & !1);
                        register_writes.push(RegisterWrite {
                            index: 15,
                            value: right & !1,
                        });
                    } else {
                        regs.set_reg(rd, right);
                        register_writes.push(RegisterWrite {
                            index: rd,
                            value: right,
                        });
                        let next_pc = pc.wrapping_add(2);
                        regs.set_pc(next_pc);
                        register_writes.push(RegisterWrite {
                            index: 15,
                            value: next_pc,
                        });
                    }
                }
                ThumbHiOp::Bx => {
                    let next_mode = if right & 1 != 0 {
                        ExecutionMode::Thumb
                    } else {
                        ExecutionMode::Arm
                    };
                    let next_pc = if next_mode == ExecutionMode::Thumb {
                        right & !1
                    } else {
                        right & !3
                    };
                    regs.set_execution_mode(next_mode);
                    regs.set_pc(next_pc);
                    register_writes.push(RegisterWrite {
                        index: 15,
                        value: next_pc,
                    });
                }
                ThumbHiOp::Blx => {
                    let lr = pc.wrapping_add(2) | 1;
                    regs.set_lr(lr);
                    register_writes.push(RegisterWrite {
                        index: 14,
                        value: lr,
                    });
                    let next_pc = right & !3;
                    regs.set_execution_mode(ExecutionMode::Arm);
                    regs.set_pc(next_pc);
                    register_writes.push(RegisterWrite {
                        index: 15,
                        value: next_pc,
                    });
                }
            }
        }
        DecodedInstruction::ThumbLiteralLoad { rd, offset } => {
            let base = (pc.wrapping_add(4)) & !3;
            let addr = base.wrapping_add(offset);
            let value = memory.read32(GuestAddr::new(addr))?;
            regs.set_reg(rd, value);
            register_writes.push(RegisterWrite { index: rd, value });
            let next_pc = pc.wrapping_add(2);
            regs.set_pc(next_pc);
            register_writes.push(RegisterWrite {
                index: 15,
                value: next_pc,
            });
        }
        DecodedInstruction::ThumbLoadAddress { sp, rd, offset } => {
            let base = if sp {
                regs.sp()
            } else {
                (pc.wrapping_add(4)) & !3
            };
            let value = base.wrapping_add(offset);
            regs.set_reg(rd, value);
            register_writes.push(RegisterWrite { index: rd, value });
            let next_pc = pc.wrapping_add(2);
            regs.set_pc(next_pc);
            register_writes.push(RegisterWrite {
                index: 15,
                value: next_pc,
            });
        }
        DecodedInstruction::ThumbLoadStoreRegisterOffset {
            load,
            byte,
            offset,
            base,
            rd,
        } => {
            let addr = regs.reg(base).wrapping_add(regs.reg(offset));
            if load {
                let value = if byte {
                    memory.read8(GuestAddr::new(addr))? as u32
                } else {
                    memory.read32(GuestAddr::new(addr))?
                };
                regs.set_reg(rd, value);
                register_writes.push(RegisterWrite { index: rd, value });
            } else if byte {
                memory.write8(GuestAddr::new(addr), regs.reg(rd) as u8)?;
            } else {
                memory.write32(GuestAddr::new(addr), regs.reg(rd))?;
            }
            let next_pc = pc.wrapping_add(2);
            regs.set_pc(next_pc);
            register_writes.push(RegisterWrite {
                index: 15,
                value: next_pc,
            });
        }
        DecodedInstruction::ThumbLoadStoreMultiple {
            load,
            base,
            register_mask,
        } => {
            let mut addr = regs.reg(base);
            for index in 0..8usize {
                if (register_mask & (1 << index)) == 0 {
                    continue;
                }
                if load {
                    let value = memory.read32(GuestAddr::new(addr))?;
                    regs.set_reg(index, value);
                    register_writes.push(RegisterWrite { index, value });
                } else {
                    let value = regs.reg(index);
                    memory.write32(GuestAddr::new(addr), value)?;
                }
                addr = addr.wrapping_add(4);
            }
            regs.set_reg(base, addr);
            register_writes.push(RegisterWrite {
                index: base,
                value: addr,
            });
            let next_pc = pc.wrapping_add(2);
            regs.set_pc(next_pc);
            register_writes.push(RegisterWrite {
                index: 15,
                value: next_pc,
            });
        }
        DecodedInstruction::ThumbLoadStoreSpRelative { load, rd, offset } => {
            let addr = regs.sp().wrapping_add(offset);
            if load {
                let value = memory.read32(GuestAddr::new(addr))?;
                regs.set_reg(rd, value);
                register_writes.push(RegisterWrite { index: rd, value });
            } else {
                let value = regs.reg(rd);
                memory.write32(GuestAddr::new(addr), value)?;
            }
            let next_pc = pc.wrapping_add(2);
            regs.set_pc(next_pc);
            register_writes.push(RegisterWrite {
                index: 15,
                value: next_pc,
            });
        }
        DecodedInstruction::ThumbLoadStoreByteImmediate {
            load,
            base,
            rd,
            offset,
        } => {
            let addr = regs.reg(base).wrapping_add(offset);
            if load {
                let value = memory.read8(GuestAddr::new(addr))? as u32;
                regs.set_reg(rd, value);
                register_writes.push(RegisterWrite { index: rd, value });
            } else {
                memory.write8(GuestAddr::new(addr), regs.reg(rd) as u8)?;
            }
            let next_pc = pc.wrapping_add(2);
            regs.set_pc(next_pc);
            register_writes.push(RegisterWrite {
                index: 15,
                value: next_pc,
            });
        }
        DecodedInstruction::ThumbLoadStoreWordImmediate {
            load,
            base,
            rd,
            offset,
        } => {
            let addr = regs.reg(base).wrapping_add(offset);
            if load {
                let value = memory.read32(GuestAddr::new(addr))?;
                regs.set_reg(rd, value);
                register_writes.push(RegisterWrite { index: rd, value });
            } else {
                let value = regs.reg(rd);
                memory.write32(GuestAddr::new(addr), value)?;
            }
            let next_pc = pc.wrapping_add(2);
            regs.set_pc(next_pc);
            register_writes.push(RegisterWrite {
                index: 15,
                value: next_pc,
            });
        }
        DecodedInstruction::ThumbPop {
            register_mask,
            include_pc,
        } => {
            let mut sp = regs.sp();
            for index in 0..8usize {
                if (register_mask & (1 << index)) == 0 {
                    continue;
                }
                let value = memory.read32(GuestAddr::new(sp))?;
                regs.set_reg(index, value);
                register_writes.push(RegisterWrite { index, value });
                sp = sp.wrapping_add(4);
            }
            if include_pc {
                let target = memory.read32(GuestAddr::new(sp))?;
                sp = sp.wrapping_add(4);
                regs.set_sp(sp);
                register_writes.push(RegisterWrite {
                    index: 13,
                    value: sp,
                });
                let next_mode = if target & 1 != 0 {
                    ExecutionMode::Thumb
                } else {
                    ExecutionMode::Arm
                };
                let next_pc = if next_mode == ExecutionMode::Thumb {
                    target & !1
                } else {
                    target & !3
                };
                regs.set_execution_mode(next_mode);
                regs.set_pc(next_pc);
                register_writes.push(RegisterWrite {
                    index: 15,
                    value: next_pc,
                });
            } else {
                regs.set_sp(sp);
                register_writes.push(RegisterWrite {
                    index: 13,
                    value: sp,
                });
                let next_pc = pc.wrapping_add(2);
                regs.set_pc(next_pc);
                register_writes.push(RegisterWrite {
                    index: 15,
                    value: next_pc,
                });
            }
        }
        DecodedInstruction::ThumbPush {
            register_mask,
            include_lr,
        } => {
            let mut registers: Vec<usize> = (0..8)
                .filter(|index| (register_mask & (1 << index)) != 0)
                .collect();
            if include_lr {
                registers.push(14);
            }
            let mut sp = regs.sp();
            for index in registers.iter().rev() {
                sp = sp.wrapping_sub(4);
                let value = regs.reg(*index);
                memory.write32(GuestAddr::new(sp), value)?;
            }
            regs.set_sp(sp);
            register_writes.push(RegisterWrite {
                index: 13,
                value: sp,
            });
            let next_pc = pc.wrapping_add(2);
            regs.set_pc(next_pc);
            register_writes.push(RegisterWrite {
                index: 15,
                value: next_pc,
            });
        }
        DecodedInstruction::ThumbSubImmediate { rd, immediate } => {
            let left = regs.reg(rd);
            let result = left.wrapping_sub(immediate);
            regs.set_reg(rd, result);
            register_writes.push(RegisterWrite {
                index: rd,
                value: result,
            });
            regs.cpsr_mut().update_nzcv_sub(left, immediate, result);
            let next_pc = pc.wrapping_add(2);
            regs.set_pc(next_pc);
            register_writes.push(RegisterWrite {
                index: 15,
                value: next_pc,
            });
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

fn execute_arm_single_data_transfer<B: MemoryBus>(
    memory: &mut B,
    regs: &mut CpuRegs,
    pc: u32,
    mode: ExecutionMode,
    load: bool,
    byte: bool,
    base: usize,
    rd: usize,
    offset: u32,
    add_offset: bool,
    pre_index: bool,
    write_back: bool,
    register_writes: &mut Vec<RegisterWrite>,
) -> Result<(), CpuError> {
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
        let value = if byte {
            memory.read8(GuestAddr::new(address))? as u32
        } else {
            read_arm_word(memory, address)?
        };
        if rd == 15 && !byte {
            let next_mode = if value & 1 != 0 {
                ExecutionMode::Thumb
            } else {
                ExecutionMode::Arm
            };
            let next_pc = if next_mode == ExecutionMode::Thumb {
                value & !1
            } else {
                value & !3
            };
            regs.set_execution_mode(next_mode);
            regs.set_pc(next_pc);
            register_writes.push(RegisterWrite {
                index: 15,
                value: next_pc,
            });
        } else {
            regs.set_reg(rd, value);
            register_writes.push(RegisterWrite { index: rd, value });
        }
    } else {
        let value = regs.reg(rd);
        if byte {
            memory.write8(GuestAddr::new(address), value as u8)?;
        } else {
            write_arm_word(memory, address, value)?;
        }
    }

    if !pre_index || write_back {
        regs.set_reg(base, offset_addr);
        register_writes.push(RegisterWrite {
            index: base,
            value: offset_addr,
        });
    }

    if !(load && rd == 15 && !byte) {
        let next_pc = pc.wrapping_add(4);
        regs.set_pc(next_pc);
        register_writes.push(RegisterWrite {
            index: 15,
            value: next_pc,
        });
    }

    Ok(())
}

fn execute_arm_halfword_transfer<B: MemoryBus>(
    memory: &mut B,
    regs: &mut CpuRegs,
    pc: u32,
    mode: ExecutionMode,
    load: bool,
    signed: bool,
    halfword: bool,
    base: usize,
    rd: usize,
    offset: u32,
    add_offset: bool,
    pre_index: bool,
    write_back: bool,
    register_writes: &mut Vec<RegisterWrite>,
) -> Result<(), CpuError> {
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
        let value = if signed {
            if halfword {
                memory.read16(GuestAddr::new(address))? as i16 as i32 as u32
            } else {
                memory.read8(GuestAddr::new(address))? as i8 as i32 as u32
            }
        } else if halfword {
            memory.read16(GuestAddr::new(address))? as u32
        } else {
            return Err(CpuError::UnimplementedInstruction {
                pc,
                mode,
                opcode: 0,
            });
        };

        regs.set_reg(rd, value);
        register_writes.push(RegisterWrite { index: rd, value });
    } else {
        if signed || !halfword {
            return Err(CpuError::UnimplementedInstruction {
                pc,
                mode,
                opcode: 0,
            });
        }
        memory.write16(GuestAddr::new(address), regs.reg(rd) as u16)?;
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

    Ok(())
}

fn read_arm_word<B: MemoryBus>(memory: &B, address: u32) -> Result<u32, CpuError> {
    let aligned = address & !3;
    let word = memory.read32(GuestAddr::new(aligned))?;
    let rotate = (address & 3) * 8;
    Ok(if rotate == 0 {
        word
    } else {
        word.rotate_right(rotate)
    })
}

fn write_arm_word<B: MemoryBus>(
    memory: &mut B,
    address: u32,
    value: u32,
) -> Result<(), CpuError> {
    let aligned = address & !3;
    memory.write32(GuestAddr::new(aligned), value)?;
    Ok(())
}

fn add_with_carry(lhs: u32, rhs: u32, carry_in: bool) -> (u32, bool, bool) {
    let carry = carry_in as u64;
    let wide = lhs as u64 + rhs as u64 + carry;
    let result = wide as u32;
    let carry_out = wide > (u32::MAX as u64);
    let overflow = (((lhs ^ result) & (rhs ^ result)) & 0x8000_0000) != 0;
    (result, carry_out, overflow)
}

fn arm_immediate_carry(opcode: u32, carry_in: bool) -> bool {
    let rotate = (((opcode >> 8) & 0xF) * 2) as u32;
    if rotate == 0 {
        carry_in
    } else {
        let imm8 = opcode & 0xFF;
        let immediate = imm8.rotate_right(rotate);
        ((immediate >> 31) & 1) != 0
    }
}

fn apply_register_shift(
    value: u32,
    shift: RegisterShift,
    carry_in: bool,
    regs: &CpuRegs,
) -> (u32, Option<bool>) {
    match shift {
        RegisterShift::Lsl(0) => (value, None),
        RegisterShift::Lsl(n) if n < 32 => {
            let out = value.wrapping_shl(n as u32);
            let carry = ((value >> (32 - n)) & 1) != 0;
            (out, Some(carry))
        }
        RegisterShift::Lsl(32) => (0, Some((value & 1) != 0)),
        RegisterShift::Lsl(_) => (0, Some(false)),
        RegisterShift::LslRegister(rs) => {
            let amount = (regs.reg(rs) & 0xFF) as u32;
            match amount {
                0 => (value, None),
                1..=31 => {
                    let out = value << amount;
                    let carry = ((value >> (32 - amount)) & 1) != 0;
                    (out, Some(carry))
                }
                32 => (0, Some((value & 1) != 0)),
                _ => (0, Some(false)),
            }
        }

        RegisterShift::Lsr(32) => (0, Some(((value >> 31) & 1) != 0)),
        RegisterShift::Lsr(n) if n < 32 => {
            let out = value >> n;
            let carry = ((value >> (n - 1)) & 1) != 0;
            (out, Some(carry))
        }
        RegisterShift::Lsr(_) => (0, Some(false)),
        RegisterShift::LsrRegister(rs) => {
            let amount = (regs.reg(rs) & 0xFF) as u32;
            match amount {
                0 => (value, None),
                1..=31 => {
                    let out = value >> amount;
                    let carry = ((value >> (amount - 1)) & 1) != 0;
                    (out, Some(carry))
                }
                32 => (0, Some(((value >> 31) & 1) != 0)),
                _ => (0, Some(false)),
            }
        }

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
        RegisterShift::AsrRegister(rs) => {
            let amount = (regs.reg(rs) & 0xFF) as u32;
            match amount {
                0 => (value, None),
                1..=31 => {
                    let out = ((value as i32) >> amount) as u32;
                    let carry = ((value >> (amount - 1)) & 1) != 0;
                    (out, Some(carry))
                }
                _ if (value & 0x8000_0000) != 0 => (u32::MAX, Some(true)),
                _ => (0, Some(false)),
            }
        }

        RegisterShift::Ror(0) => {
            let out = ((carry_in as u32) << 31) | (value >> 1);
            let carry = (value & 1) != 0;
            (out, Some(carry))
        }
        RegisterShift::Ror(n) => {
            let rot = (n as u32) % 32;
            let out = if rot == 0 {
                value
            } else {
                value.rotate_right(rot)
            };
            let carry = ((out >> 31) & 1) != 0;
            (out, Some(carry))
        }
        RegisterShift::RorRegister(rs) => {
            let amount = (regs.reg(rs) & 0xFF) as u32;
            match amount {
                0 => (value, None),
                _ => {
                    let rot = amount % 32;
                    let out = if rot == 0 {
                        value
                    } else {
                        value.rotate_right(rot)
                    };
                    let carry = if rot == 0 {
                        ((value >> 31) & 1) != 0
                    } else {
                        ((out >> 31) & 1) != 0
                    };
                    (out, Some(carry))
                }
            }
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

fn thumb_condition_passed(cpsr: Cpsr, condition: Condition) -> bool {
    match condition {
        Condition::Eq => cpsr.zero(),
        Condition::Ne => !cpsr.zero(),
        Condition::Al => true,
        Condition::Other(_) => false,
    }
}















