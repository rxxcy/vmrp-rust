use vmrp_core::GuestAddr;
use vmrp_cpu::{MemoryBus, MemoryAccessError, TestMemory};

#[test]
fn reads_and_writes_u32_little_endian_values() {
    let mut mem = TestMemory::with_ram(GuestAddr::new(0x80000), 0x1000);
    mem.write32(GuestAddr::new(0x80000), 0x1234_5678).unwrap();
    assert_eq!(mem.read32(GuestAddr::new(0x80000)).unwrap(), 0x1234_5678);
}

#[test]
fn rejects_out_of_range_accesses() {
    let mut mem = TestMemory::with_ram(GuestAddr::new(0x80000), 0x10);
    let err = mem.write8(GuestAddr::new(0x90000), 0xAA).unwrap_err();
    assert_eq!(err, MemoryAccessError::OutOfRange(GuestAddr::new(0x90000)));
}
