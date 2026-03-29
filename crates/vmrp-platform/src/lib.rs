use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use vmrp_core::GuestAddr;
use vmrp_cpu::{Cpu, ExecutionMode, MemoryAccessError, MemoryBus};

pub const MR_MALLOC_OFFSET: u32 = 0x0;
pub const MR_FREE_OFFSET: u32 = 0x04;
pub const MR_REALLOC_OFFSET: u32 = 0x08;
pub const MEMCPY_OFFSET: u32 = 0x0C;
pub const MEMSET_OFFSET: u32 = 0x38;
pub const MR_C_FUNCTION_NEW_OFFSET: u32 = 0x64;
pub const DSM_REQUIRE_FUNCS_SIZE: u32 = 0xD0;
pub const DSM_FLAGS_OFFSET: u32 = 0xCC;
pub const VMRP_VER: i32 = 20210701;
pub const MR_SUCCESS: i32 = 0;
pub const MR_FAILED: i32 = -1;
pub const FLAG_USE_UTF8_FS: u32 = 1 << 0;
pub const FLAG_USE_UTF8_EDIT: u32 = 1 << 1;

const MOV_R0_IMM0: u32 = 0xE3A0_0000;
const BX_LR: u32 = 0xE12F_FF1E;
const MR_FILE_WRONLY: u32 = 2;
const MR_FILE_RDWR: u32 = 4;
const MR_FILE_CREATE: u32 = 8;
const MR_FILE_RECREATE: u32 = 16;
const MR_SEEK_SET: u32 = 0;
const MR_SEEK_CUR: u32 = 1;
const MR_SEEK_END: u32 = 2;
const MR_IS_FILE: i32 = 1;
const MR_IS_DIR: i32 = 2;
const MR_IS_INVALID: i32 = 8;
const DSM_MEM_GET_SIZE: u32 = 4 * 1024 * 1024;
const SCREEN_WIDTH: usize = 240;
const SCREEN_HEIGHT: usize = 320;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtBootstrap {
    pub mr_table_addr: GuestAddr,
    pub mr_c_function_new_addr: GuestAddr,
    pub mr_c_function_p_addr: GuestAddr,
    pub mr_malloc_addr: GuestAddr,
    pub mr_free_addr: GuestAddr,
    pub mr_realloc_addr: GuestAddr,
    pub mr_malloc_result_addr: GuestAddr,
    pub memcpy_addr: GuestAddr,
    pub memset_addr: GuestAddr,
}

impl ExtBootstrap {
    pub fn apply<B: MemoryBus>(
        &self,
        memory: &mut B,
        code_base: GuestAddr,
    ) -> Result<(), MemoryAccessError> {
        memory.write32(code_base, self.mr_table_addr.get())?;
        memory.write32(
            GuestAddr::new(code_base.get().wrapping_add(4)),
            self.mr_c_function_p_addr.get(),
        )?;

        // Seed a non-null default for early table lookups so unresolved entries
        // return safely instead of jumping to address 0.
        for offset in (0..=0x240u32).step_by(4) {
            let slot = GuestAddr::new(self.mr_table_addr.get().wrapping_add(offset));
            memory.write32(slot, self.mr_free_addr.get())?;
        }

        let mr_malloc_slot = GuestAddr::new(self.mr_table_addr.get().wrapping_add(MR_MALLOC_OFFSET));
        memory.write32(mr_malloc_slot, self.mr_malloc_addr.get())?;

        let mr_free_slot = GuestAddr::new(self.mr_table_addr.get().wrapping_add(MR_FREE_OFFSET));
        memory.write32(mr_free_slot, self.mr_free_addr.get())?;

        let mr_realloc_slot = GuestAddr::new(self.mr_table_addr.get().wrapping_add(MR_REALLOC_OFFSET));
        memory.write32(mr_realloc_slot, self.mr_realloc_addr.get())?;

        let memcpy_slot = GuestAddr::new(self.mr_table_addr.get().wrapping_add(MEMCPY_OFFSET));
        memory.write32(memcpy_slot, self.memcpy_addr.get())?;

        let memset_slot = GuestAddr::new(self.mr_table_addr.get().wrapping_add(MEMSET_OFFSET));
        memory.write32(memset_slot, self.memset_addr.get())?;

        let c_function_new_slot =
            GuestAddr::new(self.mr_table_addr.get().wrapping_add(MR_C_FUNCTION_NEW_OFFSET));
        memory.write32(c_function_new_slot, self.mr_c_function_new_addr.get())?;

        // Host callback stubs. Actual behavior is implemented in ExtHost::handle.
        memory.write32(self.mr_c_function_new_addr, MOV_R0_IMM0)?;
        memory.write32(
            GuestAddr::new(self.mr_c_function_new_addr.get().wrapping_add(4)),
            BX_LR,
        )?;

        memory.write32(self.mr_malloc_addr, MOV_R0_IMM0)?;
        memory.write32(
            GuestAddr::new(self.mr_malloc_addr.get().wrapping_add(4)),
            BX_LR,
        )?;
        memory.write32(
            GuestAddr::new(self.mr_malloc_addr.get().wrapping_add(8)),
            self.mr_malloc_result_addr.get(),
        )?;

        memory.write32(self.mr_free_addr, MOV_R0_IMM0)?;
        memory.write32(
            GuestAddr::new(self.mr_free_addr.get().wrapping_add(4)),
            BX_LR,
        )?;
        memory.write32(self.mr_realloc_addr, MOV_R0_IMM0)?;
        memory.write32(
            GuestAddr::new(self.mr_realloc_addr.get().wrapping_add(4)),
            BX_LR,
        )?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DsmHostFn {
    Test,
    Log,
    Exit,
    Srand,
    Rand,
    MemGet,
    MemFree,
    TimerStart,
    TimerStop,
    GetUptimeMs,
    GetDatetime,
    Sleep,
    Open,
    Close,
    Read,
    Write,
    Seek,
    Info,
    Remove,
    Rename,
    MkDir,
    RmDir,
    OpenDir,
    ReadDir,
    CloseDir,
    GetLen,
    DrawBitmap,
    UnsupportedFailed,
}

#[derive(Debug)]
struct HostFile {
    file: File,
}

#[derive(Debug)]
struct HostDir {
    entries: Vec<String>,
    cursor: usize,
    scratch_ptr: u32,
}

#[derive(Clone, Copy, Debug)]
struct HostDateTime {
    year: u16,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostTimerCommand {
    Start(u32),
    Stop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostScreenRegion {
    pub x: i32,
    pub y: i32,
    pub w: u16,
    pub h: u16,
}

#[derive(Debug)]
pub struct ExtHost {
    pub mr_c_function_new_addr: GuestAddr,
    pub mr_c_function_p_addr: GuestAddr,
    pub mr_malloc_addr: GuestAddr,
    pub mr_free_addr: GuestAddr,
    pub mr_realloc_addr: GuestAddr,
    pub memcpy_addr: GuestAddr,
    pub memset_addr: GuestAddr,
    pub alloc_base: GuestAddr,
    pub alloc_size: u32,
    ext_helper_addr: Option<GuestAddr>,
    next_alloc: u32,
    next_callback: u32,
    dsm_callbacks: BTreeMap<u32, DsmHostFn>,
    working_dir: PathBuf,
    verbose: bool,
    exit_requested: bool,
    rng_state: u32,
    pending_timer_delay_ms: Option<u32>,
    pending_timer_command: Option<HostTimerCommand>,
    uptime_epoch: Instant,
    screen_buffer: Vec<u16>,
    dirty_region: Option<HostScreenRegion>,
    files: BTreeMap<i32, HostFile>,
    next_file_handle: i32,
    dirs: BTreeMap<i32, HostDir>,
    next_dir_handle: i32,
}

impl ExtHost {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        mr_c_function_new_addr: GuestAddr,
        mr_c_function_p_addr: GuestAddr,
        mr_malloc_addr: GuestAddr,
        mr_free_addr: GuestAddr,
        mr_realloc_addr: GuestAddr,
        memcpy_addr: GuestAddr,
        memset_addr: GuestAddr,
        alloc_base: GuestAddr,
        alloc_size: u32,
    ) -> Self {
        Self {
            mr_c_function_new_addr,
            mr_c_function_p_addr,
            mr_malloc_addr,
            mr_free_addr,
            mr_realloc_addr,
            memcpy_addr,
            memset_addr,
            alloc_base,
            alloc_size,
            ext_helper_addr: None,
            next_alloc: alloc_base.get(),
            next_callback: alloc_base.get().wrapping_add(alloc_size).wrapping_add(0x1000),
            dsm_callbacks: BTreeMap::new(),
            working_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            verbose: false,
            exit_requested: false,
            rng_state: 0x1234_5678,
            pending_timer_delay_ms: None,
            pending_timer_command: None,
            uptime_epoch: Instant::now(),
            screen_buffer: vec![0; SCREEN_WIDTH * SCREEN_HEIGHT],
            dirty_region: None,
            files: BTreeMap::new(),
            next_file_handle: 3,
            dirs: BTreeMap::new(),
            next_dir_handle: 1000,
        }
    }

    pub fn ext_helper_addr(&self) -> Option<GuestAddr> {
        self.ext_helper_addr
    }

    pub fn c_function_p_addr(&self) -> GuestAddr {
        self.mr_c_function_p_addr
    }

    pub fn reset_alloc(&mut self) {
        self.next_alloc = self.alloc_base.get();
    }

    pub fn set_working_dir<P: Into<PathBuf>>(&mut self, path: P) {
        self.working_dir = path.into();
    }

    pub fn set_verbose(&mut self, verbose: bool) {
        self.verbose = verbose;
    }

    pub fn exit_requested(&self) -> bool {
        self.exit_requested
    }

    pub fn pending_timer_delay_ms(&self) -> Option<u32> {
        self.pending_timer_delay_ms
    }

    pub fn take_timer_command(&mut self) -> Option<HostTimerCommand> {
        self.pending_timer_command.take()
    }

    pub fn screen_buffer(&self) -> &[u16] {
        &self.screen_buffer
    }

    pub fn take_dirty_region(&mut self) -> Option<HostScreenRegion> {
        self.dirty_region.take()
    }

    pub fn install_dsm_require_funcs<B: MemoryBus>(
        &mut self,
        memory: &mut B,
        table_addr: GuestAddr,
        flags: u32,
    ) -> Result<(), MemoryAccessError> {
        let callbacks = [
            (0x00, DsmHostFn::Test),
            (0x04, DsmHostFn::Log),
            (0x08, DsmHostFn::Exit),
            (0x0C, DsmHostFn::Srand),
            (0x10, DsmHostFn::Rand),
            (0x14, DsmHostFn::MemGet),
            (0x18, DsmHostFn::MemFree),
            (0x1C, DsmHostFn::TimerStart),
            (0x20, DsmHostFn::TimerStop),
            (0x24, DsmHostFn::GetUptimeMs),
            (0x28, DsmHostFn::GetDatetime),
            (0x2C, DsmHostFn::Sleep),
            (0x30, DsmHostFn::Open),
            (0x34, DsmHostFn::Close),
            (0x38, DsmHostFn::Read),
            (0x3C, DsmHostFn::Write),
            (0x40, DsmHostFn::Seek),
            (0x44, DsmHostFn::Info),
            (0x48, DsmHostFn::Remove),
            (0x4C, DsmHostFn::Rename),
            (0x50, DsmHostFn::MkDir),
            (0x54, DsmHostFn::RmDir),
            (0x58, DsmHostFn::OpenDir),
            (0x5C, DsmHostFn::ReadDir),
            (0x60, DsmHostFn::CloseDir),
            (0x64, DsmHostFn::GetLen),
            (0x68, DsmHostFn::DrawBitmap),
            (0x6C, DsmHostFn::UnsupportedFailed),
            (0x70, DsmHostFn::UnsupportedFailed),
            (0x74, DsmHostFn::UnsupportedFailed),
            (0x78, DsmHostFn::UnsupportedFailed),
            (0x7C, DsmHostFn::UnsupportedFailed),
            (0x80, DsmHostFn::UnsupportedFailed),
            (0x84, DsmHostFn::UnsupportedFailed),
            (0x88, DsmHostFn::UnsupportedFailed),
            (0x8C, DsmHostFn::UnsupportedFailed),
            (0x90, DsmHostFn::UnsupportedFailed),
            (0x94, DsmHostFn::UnsupportedFailed),
            (0x98, DsmHostFn::UnsupportedFailed),
            (0x9C, DsmHostFn::UnsupportedFailed),
            (0xA0, DsmHostFn::UnsupportedFailed),
            (0xA4, DsmHostFn::UnsupportedFailed),
            (0xA8, DsmHostFn::UnsupportedFailed),
            (0xAC, DsmHostFn::UnsupportedFailed),
            (0xB0, DsmHostFn::UnsupportedFailed),
            (0xB4, DsmHostFn::UnsupportedFailed),
            (0xB8, DsmHostFn::UnsupportedFailed),
            (0xBC, DsmHostFn::UnsupportedFailed),
            (0xC0, DsmHostFn::UnsupportedFailed),
            (0xC4, DsmHostFn::UnsupportedFailed),
            (0xC8, DsmHostFn::UnsupportedFailed),
        ];

        for (offset, kind) in callbacks {
            let slot = GuestAddr::new(table_addr.get().wrapping_add(offset));
            self.register_dsm_callback(memory, slot, kind)?;
        }

        memory.write32(
            GuestAddr::new(table_addr.get().wrapping_add(DSM_FLAGS_OFFSET)),
            flags,
        )?;
        Ok(())
    }

    pub fn handle<B: MemoryBus>(&mut self, cpu: &mut Cpu<B>) -> Result<bool, MemoryAccessError> {
        let pc = cpu.regs().pc();
        if pc == self.mr_c_function_new_addr.get() {
            self.handle_mr_c_function_new(cpu)?;
            return Ok(true);
        }

        if pc == self.mr_malloc_addr.get() {
            self.handle_mr_malloc(cpu);
            return Ok(true);
        }

        if pc == self.mr_free_addr.get() {
            self.handle_mr_free(cpu);
            return Ok(true);
        }

        if pc == self.mr_realloc_addr.get() {
            self.handle_mr_realloc(cpu)?;
            return Ok(true);
        }

        if pc == self.memcpy_addr.get() {
            self.handle_memcpy(cpu)?;
            return Ok(true);
        }

        if pc == self.memset_addr.get() {
            self.handle_memset(cpu)?;
            return Ok(true);
        }

        if let Some(callback) = self.dsm_callbacks.get(&pc).copied() {
            self.handle_dsm_callback(cpu, callback)?;
            return Ok(true);
        }

        Ok(false)
    }

    fn handle_mr_c_function_new<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let helper = cpu.regs().reg(0);
        let len = cpu.regs().reg(1);
        if helper != 0 {
            self.ext_helper_addr = Some(GuestAddr::new(helper));
        }

        let context_addr = self.mr_c_function_p_addr.get();
        for offset in 0..len {
            cpu.memory_mut()
                .write8(GuestAddr::new(context_addr.wrapping_add(offset)), 0)?;
        }

        cpu.regs_mut().set_reg(0, context_addr);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_mr_malloc<B: MemoryBus>(&mut self, cpu: &mut Cpu<B>) {
        let requested = cpu.regs().reg(0).max(1);
        let out = self.alloc(requested).unwrap_or(0);
        cpu.regs_mut().set_reg(0, out);
        return_to_lr(cpu);
    }

    fn handle_mr_free<B: MemoryBus>(&self, cpu: &mut Cpu<B>) {
        cpu.regs_mut().set_reg(0, 0);
        return_to_lr(cpu);
    }

    fn handle_mr_realloc<B: MemoryBus>(&mut self, cpu: &mut Cpu<B>) -> Result<(), MemoryAccessError> {
        let old_ptr = cpu.regs().reg(0);
        let new_len = cpu.regs().reg(1);

        let out = if new_len == 0 {
            0
        } else if old_ptr == 0 {
            self.alloc(new_len).unwrap_or(0)
        } else if let Some(new_ptr) = self.alloc(new_len) {
            for offset in 0..new_len {
                let byte = cpu.memory().read8(GuestAddr::new(old_ptr.wrapping_add(offset)))?;
                cpu.memory_mut()
                    .write8(GuestAddr::new(new_ptr.wrapping_add(offset)), byte)?;
            }
            new_ptr
        } else {
            0
        };

        cpu.regs_mut().set_reg(0, out);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_memcpy<B: MemoryBus>(&self, cpu: &mut Cpu<B>) -> Result<(), MemoryAccessError> {
        let dst = cpu.regs().reg(0);
        let src = cpu.regs().reg(1);
        let len = cpu.regs().reg(2);

        for offset in 0..len {
            let value = cpu.memory().read8(GuestAddr::new(src.wrapping_add(offset)))?;
            cpu.memory_mut()
                .write8(GuestAddr::new(dst.wrapping_add(offset)), value)?;
        }

        cpu.regs_mut().set_reg(0, dst);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_memset<B: MemoryBus>(&self, cpu: &mut Cpu<B>) -> Result<(), MemoryAccessError> {
        let dst = cpu.regs().reg(0);
        let value = cpu.regs().reg(1) as u8;
        let len = cpu.regs().reg(2);

        for offset in 0..len {
            cpu.memory_mut()
                .write8(GuestAddr::new(dst.wrapping_add(offset)), value)?;
        }

        cpu.regs_mut().set_reg(0, dst);
        return_to_lr(cpu);
        Ok(())
    }

    fn alloc(&mut self, size: u32) -> Option<u32> {
        let aligned = align_up(size, 4);
        let end = self.alloc_base.get().wrapping_add(self.alloc_size);
        if self.next_alloc > end {
            return None;
        }
        if aligned > end.wrapping_sub(self.next_alloc) {
            return None;
        }

        let ptr = self.next_alloc;
        self.next_alloc = self.next_alloc.wrapping_add(aligned);
        Some(ptr)
    }

    fn register_dsm_callback<B: MemoryBus>(
        &mut self,
        memory: &mut B,
        slot: GuestAddr,
        callback: DsmHostFn,
    ) -> Result<(), MemoryAccessError> {
        let callback_addr = self.next_callback;
        self.next_callback = self.next_callback.wrapping_add(4);
        self.dsm_callbacks.insert(callback_addr, callback);
        memory.write32(slot, callback_addr)
    }

    fn handle_dsm_callback<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
        callback: DsmHostFn,
    ) -> Result<(), MemoryAccessError> {
        match callback {
            DsmHostFn::Test => {
                return_to_lr(cpu);
            }
            DsmHostFn::Log => {
                let msg_addr = cpu.regs().reg(0);
                let msg = read_guest_c_string(cpu, msg_addr, 4096)?;
                if self.verbose && !msg.is_empty() {
                    println!("[guest-log] {msg}");
                }
                return_to_lr(cpu);
            }
            DsmHostFn::Exit => {
                self.exit_requested = true;
                return_to_lr(cpu);
            }
            DsmHostFn::Srand => {
                self.rng_state = cpu.regs().reg(0);
                return_to_lr(cpu);
            }
            DsmHostFn::Rand => {
                self.rng_state = self
                    .rng_state
                    .wrapping_mul(214013)
                    .wrapping_add(2531011);
                cpu.regs_mut()
                    .set_reg(0, (self.rng_state >> 16) & 0x7FFF);
                return_to_lr(cpu);
            }
            DsmHostFn::MemGet => {
                let mem_base_ptr = cpu.regs().reg(0);
                let mem_len_ptr = cpu.regs().reg(1);
                if let Some(ptr) = self.alloc(DSM_MEM_GET_SIZE) {
                    cpu.memory_mut().write32(GuestAddr::new(mem_base_ptr), ptr)?;
                    cpu.memory_mut()
                        .write32(GuestAddr::new(mem_len_ptr), DSM_MEM_GET_SIZE)?;

                cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
                } else {
                    cpu.regs_mut().set_reg(0, MR_FAILED as u32);
                }
                return_to_lr(cpu);
            }
            DsmHostFn::MemFree => {

                cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
                return_to_lr(cpu);
            }
            DsmHostFn::TimerStart => {
                let delay = cpu.regs().reg(0);
                self.pending_timer_delay_ms = Some(delay);
                self.pending_timer_command = Some(HostTimerCommand::Start(delay));

                cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
                return_to_lr(cpu);
            }
            DsmHostFn::TimerStop => {
                self.pending_timer_delay_ms = None;
                self.pending_timer_command = Some(HostTimerCommand::Stop);

                cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
                return_to_lr(cpu);
            }
            DsmHostFn::GetUptimeMs => {
                let elapsed = self.uptime_epoch.elapsed().as_millis() as u32;
                cpu.regs_mut().set_reg(0, elapsed);
                return_to_lr(cpu);
            }
            DsmHostFn::GetDatetime => {
                let out = cpu.regs().reg(0);
                if out == 0 {
                    cpu.regs_mut().set_reg(0, MR_FAILED as u32);
                    return_to_lr(cpu);
                    return Ok(());
                }

                let datetime = current_local_datetime();
                let [year_lo, year_hi] = datetime.year.to_le_bytes();
                cpu.memory_mut().write8(GuestAddr::new(out), year_lo)?;
                cpu.memory_mut()
                    .write8(GuestAddr::new(out.wrapping_add(1)), year_hi)?;
                cpu.memory_mut()
                    .write8(GuestAddr::new(out.wrapping_add(2)), datetime.month)?;
                cpu.memory_mut()
                    .write8(GuestAddr::new(out.wrapping_add(3)), datetime.day)?;
                cpu.memory_mut()
                    .write8(GuestAddr::new(out.wrapping_add(4)), datetime.hour)?;
                cpu.memory_mut()
                    .write8(GuestAddr::new(out.wrapping_add(5)), datetime.minute)?;
                cpu.memory_mut()
                    .write8(GuestAddr::new(out.wrapping_add(6)), datetime.second)?;

                cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
                return_to_lr(cpu);
            }
            DsmHostFn::Sleep => {
                let ms = cpu.regs().reg(0);
                if ms > 0 {
                    thread::sleep(Duration::from_millis(ms as u64));
                }

                cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
                return_to_lr(cpu);
            }
            DsmHostFn::Open => {
                let name_addr = cpu.regs().reg(0);
                let mode = cpu.regs().reg(1);
                let name = read_guest_c_string(cpu, name_addr, 1024)?;
                let path = self.resolve_guest_path(&name);

                let mut opts = OpenOptions::new();
                if mode & MR_FILE_RDWR != 0 {
                    opts.read(true).write(true);
                } else if mode & MR_FILE_WRONLY != 0 {
                    opts.write(true);
                } else {
                    opts.read(true);
                }
                if mode & MR_FILE_CREATE != 0 {
                    opts.create(true);
                }
                if mode & MR_FILE_RECREATE != 0 {
                    opts.create(true).truncate(true);
                }

                let ret = match opts.open(path) {
                    Ok(file) => {
                        let fd = self.next_file_handle;
                        self.next_file_handle = self.next_file_handle.saturating_add(1);
                        self.files.insert(fd, HostFile { file });
                        fd
                    }
                    Err(_) => MR_FAILED,
                };
                cpu.regs_mut().set_reg(0, ret as u32);
                return_to_lr(cpu);
            }
            DsmHostFn::Close => {
                let fd = cpu.regs().reg(0) as i32;
                let ret = if self.files.remove(&fd).is_some() {
                    MR_SUCCESS
                } else {
                    MR_FAILED
                };
                cpu.regs_mut().set_reg(0, ret as u32);
                return_to_lr(cpu);
            }
            DsmHostFn::Read => {
                let fd = cpu.regs().reg(0) as i32;
                let buffer_ptr = cpu.regs().reg(1);
                let len = cpu.regs().reg(2) as usize;
                let mut ret = MR_FAILED;

                if let Some(file) = self.files.get_mut(&fd) {
                    let mut buffer = vec![0u8; len];
                    match file.file.read(&mut buffer) {
                        Ok(read_len) => {
                            for (index, byte) in buffer[..read_len].iter().enumerate() {
                                cpu.memory_mut().write8(
                                    GuestAddr::new(buffer_ptr.wrapping_add(index as u32)),
                                    *byte,
                                )?;
                            }
                            ret = read_len as i32;
                        }
                        Err(_) => {
                            ret = MR_FAILED;
                        }
                    }
                }

                cpu.regs_mut().set_reg(0, ret as u32);
                return_to_lr(cpu);
            }
            DsmHostFn::Write => {
                let fd = cpu.regs().reg(0) as i32;
                let buffer_ptr = cpu.regs().reg(1);
                let len = cpu.regs().reg(2) as usize;
                let mut ret = MR_FAILED;

                if let Some(file) = self.files.get_mut(&fd) {
                    let mut buffer = vec![0u8; len];
                    for (index, byte) in buffer.iter_mut().enumerate() {
                        *byte = cpu
                            .memory()
                            .read8(GuestAddr::new(buffer_ptr.wrapping_add(index as u32)))?;
                    }
                    ret = match file.file.write(&buffer) {
                        Ok(write_len) => write_len as i32,
                        Err(_) => MR_FAILED,
                    };
                }

                cpu.regs_mut().set_reg(0, ret as u32);
                return_to_lr(cpu);
            }
            DsmHostFn::Seek => {
                let fd = cpu.regs().reg(0) as i32;
                let pos = cpu.regs().reg(1) as i32;
                let method = cpu.regs().reg(2);
                let mut ret = MR_FAILED;

                if let Some(file) = self.files.get_mut(&fd) {
                    let seek_from = match method {
                        MR_SEEK_SET => Some(SeekFrom::Start(pos.max(0) as u64)),
                        MR_SEEK_CUR => Some(SeekFrom::Current(pos as i64)),
                        MR_SEEK_END => Some(SeekFrom::End(pos as i64)),
                        _ => None,
                    };

                    if let Some(from) = seek_from {
                        ret = match file.file.seek(from) {
                            Ok(new_pos) => i32::try_from(new_pos).unwrap_or(MR_FAILED),
                            Err(_) => MR_FAILED,
                        };
                    }
                }

                cpu.regs_mut().set_reg(0, ret as u32);
                return_to_lr(cpu);
            }
            DsmHostFn::Info => {
                let name_addr = cpu.regs().reg(0);
                let name = read_guest_c_string(cpu, name_addr, 1024)?;
                let path = self.resolve_guest_path(&name);
                let ret = match fs::metadata(path) {
                    Ok(meta) if meta.is_file() => MR_IS_FILE,
                    Ok(meta) if meta.is_dir() => MR_IS_DIR,
                    Ok(_) => MR_IS_INVALID,
                    Err(_) => MR_IS_INVALID,
                };
                cpu.regs_mut().set_reg(0, ret as u32);
                return_to_lr(cpu);
            }
            DsmHostFn::Remove => {
                let name_addr = cpu.regs().reg(0);
                let name = read_guest_c_string(cpu, name_addr, 1024)?;
                let path = self.resolve_guest_path(&name);
                let ret = if fs::remove_file(path).is_ok() {
                    MR_SUCCESS
                } else {
                    MR_FAILED
                };
                cpu.regs_mut().set_reg(0, ret as u32);
                return_to_lr(cpu);
            }
            DsmHostFn::Rename => {
                let old_addr = cpu.regs().reg(0);
                let new_addr = cpu.regs().reg(1);
                let old_name = read_guest_c_string(cpu, old_addr, 1024)?;
                let new_name = read_guest_c_string(cpu, new_addr, 1024)?;
                let old_path = self.resolve_guest_path(&old_name);
                let new_path = self.resolve_guest_path(&new_name);
                let ret = if fs::rename(old_path, new_path).is_ok() {
                    MR_SUCCESS
                } else {
                    MR_FAILED
                };
                cpu.regs_mut().set_reg(0, ret as u32);
                return_to_lr(cpu);
            }
            DsmHostFn::MkDir => {
                let name_addr = cpu.regs().reg(0);
                let name = read_guest_c_string(cpu, name_addr, 1024)?;
                let path = self.resolve_guest_path(&name);
                let ret = if fs::create_dir_all(path).is_ok() {
                    MR_SUCCESS
                } else {
                    MR_FAILED
                };
                cpu.regs_mut().set_reg(0, ret as u32);
                return_to_lr(cpu);
            }
            DsmHostFn::RmDir => {
                let name_addr = cpu.regs().reg(0);
                let name = read_guest_c_string(cpu, name_addr, 1024)?;
                let path = self.resolve_guest_path(&name);
                let ret = if fs::remove_dir(path).is_ok() {
                    MR_SUCCESS
                } else {
                    MR_FAILED
                };
                cpu.regs_mut().set_reg(0, ret as u32);
                return_to_lr(cpu);
            }
            DsmHostFn::OpenDir => {
                let name_addr = cpu.regs().reg(0);
                let name = read_guest_c_string(cpu, name_addr, 1024)?;
                let path = self.resolve_guest_path(&name);
                let ret = match fs::read_dir(path) {
                    Ok(entries) => {
                        let list = entries
                            .filter_map(|entry| entry.ok())
                            .filter_map(|entry| entry.file_name().into_string().ok())
                            .collect::<Vec<_>>();
                        let handle = self.next_dir_handle;
                        self.next_dir_handle = self.next_dir_handle.saturating_add(1);
                        self.dirs.insert(
                            handle,
                            HostDir {
                                entries: list,
                                cursor: 0,
                                scratch_ptr: 0,
                            },
                        );
                        handle
                    }
                    Err(_) => MR_FAILED,
                };
                cpu.regs_mut().set_reg(0, ret as u32);
                return_to_lr(cpu);
            }
            DsmHostFn::ReadDir => {
                let handle = cpu.regs().reg(0) as i32;
                let mut ret = 0u32;
                let mut entry_name: Option<String> = None;
                let mut scratch_ptr = 0u32;
                let mut need_alloc = false;

                if let Some(dir) = self.dirs.get_mut(&handle) {
                    if dir.cursor < dir.entries.len() {
                        entry_name = Some(dir.entries[dir.cursor].clone());
                        dir.cursor += 1;
                        if dir.scratch_ptr == 0 {
                            need_alloc = true;
                        } else {
                            scratch_ptr = dir.scratch_ptr;
                        }
                    }
                }

                if need_alloc {
                    let allocated = self.alloc(260).unwrap_or(0);
                    if let Some(dir) = self.dirs.get_mut(&handle) {
                        if dir.scratch_ptr == 0 {
                            dir.scratch_ptr = allocated;
                        }
                        scratch_ptr = dir.scratch_ptr;
                    }
                }

                if let Some(name) = entry_name {
                    if scratch_ptr != 0 {
                        write_guest_c_string(cpu, scratch_ptr, &name)?;
                        ret = scratch_ptr;
                    }
                }

                cpu.regs_mut().set_reg(0, ret);
                return_to_lr(cpu);
            }
            DsmHostFn::CloseDir => {
                let handle = cpu.regs().reg(0) as i32;
                let ret = if self.dirs.remove(&handle).is_some() {
                    MR_SUCCESS
                } else {
                    MR_FAILED
                };
                cpu.regs_mut().set_reg(0, ret as u32);
                return_to_lr(cpu);
            }
            DsmHostFn::GetLen => {
                let name_addr = cpu.regs().reg(0);
                let name = read_guest_c_string(cpu, name_addr, 1024)?;
                let path = self.resolve_guest_path(&name);
                let ret = match fs::metadata(path) {
                    Ok(meta) if meta.is_file() => i32::try_from(meta.len()).unwrap_or(MR_FAILED),
                    _ => MR_FAILED,
                };
                cpu.regs_mut().set_reg(0, ret as u32);
                return_to_lr(cpu);
            }
            DsmHostFn::DrawBitmap => {
                let bmp_ptr = cpu.regs().reg(0);
                let x = cpu.regs().reg(1) as i32;
                let y = cpu.regs().reg(2) as i32;
                let w = cpu.regs().reg(3) as usize;
                let h = cpu.memory().read32(GuestAddr::new(cpu.regs().sp()))? as usize;

                for j in 0..h {
                    for i in 0..w {
                        let xx = x + i as i32;
                        let yy = y + j as i32;
                        if xx < 0 || yy < 0 || xx >= SCREEN_WIDTH as i32 || yy >= SCREEN_HEIGHT as i32 {
                            continue;
                        }

                        let src_index = xx as u32 + yy as u32 * SCREEN_WIDTH as u32;
                        let pixel = cpu.memory().read16(GuestAddr::new(
                            bmp_ptr.wrapping_add(src_index.wrapping_mul(2)),
                        ))?;
                        let dst_index = yy as usize * SCREEN_WIDTH + xx as usize;
                        self.screen_buffer[dst_index] = pixel;
                    }
                }

                self.dirty_region = Some(HostScreenRegion {
                    x,
                    y,
                    w: w as u16,
                    h: h as u16,
                });
                cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
                return_to_lr(cpu);
            }
            DsmHostFn::UnsupportedFailed => {
                cpu.regs_mut().set_reg(0, MR_FAILED as u32);
                return_to_lr(cpu);
            }
        }
        Ok(())
    }

    fn resolve_guest_path(&self, raw: &str) -> PathBuf {
        let raw = raw.trim();
        if raw.is_empty() {
            return self.working_dir.clone();
        }

        let path = PathBuf::from(raw);
        if path.is_absolute() {
            path
        } else {
            self.working_dir.join(Path::new(raw))
        }
    }
}

fn return_to_lr<B>(cpu: &mut Cpu<B>) {
    let lr = cpu.regs().lr();
    let next_mode = if lr & 1 != 0 {
        ExecutionMode::Thumb
    } else {
        ExecutionMode::Arm
    };
    cpu.regs_mut().set_execution_mode(next_mode);
    cpu.regs_mut().set_pc(lr & !1);
}

fn align_up(value: u32, align: u32) -> u32 {
    if align <= 1 {
        return value;
    }
    let mask = align - 1;
    value.wrapping_add(mask) & !mask
}

fn read_guest_c_string<B: MemoryBus>(
    cpu: &Cpu<B>,
    addr: u32,
    max_len: usize,
) -> Result<String, MemoryAccessError> {
    if addr == 0 {
        return Ok(String::new());
    }
    let mut bytes = Vec::new();
    for index in 0..max_len {
        let value = cpu
            .memory()
            .read8(GuestAddr::new(addr.wrapping_add(index as u32)))?;
        if value == 0 {
            break;
        }
        bytes.push(value);
    }
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

fn write_guest_c_string<B: MemoryBus>(
    cpu: &mut Cpu<B>,
    addr: u32,
    value: &str,
) -> Result<(), MemoryAccessError> {
    for (index, byte) in value.as_bytes().iter().enumerate() {
        cpu.memory_mut()
            .write8(GuestAddr::new(addr.wrapping_add(index as u32)), *byte)?;
    }
    cpu.memory_mut()
        .write8(GuestAddr::new(addr.wrapping_add(value.len() as u32)), 0)?;
    Ok(())
}



fn current_local_datetime() -> HostDateTime {
    #[cfg(windows)]
    {
        #[repr(C)]
        struct SystemTimeWin {
            year: u16,
            month: u16,
            day_of_week: u16,
            day: u16,
            hour: u16,
            minute: u16,
            second: u16,
            milliseconds: u16,
        }

        unsafe extern "system" {
            fn GetLocalTime(system_time: *mut SystemTimeWin);
        }

        let mut system_time = SystemTimeWin {
            year: 0,
            month: 0,
            day_of_week: 0,
            day: 0,
            hour: 0,
            minute: 0,
            second: 0,
            milliseconds: 0,
        };
        unsafe {
            GetLocalTime(&mut system_time);
        }
        return HostDateTime {
            year: system_time.year,
            month: system_time.month as u8,
            day: system_time.day as u8,
            hour: system_time.hour as u8,
            minute: system_time.minute as u8,
            second: system_time.second as u8,
        };
    }

    #[cfg(not(windows))]
    {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let total_seconds = now.as_secs();
        let second = (total_seconds % 60) as u8;
        let minute = ((total_seconds / 60) % 60) as u8;
        let hour = ((total_seconds / 3600) % 24) as u8;
        let days = (total_seconds / 86_400) as i64;
        let (year, month, day) = civil_from_days(days);
        HostDateTime {
            year,
            month,
            day,
            hour,
            minute,
            second,
        }
    }
}

#[cfg(not(windows))]
fn civil_from_days(days_since_unix_epoch: i64) -> (u16, u8, u8) {
    let z = days_since_unix_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = year + if month <= 2 { 1 } else { 0 };
    (year as u16, month as u8, day as u8)
}












