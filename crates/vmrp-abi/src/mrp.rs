use std::fs;
use std::io::Read;
use std::path::Path;

use flate2::read::GzDecoder;

use crate::ExtFile;

const MRP_HEADER_LEN: usize = 240;
const MRP_MAGIC: &[u8; 4] = b"MRPG";
const GZIP_MAGIC: &[u8; 3] = &[0x1F, 0x8B, 0x08];
const START_MR_NAME: &str = "start.mr";
const CFUNCTION_EXT_NAME: &str = "cfunction.ext";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MrpFile {
    bytes: Vec<u8>,
    header: MrpHeader,
    entries: Vec<MrpEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MrpHeader {
    file_start: u32,
    file_len: u32,
    list_start: u32,
    internal_name: String,
    app_name: String,
    appid: u32,
    version: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MrpEntry {
    name: String,
    offset: u32,
    len: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MrpRuntimeAssets {
    cfunction_ext: ExtFile,
    start_mr: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MrpLoadError {
    InvalidHeader,
    Io,
    Truncated,
    MalformedDirectory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MrpDecodeError {
    NotFound,
    InflateFailed,
    InvalidExtFormat,
}

impl MrpFile {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, MrpLoadError> {
        if bytes.len() < MRP_HEADER_LEN {
            return Err(MrpLoadError::Truncated);
        }

        if &bytes[..MRP_MAGIC.len()] != MRP_MAGIC {
            return Err(MrpLoadError::InvalidHeader);
        }

        let file_start = read_u32_at(bytes, 4)?
            .checked_add(8)
            .ok_or(MrpLoadError::MalformedDirectory)?;
        let file_len = read_u32_at(bytes, 8)?;
        let list_start = read_u32_at(bytes, 12)?;

        if (file_start as usize) > bytes.len() || (list_start as usize) > (file_start as usize) {
            return Err(MrpLoadError::MalformedDirectory);
        }

        if file_len > 0 && (file_len as usize) > bytes.len() {
            return Err(MrpLoadError::MalformedDirectory);
        }

        let header = MrpHeader {
            file_start,
            file_len,
            list_start,
            internal_name: parse_c_string(&bytes[16..28]),
            app_name: parse_c_string(&bytes[28..52]),
            appid: read_u32_at(bytes, 68)?,
            version: read_u32_at(bytes, 72)?,
        };

        let mut entries = Vec::new();
        let mut cursor = list_start as usize;
        let end = file_start as usize;

        while cursor < end {
            let name_len = read_u32_at(bytes, cursor)? as usize;
            cursor += 4;

            if name_len == 0 || cursor + name_len + 12 > end {
                return Err(MrpLoadError::MalformedDirectory);
            }

            let name = parse_c_string(&bytes[cursor..cursor + name_len]);
            cursor += name_len;
            let offset = read_u32_at(bytes, cursor)?;
            cursor += 4;
            let len = read_u32_at(bytes, cursor)?;
            cursor += 4;
            let _reserved = read_u32_at(bytes, cursor)?;
            cursor += 4;

            if name.is_empty() {
                return Err(MrpLoadError::MalformedDirectory);
            }

            let data_start = offset as usize;
            let data_end = data_start
                .checked_add(len as usize)
                .ok_or(MrpLoadError::MalformedDirectory)?;
            if data_end > bytes.len() {
                return Err(MrpLoadError::MalformedDirectory);
            }

            entries.push(MrpEntry { name, offset, len });
        }

        if cursor != end {
            return Err(MrpLoadError::MalformedDirectory);
        }

        Ok(Self {
            bytes: bytes.to_vec(),
            header,
            entries,
        })
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, MrpLoadError> {
        let bytes = fs::read(path).map_err(|_| MrpLoadError::Io)?;
        Self::from_bytes(&bytes)
    }

    pub fn magic(&self) -> &[u8] {
        &self.bytes[..4]
    }

    pub fn header(&self) -> &MrpHeader {
        &self.header
    }

    pub fn internal_name(&self) -> &str {
        &self.header.internal_name
    }

    pub fn app_name(&self) -> &str {
        &self.header.app_name
    }

    pub fn entries(&self) -> &[MrpEntry] {
        &self.entries
    }

    pub fn entry(&self, name: &str) -> Option<&MrpEntry> {
        self.entries.iter().find(|entry| entry.name == name)
    }

    pub fn file_bytes(&self, name: &str) -> Option<&[u8]> {
        let entry = self.entry(name)?;
        let start = entry.offset as usize;
        let end = start + entry.len as usize;
        Some(&self.bytes[start..end])
    }

    pub fn file_bytes_inflated(&self, name: &str) -> Result<Vec<u8>, MrpDecodeError> {
        let bytes = self.file_bytes(name).ok_or(MrpDecodeError::NotFound)?;

        if bytes.len() >= GZIP_MAGIC.len() && &bytes[..GZIP_MAGIC.len()] == GZIP_MAGIC {
            let mut decoder = GzDecoder::new(bytes);
            let mut out = Vec::new();
            decoder
                .read_to_end(&mut out)
                .map_err(|_| MrpDecodeError::InflateFailed)?;
            Ok(out)
        } else {
            Ok(bytes.to_vec())
        }
    }

    pub fn runtime_assets(&self) -> Result<MrpRuntimeAssets, MrpDecodeError> {
        let ext_bytes = self.file_bytes_inflated(CFUNCTION_EXT_NAME)?;
        let cfunction_ext =
            ExtFile::from_bytes(&ext_bytes).map_err(|_| MrpDecodeError::InvalidExtFormat)?;
        self.runtime_assets_with_ext(cfunction_ext)
    }

    pub fn runtime_assets_with_ext(
        &self,
        cfunction_ext: ExtFile,
    ) -> Result<MrpRuntimeAssets, MrpDecodeError> {
        let start_mr = self.load_start_mr()?;

        Ok(MrpRuntimeAssets {
            cfunction_ext,
            start_mr,
        })
    }

    fn load_start_mr(&self) -> Result<Vec<u8>, MrpDecodeError> {
        if self.entry(START_MR_NAME).is_some() {
            self.file_bytes_inflated(START_MR_NAME)
        } else {
            let fallback_name = self
                .entries()
                .iter()
                .find(|entry| entry.name().ends_with(".mr"))
                .map(|entry| entry.name().to_string())
                .ok_or(MrpDecodeError::NotFound)?;
            self.file_bytes_inflated(&fallback_name)
        }
    }
}

impl MrpRuntimeAssets {
    pub fn cfunction_ext(&self) -> &ExtFile {
        &self.cfunction_ext
    }

    pub fn start_mr(&self) -> &[u8] {
        &self.start_mr
    }
}

impl MrpHeader {
    pub fn file_start(&self) -> u32 {
        self.file_start
    }

    pub fn file_len(&self) -> u32 {
        self.file_len
    }

    pub fn list_start(&self) -> u32 {
        self.list_start
    }

    pub fn appid(&self) -> u32 {
        self.appid
    }

    pub fn version(&self) -> u32 {
        self.version
    }
}

impl MrpEntry {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn offset(&self) -> u32 {
        self.offset
    }

    pub fn len(&self) -> u32 {
        self.len
    }
}

fn read_u32_at(bytes: &[u8], offset: usize) -> Result<u32, MrpLoadError> {
    if offset + 4 > bytes.len() {
        return Err(MrpLoadError::Truncated);
    }

    Ok(u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ]))
}

fn parse_c_string(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).to_string()
}
