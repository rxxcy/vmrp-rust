use crate::GuestAddr;

pub const CODE_ADDRESS: GuestAddr = GuestAddr::new(0x80000);
pub const CODE_SIZE: u32 = 1024 * 1024;
pub const STACK_ADDRESS: GuestAddr = GuestAddr::new(CODE_ADDRESS.get() + CODE_SIZE);
pub const STACK_SIZE: u32 = 1024 * 1024;
pub const MEMORY_MANAGER_ADDRESS: GuestAddr = GuestAddr::new(STACK_ADDRESS.get() + STACK_SIZE);
pub const MEMORY_MANAGER_SIZE: u32 = 1024 * 1024 * 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryRegion {
    base: GuestAddr,
    size: u32,
}

impl MemoryRegion {
    pub const fn new(base: GuestAddr, size: u32) -> Self {
        Self { base, size }
    }

    pub const fn base(self) -> GuestAddr {
        self.base
    }

    pub const fn size(self) -> u32 {
        self.size
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AddressSpaceLayout {
    code: MemoryRegion,
    stack: MemoryRegion,
    memory_manager: MemoryRegion,
}

impl AddressSpaceLayout {
    pub const fn new(code: MemoryRegion, stack: MemoryRegion, memory_manager: MemoryRegion) -> Self {
        Self {
            code,
            stack,
            memory_manager,
        }
    }

    pub const fn code_address(self) -> GuestAddr {
        self.code.base()
    }

    pub const fn code_size(self) -> u32 {
        self.code.size()
    }

    pub const fn stack_address(self) -> GuestAddr {
        self.stack.base()
    }

    pub const fn stack_size(self) -> u32 {
        self.stack.size()
    }

    pub const fn memory_manager_address(self) -> GuestAddr {
        self.memory_manager.base()
    }

    pub const fn memory_manager_size(self) -> u32 {
        self.memory_manager.size()
    }
}

pub const DEFAULT_LAYOUT: AddressSpaceLayout = AddressSpaceLayout::new(
    MemoryRegion::new(CODE_ADDRESS, CODE_SIZE),
    MemoryRegion::new(STACK_ADDRESS, STACK_SIZE),
    MemoryRegion::new(MEMORY_MANAGER_ADDRESS, MEMORY_MANAGER_SIZE),
);
