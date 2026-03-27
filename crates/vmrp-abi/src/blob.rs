use vmrp_core::GuestAddr;
use vmrp_cpu::{ExecutionMode, MemoryAccessError, TestMemory};

use crate::image::GuestImage;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbiLoadError {
    Memory(MemoryAccessError),
}

impl From<MemoryAccessError> for AbiLoadError {
    fn from(value: MemoryAccessError) -> Self {
        Self::Memory(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeBlob {
    load_address: GuestAddr,
    entry: GuestAddr,
    bytes: Vec<u8>,
    mode: ExecutionMode,
}

impl CodeBlob {
    pub fn raw_arm(load_address: u32, bytes: Vec<u8>) -> Self {
        let load_address = GuestAddr::new(load_address);
        Self {
            load_address,
            entry: load_address,
            bytes,
            mode: ExecutionMode::Arm,
        }
    }

    pub fn with_entry(load_address: u32, entry: u32, bytes: Vec<u8>, mode: ExecutionMode) -> Self {
        Self {
            load_address: GuestAddr::new(load_address),
            entry: GuestAddr::new(entry),
            bytes,
            mode,
        }
    }

    pub fn arm(entry: u32, bytes: Vec<u8>) -> Self {
        Self::raw_arm(entry, bytes)
    }

    pub fn load_address(&self) -> GuestAddr {
        self.load_address
    }

    pub fn entry(&self) -> GuestAddr {
        self.entry
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_arm(&self) -> bool {
        self.mode == ExecutionMode::Arm
    }

    pub fn mode(&self) -> ExecutionMode {
        self.mode
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn load(&self) -> Result<LoadedImage, AbiLoadError> {
        let mut image = GuestImage::for_layout(vmrp_core::DEFAULT_LAYOUT);
        image.write_blob(self.load_address, &self.bytes)?;
        Ok(LoadedImage {
            memory: image.into_memory(),
            load_address: self.load_address,
            entry: self.entry,
            mode: self.mode,
        })
    }
}

#[derive(Clone, Debug)]
pub struct LoadedImage {
    memory: TestMemory,
    load_address: GuestAddr,
    entry: GuestAddr,
    mode: ExecutionMode,
}

impl LoadedImage {
    pub fn memory(&self) -> &TestMemory {
        &self.memory
    }

    pub fn load_address(&self) -> GuestAddr {
        self.load_address
    }

    pub fn entry(&self) -> GuestAddr {
        self.entry
    }

    pub fn mode(&self) -> ExecutionMode {
        self.mode
    }
}
