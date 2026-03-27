use vmrp_core::{AddressSpaceLayout, GuestAddr};
use vmrp_cpu::{MemoryAccessError, MemoryBus, TestMemory};

#[derive(Clone, Debug)]
pub struct GuestImage {
    memory: TestMemory,
}

impl GuestImage {
    pub fn for_layout(layout: AddressSpaceLayout) -> Self {
        Self {
            memory: TestMemory::with_ram(layout.code_address(), layout.code_size()),
        }
    }

    pub fn write_blob(&mut self, start: GuestAddr, bytes: &[u8]) -> Result<(), MemoryAccessError> {
        for (offset, byte) in bytes.iter().enumerate() {
            let addr = GuestAddr::new(start.get().wrapping_add(offset as u32));
            self.memory.write8(addr, *byte)?;
        }
        Ok(())
    }

    pub fn into_memory(self) -> TestMemory {
        self.memory
    }
}
