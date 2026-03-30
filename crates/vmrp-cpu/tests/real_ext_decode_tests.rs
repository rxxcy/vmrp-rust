use vmrp_cpu::{decode_arm_opcode, Condition, DataProcessingOp, DecodedInstruction, RegisterShift};

#[test]
fn public_decode_api_recognizes_real_ext_push_opcode() {
    let instruction = decode_arm_opcode(0xE92D4038);

    assert!(matches!(
        instruction,
        DecodedInstruction::BlockTransfer {
            load: false,
            pre_index: true,
            add_offset: false,
            write_back: true,
            base: 13,
            register_mask: 0x4038,
        }
    ));
}

#[test]
fn decodes_real_ext_literal_load_and_register_add() {
    let ldr_literal = decode_arm_opcode(0xE59F410C);
    let add_register = decode_arm_opcode(0xE08F4004);

    assert!(matches!(
        ldr_literal,
        DecodedInstruction::SingleDataTransferImmediate {
            load: true,
            byte: false,
            base: 15,
            rd: 4,
            offset: 0x10C,
            add_offset: true,
            pre_index: true,
            write_back: false,
        }
    ));

    assert!(matches!(
        add_register,
        DecodedInstruction::DataProcessingRegister {
            op: DataProcessingOp::Add,
            set_flags: false,
            rn: 15,
            rd: 4,
            rm: 4,
            shift: RegisterShift::Lsl(0),
        }
    ));
}

#[test]
fn decodes_real_ext_memory_branch_and_blx_forms() {
    let ldr_offset = decode_arm_opcode(0xE5141008);
    let branch = decode_arm_opcode(0x1A00000F);
    let blx = decode_arm_opcode(0xE12FFF32);
    let mov_register = decode_arm_opcode(0xE1A04000);
    let sub_register = decode_arm_opcode(0xE0415000);

    assert!(matches!(
        ldr_offset,
        DecodedInstruction::SingleDataTransferImmediate {
            load: true,
            byte: false,
            base: 4,
            rd: 1,
            offset: 8,
            add_offset: false,
            pre_index: true,
            write_back: false,
        }
    ));

    assert!(matches!(
        branch,
        DecodedInstruction::Branch {
            link: false,
            condition: Condition::Ne,
            offset: 60,
        }
    ));

    assert!(matches!(
        blx,
        DecodedInstruction::BranchExchange {
            link: true,
            register: 2,
        }
    ));

    assert!(matches!(
        mov_register,
        DecodedInstruction::DataProcessingRegister {
            op: DataProcessingOp::Mov,
            set_flags: false,
            rn: 0,
            rd: 4,
            rm: 0,
            shift: RegisterShift::Lsl(0),
        }
    ));

    assert!(matches!(
        sub_register,
        DecodedInstruction::DataProcessingRegister {
            op: DataProcessingOp::Sub,
            set_flags: false,
            rn: 1,
            rd: 5,
            rm: 0,
            shift: RegisterShift::Lsl(0),
        }
    ));
}

#[test]
fn decodes_event_path_shifted_add_as_data_processing_register() {
    let shifted_add = decode_arm_opcode(0x908F_F108);

    assert!(matches!(
        shifted_add,
        DecodedInstruction::DataProcessingRegister {
            op: DataProcessingOp::Add,
            set_flags: false,
            rn: 15,
            rd: 15,
            rm: 8,
            shift: RegisterShift::Lsl(2),
        }
    ));
}

#[test]
fn decodes_real_ext_and_immediate_and_stmib() {
    let and_imm = decode_arm_opcode(0xE20230FF);
    let stmib = decode_arm_opcode(0xE98D000C);

    assert!(matches!(
        and_imm,
        DecodedInstruction::DataProcessingImmediate {
            op: DataProcessingOp::And,
            set_flags: false,
            rn: 2,
            rd: 3,
            immediate: 0xFF,
        }
    ));

    assert!(matches!(
        stmib,
        DecodedInstruction::BlockTransfer {
            load: false,
            pre_index: true,
            add_offset: true,
            write_back: false,
            base: 13,
            register_mask: 0x000C,
        }
    ));
}

#[test]
fn decodes_real_ext_smull() {
    let smull = decode_arm_opcode(0xE0C32190);

    assert!(matches!(
        smull,
        DecodedInstruction::MultiplyLong {
            signed: true,
            accumulate: false,
            set_flags: false,
            rd_hi: 3,
            rd_lo: 2,
            rm: 0,
            rs: 1,
        }
    ));
}

#[test]
fn decodes_real_ext_cmp_register_form() {
    let cmp_register = decode_arm_opcode(0xE1520003);

    assert!(matches!(
        cmp_register,
        DecodedInstruction::DataProcessingRegister {
            op: DataProcessingOp::Cmp,
            set_flags: true,
            rn: 2,
            rd: 0,
            rm: 3,
            shift: RegisterShift::Lsl(0),
        }
    ));
}
#[test]
fn decodes_real_ext_byte_store_forms() {
    let strb_post = decode_arm_opcode(0xE4C13001);
    let strb_reg = decode_arm_opcode(0x37C10002);

    assert!(matches!(
        strb_post,
        DecodedInstruction::SingleDataTransferImmediate {
            load: false,
            byte: true,
            base: 1,
            rd: 3,
            offset: 1,
            add_offset: true,
            pre_index: false,
            write_back: false,
        }
    ));

    assert!(matches!(
        strb_reg,
        DecodedInstruction::SingleDataTransferRegister {
            load: false,
            byte: true,
            base: 1,
            rd: 0,
            rm: 2,
            shift: RegisterShift::Lsl(0),
            add_offset: true,
            pre_index: true,
            write_back: false,
        }
    ));
}
#[test]
fn decodes_real_ext_mul() {
    let mul = decode_arm_opcode(0xE0000091);

    assert!(matches!(
        mul,
        DecodedInstruction::Multiply {
            accumulate: false,
            set_flags: false,
            rd: 0,
            rn: 0,
            rm: 1,
            rs: 0,
        }
    ));
}

#[test]
fn decodes_real_ext_bic_register_shifted_by_register() {
    let bic = decode_arm_opcode(0xE1C31E32);

    assert!(matches!(
        bic,
        DecodedInstruction::DataProcessingRegister {
            op: DataProcessingOp::Bic,
            set_flags: false,
            rn: 3,
            rd: 1,
            rm: 2,
            shift: RegisterShift::LsrRegister(14),
        }
    ));
}

#[test]
fn decodes_real_ext_eor_register() {
    let eor = decode_arm_opcode(0xE0228008);

    assert!(matches!(
        eor,
        DecodedInstruction::DataProcessingRegister {
            op: DataProcessingOp::Eor,
            set_flags: false,
            rn: 2,
            rd: 8,
            rm: 8,
            shift: RegisterShift::Lsl(0),
        }
    ));
}

#[test]
fn decodes_halfword_transfer_immediate() {
    let ldrh = decode_arm_opcode(0xE1D3_20B8);

    assert!(matches!(
        ldrh,
        DecodedInstruction::HalfwordTransferImmediate {
            load: true,
            signed: false,
            halfword: true,
            base: 3,
            rd: 2,
            offset: 8,
            add_offset: true,
            pre_index: true,
            write_back: false,
        }
    ));
}

