const MRP_SIGNATURE: &[u8; 4] = b"\x1BMRP";
const VERSION_MAX: u8 = 0x80;
const VERSION_MIN: u8 = 0x50;
const INSTRUCTION_SIZE_BYTES: usize = 4;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MrChunk {
    header: MrChunkHeader,
    main: MrFunction,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MrChunkHeader {
    version: u8,
    little_endian: bool,
    number_size: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MrFunction {
    source: Option<String>,
    line_defined: u32,
    nups: u8,
    num_params: u8,
    is_vararg: u8,
    max_stack_size: u8,
    line_count: u32,
    local_count: u32,
    upvalue_count: u32,
    constant_count: u32,
    code_count: u32,
    children: Vec<MrFunction>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MrChunkError {
    Truncated,
    InvalidSignature,
    UnsupportedVersion { found: u8 },
    InvalidFormat,
}

impl MrChunk {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, MrChunkError> {
        let mut header_cursor = Cursor::new(bytes, true, 8);
        header_cursor.expect_bytes(MRP_SIGNATURE)?;

        let version = header_cursor.read_u8()?;
        if version > VERSION_MAX || version < VERSION_MIN {
            return Err(MrChunkError::UnsupportedVersion { found: version });
        }

        let little_endian = header_cursor.read_u8()? != 0;
        let header_end = header_cursor.pos;

        let mut last_error = MrChunkError::InvalidFormat;
        for number_size in [8usize, 4usize] {
            let mut cursor = Cursor::new(bytes, little_endian, number_size);
            cursor.pos = header_end;

            match cursor.parse_function() {
                Ok(main) if cursor.remaining() == 0 => {
                    return Ok(Self {
                        header: MrChunkHeader {
                            version,
                            little_endian,
                            number_size,
                        },
                        main,
                    });
                }
                Ok(_) => {
                    last_error = MrChunkError::InvalidFormat;
                }
                Err(err) => {
                    last_error = err;
                }
            }
        }

        Err(last_error)
    }

    pub fn header(&self) -> &MrChunkHeader {
        &self.header
    }

    pub fn main(&self) -> &MrFunction {
        &self.main
    }
}

impl MrChunkHeader {
    pub fn version(&self) -> u8 {
        self.version
    }

    pub fn little_endian(&self) -> bool {
        self.little_endian
    }

    pub fn number_size(&self) -> usize {
        self.number_size
    }
}

impl MrFunction {
    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    pub fn line_defined(&self) -> u32 {
        self.line_defined
    }

    pub fn nups(&self) -> u8 {
        self.nups
    }

    pub fn num_params(&self) -> u8 {
        self.num_params
    }

    pub fn is_vararg(&self) -> u8 {
        self.is_vararg
    }

    pub fn max_stack_size(&self) -> u8 {
        self.max_stack_size
    }

    pub fn line_count(&self) -> u32 {
        self.line_count
    }

    pub fn local_count(&self) -> u32 {
        self.local_count
    }

    pub fn upvalue_count(&self) -> u32 {
        self.upvalue_count
    }

    pub fn constant_count(&self) -> u32 {
        self.constant_count
    }

    pub fn code_count(&self) -> u32 {
        self.code_count
    }

    pub fn children(&self) -> &[MrFunction] {
        &self.children
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
    little_endian: bool,
    number_size: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8], little_endian: bool, number_size: usize) -> Self {
        Self {
            bytes,
            pos: 0,
            little_endian,
            number_size,
        }
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.pos)
    }

    fn expect_bytes(&mut self, expected: &[u8]) -> Result<(), MrChunkError> {
        if self.remaining() < expected.len() {
            return Err(MrChunkError::Truncated);
        }
        if &self.bytes[self.pos..self.pos + expected.len()] != expected {
            return Err(MrChunkError::InvalidSignature);
        }
        self.pos += expected.len();
        Ok(())
    }

    fn read_u8(&mut self) -> Result<u8, MrChunkError> {
        if self.remaining() < 1 {
            return Err(MrChunkError::Truncated);
        }
        let value = self.bytes[self.pos];
        self.pos += 1;
        Ok(value)
    }

    fn read_u32(&mut self) -> Result<u32, MrChunkError> {
        if self.remaining() < 4 {
            return Err(MrChunkError::Truncated);
        }
        let raw = [
            self.bytes[self.pos],
            self.bytes[self.pos + 1],
            self.bytes[self.pos + 2],
            self.bytes[self.pos + 3],
        ];
        self.pos += 4;
        Ok(if self.little_endian {
            u32::from_le_bytes(raw)
        } else {
            u32::from_be_bytes(raw)
        })
    }

    fn read_i32(&mut self) -> Result<i32, MrChunkError> {
        Ok(self.read_u32()? as i32)
    }

    fn skip(&mut self, bytes: usize) -> Result<(), MrChunkError> {
        if self.remaining() < bytes {
            return Err(MrChunkError::Truncated);
        }
        self.pos += bytes;
        Ok(())
    }

    fn read_string(&mut self) -> Result<Option<String>, MrChunkError> {
        let size = self.read_u32()? as usize;
        if size == 0 {
            return Ok(None);
        }

        if self.remaining() < size {
            return Err(MrChunkError::Truncated);
        }

        let raw = &self.bytes[self.pos..self.pos + size];
        self.pos += size;

        let content = if raw.last() == Some(&0) {
            &raw[..raw.len().saturating_sub(1)]
        } else {
            raw
        };
        Ok(Some(String::from_utf8_lossy(content).to_string()))
    }

    fn parse_function(&mut self) -> Result<MrFunction, MrChunkError> {
        let source = self.read_string()?;
        let line_defined = self.read_i32()?;
        if line_defined < 0 {
            return Err(MrChunkError::InvalidFormat);
        }

        let nups = self.read_u8()?;
        let num_params = self.read_u8()?;
        let is_vararg = self.read_u8()?;
        let max_stack_size = self.read_u8()?;

        let line_count = self.read_i32()?;
        if line_count < 0 {
            return Err(MrChunkError::InvalidFormat);
        }
        self.skip((line_count as usize) * 4)?;

        let local_count = self.read_i32()?;
        if local_count < 0 {
            return Err(MrChunkError::InvalidFormat);
        }
        for _ in 0..local_count {
            let _ = self.read_string()?;
            let start_pc = self.read_i32()?;
            let end_pc = self.read_i32()?;
            if start_pc < 0 || end_pc < 0 {
                return Err(MrChunkError::InvalidFormat);
            }
        }

        let upvalue_count = self.read_i32()?;
        if upvalue_count < 0 {
            return Err(MrChunkError::InvalidFormat);
        }
        for _ in 0..upvalue_count {
            let _ = self.read_string()?;
        }

        let constant_count = self.read_i32()?;
        if constant_count < 0 {
            return Err(MrChunkError::InvalidFormat);
        }
        for _ in 0..constant_count {
            let tag = self.read_u8()?;
            match tag {
                0 => {}
                3 => self.skip(self.number_size)?,
                4 => {
                    let _ = self.read_string()?;
                }
                _ => return Err(MrChunkError::InvalidFormat),
            }
        }

        let child_count = self.read_i32()?;
        if child_count < 0 {
            return Err(MrChunkError::InvalidFormat);
        }
        let mut children = Vec::with_capacity(child_count as usize);
        for _ in 0..child_count {
            children.push(self.parse_function()?);
        }

        let code_count = self.read_i32()?;
        if code_count < 0 {
            return Err(MrChunkError::InvalidFormat);
        }
        self.skip((code_count as usize) * INSTRUCTION_SIZE_BYTES)?;

        Ok(MrFunction {
            source,
            line_defined: line_defined as u32,
            nups,
            num_params,
            is_vararg,
            max_stack_size,
            line_count: line_count as u32,
            local_count: local_count as u32,
            upvalue_count: upvalue_count as u32,
            constant_count: constant_count as u32,
            code_count: code_count as u32,
            children,
        })
    }
}
