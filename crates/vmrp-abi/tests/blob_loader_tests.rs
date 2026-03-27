use vmrp_abi::{CodeBlob, LoadedImage};
use vmrp_cpu::Cpu;

#[test]
fn code_blob_constructor_exposes_entry_and_mode() {
    let blob = CodeBlob::raw_arm(0x80000, vec![0x01, 0x00, 0xA0, 0xE3]);
    let _image: Option<LoadedImage> = None;

    assert_eq!(blob.load_address().get(), 0x80000);
    assert_eq!(blob.entry().get(), 0x80000);
    assert_eq!(blob.len(), 4);
    assert!(blob.is_arm());
}

#[test]
fn loaded_arm_blob_executes_through_cpu() {
    let blob = CodeBlob::raw_arm(0x80000, vec![0x01, 0x00, 0xA0, 0xE3]);
    let loaded = blob.load().unwrap();

    let mut cpu = Cpu::new(loaded.memory().clone());
    cpu.regs_mut().set_pc(loaded.entry().get());
    cpu.regs_mut().set_execution_mode(loaded.mode());

    let step = cpu.step().unwrap();

    assert_eq!(loaded.load_address().get(), 0x80000);
    assert_eq!(cpu.regs().reg(0), 1);
    assert_eq!(cpu.regs().pc(), 0x80004);
    assert_eq!(step.trace.opcode, 0xE3A0_0001);
}
