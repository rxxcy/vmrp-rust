use vmrp_core::GuestAddr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryAccessError {
    OutOfRange(GuestAddr),
}

pub trait MemoryBus {
    fn read8(&self, addr: GuestAddr) -> Result<u8, MemoryAccessError>;
    fn read16(&self, addr: GuestAddr) -> Result<u16, MemoryAccessError>;
    fn read32(&self, addr: GuestAddr) -> Result<u32, MemoryAccessError>;
    fn write8(&mut self, addr: GuestAddr, value: u8) -> Result<(), MemoryAccessError>;
    fn write16(&mut self, addr: GuestAddr, value: u16) -> Result<(), MemoryAccessError>;
    fn write32(&mut self, addr: GuestAddr, value: u32) -> Result<(), MemoryAccessError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestMemory {
    base: GuestAddr,
    bytes: Vec<u8>,
}

impl TestMemory {
    pub fn with_ram(base: GuestAddr, size: u32) -> Self {
        Self {
            base,
            bytes: vec![0; size as usize],
        }
    }

    fn offset_for(&self, addr: GuestAddr, width: usize) -> Result<usize, MemoryAccessError> {
        let start = self.base.get();
        let value = addr.get();
        if value < start {
            return Err(MemoryAccessError::OutOfRange(addr));
        }

        let offset = (value - start) as usize;
        let end = offset
            .checked_add(width)
            .ok_or(MemoryAccessError::OutOfRange(addr))?;
        if end > self.bytes.len() {
            return Err(MemoryAccessError::OutOfRange(addr));
        }

        Ok(offset)
    }
}

impl MemoryBus for TestMemory {
    fn read8(&self, addr: GuestAddr) -> Result<u8, MemoryAccessError> {
        let offset = self.offset_for(addr, 1)?;
        Ok(self.bytes[offset])
    }

    fn read16(&self, addr: GuestAddr) -> Result<u16, MemoryAccessError> {
        let offset = self.offset_for(addr, 2)?;
        Ok(u16::from_le_bytes([
            self.bytes[offset],
            self.bytes[offset + 1],
        ]))
    }

    fn read32(&self, addr: GuestAddr) -> Result<u32, MemoryAccessError> {
        let offset = self.offset_for(addr, 4)?;
        Ok(u32::from_le_bytes([
            self.bytes[offset],
            self.bytes[offset + 1],
            self.bytes[offset + 2],
            self.bytes[offset + 3],
        ]))
    }

    fn write8(&mut self, addr: GuestAddr, value: u8) -> Result<(), MemoryAccessError> {
        let offset = self.offset_for(addr, 1)?;
        self.bytes[offset] = value;
        Ok(())
    }

    fn write16(&mut self, addr: GuestAddr, value: u16) -> Result<(), MemoryAccessError> {
        let offset = self.offset_for(addr, 2)?;
        let bytes = value.to_le_bytes();
        self.bytes[offset..offset + 2].copy_from_slice(&bytes);
        Ok(())
    }

    fn write32(&mut self, addr: GuestAddr, value: u32) -> Result<(), MemoryAccessError> {
        let offset = self.offset_for(addr, 4)?;
        let bytes = value.to_le_bytes();
        self.bytes[offset..offset + 4].copy_from_slice(&bytes);
        Ok(())
    }
}
