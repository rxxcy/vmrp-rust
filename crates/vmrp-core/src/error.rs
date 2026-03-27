use crate::GuestAddr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryError {
    InvalidAddress(GuestAddr),
}
