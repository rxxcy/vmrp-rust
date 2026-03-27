use vmrp_core::{layout::DEFAULT_LAYOUT, GuestAddr};

#[test]
fn default_layout_matches_c_reference() {
    assert_eq!(DEFAULT_LAYOUT.code_address().get(), 0x80000);
    assert_eq!(DEFAULT_LAYOUT.code_size(), 1024 * 1024);
    assert_eq!(DEFAULT_LAYOUT.stack_address().get(), 0x180000);
    assert_eq!(DEFAULT_LAYOUT.memory_manager_address().get(), 0x280000);
}

#[test]
fn guest_addr_round_trips_raw_values() {
    assert_eq!(GuestAddr::new(0x1234_5678).get(), 0x1234_5678);
}
