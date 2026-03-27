use std::fs;
use std::path::Path;

use vmrp_cpu::ExecutionMode;

use crate::CodeBlob;

const EXT_HEADER: &[u8; 8] = b"MRPGCMAP";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtFile {
    bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtLoadError {
    InvalidHeader,
    Io,
    Truncated,
}

impl ExtFile {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ExtLoadError> {
        if bytes.len() < EXT_HEADER.len() {
            return Err(ExtLoadError::Truncated);
        }

        if &bytes[..EXT_HEADER.len()] != EXT_HEADER {
            return Err(ExtLoadError::InvalidHeader);
        }

        Ok(Self {
            bytes: bytes.to_vec(),
        })
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, ExtLoadError> {
        let bytes = fs::read(path).map_err(|_| ExtLoadError::Io)?;
        Self::from_bytes(&bytes)
    }

    pub fn header(&self) -> &[u8] {
        &self.bytes[..EXT_HEADER.len()]
    }

    pub fn payload(&self) -> &[u8] {
        &self.bytes[EXT_HEADER.len()..]
    }

    pub fn entry_body(&self) -> &[u8] {
        self.payload()
    }

    pub fn entry_words(&self, count: usize) -> Result<Vec<u32>, ExtLoadError> {
        let body = self.entry_body();
        let required = count.checked_mul(4).ok_or(ExtLoadError::Truncated)?;
        if body.len() < required {
            return Err(ExtLoadError::Truncated);
        }

        let mut words = Vec::with_capacity(count);
        for index in 0..count {
            let offset = index * 4;
            words.push(u32::from_le_bytes([
                body[offset],
                body[offset + 1],
                body[offset + 2],
                body[offset + 3],
            ]));
        }
        Ok(words)
    }

    pub fn to_code_blob(&self, code_base: u32) -> CodeBlob {
        CodeBlob::with_entry(
            code_base,
            code_base + EXT_HEADER.len() as u32,
            self.bytes.clone(),
            ExecutionMode::Arm,
        )
    }
}
