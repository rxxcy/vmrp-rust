use std::path::PathBuf;

use vmrp_abi::ExtFile;
use vmrp_cpu::Cpu;

fn minimal_ext_path() -> PathBuf {
    PathBuf::from(r"D:\opt\rust\vmrp-rust\tests\fixtures\ext\minimal.ext")
}

fn real_ext_path() -> PathBuf {
    PathBuf::from(r"D:\opt\rust\vmrp\mrc\asm\cfunction.ext")
}

#[test]
fn parses_valid_ext_header() {
    let ext = ExtFile::from_path(minimal_ext_path()).unwrap();

    assert_eq!(ext.header(), b"MRPGCMAP");
    assert_eq!(ext.payload(), &[0x01, 0x00, 0xA0, 0xE3]);
}

#[test]
fn rejects_invalid_ext_header() {
    let err = ExtFile::from_bytes(b"NOTVALID....").unwrap_err();
    assert!(format!("{err:?}").contains("InvalidHeader"));
}

#[test]
fn ext_conversion_exposes_code_base_and_entry_offset() {
    let ext = ExtFile::from_path(minimal_ext_path()).unwrap();
    let blob = ext.to_code_blob(0x80000);

    assert_eq!(blob.load_address().get(), 0x80000);
    assert_eq!(blob.entry().get(), 0x80008);
}

#[test]
fn synthetic_ext_executes_through_cpu() {
    let ext = ExtFile::from_path(minimal_ext_path()).unwrap();
    let blob = ext.to_code_blob(0x80000);
    let loaded = blob.load().unwrap();

    let mut cpu = Cpu::new(loaded.memory().clone());
    cpu.regs_mut().set_pc(loaded.entry().get());
    cpu.regs_mut().set_execution_mode(loaded.mode());

    let step = cpu.step().unwrap();

    assert_eq!(cpu.regs().reg(0), 1);
    assert_eq!(cpu.regs().pc(), 0x8000C);
    assert_eq!(step.trace.opcode, 0xE3A0_0001);
}

#[test]
fn real_ext_entry_words_match_observed_sequence() {
    let ext = ExtFile::from_path(real_ext_path()).unwrap();
    let words = ext.entry_words(6).unwrap();

    assert_eq!(
        words,
        vec![0xE92D4038, 0xE59F410C, 0xE08F4004, 0xE5141008, 0xE3500001, 0xE5912064,]
    );
}
