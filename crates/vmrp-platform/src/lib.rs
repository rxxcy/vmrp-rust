use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use vmrp_core::{GuestAddr, DEFAULT_LAYOUT};
use vmrp_cpu::{Cpu, ExecutionMode, MemoryAccessError, MemoryBus};

pub const MR_MALLOC_OFFSET: u32 = 0x0;
pub const MR_FREE_OFFSET: u32 = 0x04;
pub const MR_REALLOC_OFFSET: u32 = 0x08;
pub const MEMCPY_OFFSET: u32 = 0x0C;
const MEMMOVE_OFFSET: u32 = 0x10;
const STRCPY_OFFSET: u32 = 0x14;
const STRNCPY_OFFSET: u32 = 0x18;
const STRCAT_OFFSET: u32 = 0x1C;
const STRNCAT_OFFSET: u32 = 0x20;
const MEMCMP_OFFSET: u32 = 0x24;
const STRCMP_OFFSET: u32 = 0x28;
const STRNCMP_OFFSET: u32 = 0x2C;
const STRCOLL_OFFSET: u32 = 0x30;
const MEMCHR_OFFSET: u32 = 0x34;
pub const MEMSET_OFFSET: u32 = 0x38;
const STRLEN_OFFSET: u32 = 0x3C;
const STRSTR_OFFSET: u32 = 0x40;
const SPRINTF_OFFSET: u32 = 0x44;
const ATOI_OFFSET: u32 = 0x48;
const STRTOUL_OFFSET: u32 = 0x4C;
const MR_PRINTF_OFFSET: u32 = 0x68;
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
const MR_GET_SCREEN_INFO_OFFSET: u32 = 0x140;
const MR_TEST_COM_OFFSET: u32 = 0x208;
const MR_TEST_COM1_OFFSET: u32 = 0x20C;
const MR_READ_FILE_OFFSET: u32 = 0x1F4;
const MR_IS_FILE: i32 = 1;
const MR_IS_DIR: i32 = 2;
const MR_IS_INVALID: i32 = 8;
const DSM_MEM_GET_SIZE: u32 = 4 * 1024 * 1024;
const SCREEN_WIDTH: usize = 240;
const SCREEN_HEIGHT: usize = 320;
const DEFAULT_MR_TABLE_ADDR: GuestAddr = GuestAddr::new(0x180000);
const MR_GET_SCREEN_INFO_ADDR: GuestAddr = GuestAddr::new(0x181340);
const MR_TEST_COM_ADDR: GuestAddr = GuestAddr::new(0x181380);
const MR_TEST_COM1_ADDR: GuestAddr = GuestAddr::new(0x1813C0);
const MR_READ_FILE_ADDR: GuestAddr = GuestAddr::new(0x181400);
const MEMMOVE_ADDR: GuestAddr = GuestAddr::new(0x181440);
const STRCPY_ADDR: GuestAddr = GuestAddr::new(0x181450);
const STRNCPY_ADDR: GuestAddr = GuestAddr::new(0x181460);
const STRCAT_ADDR: GuestAddr = GuestAddr::new(0x181470);
const STRNCAT_ADDR: GuestAddr = GuestAddr::new(0x181480);
const MEMCMP_ADDR: GuestAddr = GuestAddr::new(0x181490);
const STRCMP_ADDR: GuestAddr = GuestAddr::new(0x1814A0);
const STRNCMP_ADDR: GuestAddr = GuestAddr::new(0x1814B0);
const STRCOLL_ADDR: GuestAddr = GuestAddr::new(0x1814C0);
const MEMCHR_ADDR: GuestAddr = GuestAddr::new(0x1814D0);
const STRLEN_ADDR: GuestAddr = GuestAddr::new(0x1814E0);
const STRSTR_ADDR: GuestAddr = GuestAddr::new(0x1814F0);
const SPRINTF_ADDR: GuestAddr = GuestAddr::new(0x181500);
const ATOI_ADDR: GuestAddr = GuestAddr::new(0x181510);
const STRTOUL_ADDR: GuestAddr = GuestAddr::new(0x181520);
const MR_PRINTF_ADDR: GuestAddr = GuestAddr::new(0x181530);

pub const SEND_APP_EVENT_ADDR: GuestAddr = GuestAddr::new(0x181540);
const MR_SCREEN_BUF_OFFSET: u32 = 0x16C;
const MR_SCREEN_W_OFFSET: u32 = 0x170;
const MR_SCREEN_H_OFFSET: u32 = 0x174;
const MR_SCREEN_BIT_OFFSET: u32 = 0x178;
const MR_BITMAP_OFFSET: u32 = 0x17C;
const MR_TILE_OFFSET: u32 = 0x180;
const MR_MAP_OFFSET: u32 = 0x184;
const MR_SOUND_OFFSET: u32 = 0x188;
const MR_SPRITE_OFFSET: u32 = 0x18C;
const PACK_FILENAME_OFFSET: u32 = 0x190;
const START_FILENAME_OFFSET: u32 = 0x194;
const OLD_PACK_FILENAME_OFFSET: u32 = 0x198;
const OLD_START_FILENAME_OFFSET: u32 = 0x19C;
const MR_RAM_FILE_OFFSET: u32 = 0x1A0;
const MR_RAM_FILE_LEN_OFFSET: u32 = 0x1A4;
const MR_SOUND_ON_OFFSET: u32 = 0x1A8;
const MR_SHAKE_ON_OFFSET: u32 = 0x1AC;
const START_FILEPARAMETER_OFFSET: u32 = 0x228;
const MR_ENTRY_OFFSET: u32 = 0x240;
const LG_MEM_BASE_OFFSET: u32 = 0x1B0;
const LG_MEM_LEN_OFFSET: u32 = 0x1B4;
const LG_MEM_END_OFFSET: u32 = 0x1B8;
const LG_MEM_LEFT_OFFSET: u32 = 0x1BC;
const LG_MEM_MIN_OFFSET: u32 = 0x21C;
const LG_MEM_TOP_OFFSET: u32 = 0x220;
const MR_C_INTERNAL_TABLE_OFFSET: u32 = 0x5C;
const MR_C_PORT_TABLE_OFFSET: u32 = 0x60;
const MR_TABLE_MEMORY_CELLS_OFFSET: u32 = 0x300;
const MR_TABLE_INTERNAL_TABLE_OFFSET: u32 = 0x340;
const MR_TABLE_PORT_TABLE_OFFSET: u32 = 0x480;
const MR_TABLE_M0_FILES_OFFSET: u32 = 0x4C0;
const MR_TABLE_INTERNAL_DATA_OFFSET: u32 = 0x5A0;
const MR_TABLE_LEGACY_RUNTIME_OFFSET: u32 = 0x600;
const LEGACY_SCREEN_BUF_PTR_CELL_OFFSET: u32 = 0x00;
const LEGACY_SCREEN_W_CELL_OFFSET: u32 = 0x04;
const LEGACY_SCREEN_H_CELL_OFFSET: u32 = 0x08;
const LEGACY_SCREEN_BIT_CELL_OFFSET: u32 = 0x0C;
const LEGACY_RAM_FILE_PTR_CELL_OFFSET: u32 = 0x10;
const LEGACY_RAM_FILE_LEN_CELL_OFFSET: u32 = 0x14;
const LEGACY_SOUND_ON_CELL_OFFSET: u32 = 0x18;
const LEGACY_SHAKE_ON_CELL_OFFSET: u32 = 0x1C;
const LEGACY_PACK_FILENAME_BUF_OFFSET: u32 = 0x40;
const LEGACY_START_FILENAME_BUF_OFFSET: u32 = 0xC0;
const LEGACY_OLD_PACK_FILENAME_BUF_OFFSET: u32 = 0x140;
const LEGACY_OLD_START_FILENAME_BUF_OFFSET: u32 = 0x1C0;
const LEGACY_START_FILEPARAMETER_BUF_OFFSET: u32 = 0x240;
const LEGACY_ENTRY_BUF_OFFSET: u32 = 0x2C0;
const LEGACY_BITMAP_BUF_OFFSET: u32 = 0x340;
const LEGACY_TILE_BUF_OFFSET: u32 = 0x530;
const LEGACY_MAP_BUF_OFFSET: u32 = 0x570;
const LEGACY_SOUND_BUF_OFFSET: u32 = 0x580;
const LEGACY_SPRITE_BUF_OFFSET: u32 = 0x5C0;
const LEGACY_FILENAME_BUFFER_LEN: usize = 128;
const LEGACY_BITMAP_STRUCT_LEN: usize = 16;
const LEGACY_BITMAP_COUNT: usize = 31;
const LEGACY_TILE_STRUCT_LEN: usize = 20;
const LEGACY_TILE_COUNT: usize = 3;
const LEGACY_MAP_PTR_COUNT: usize = 3;
const LEGACY_SOUND_STRUCT_LEN: usize = 12;
const LEGACY_SOUND_COUNT: usize = 5;
const LEGACY_SPRITE_STRUCT_LEN: usize = 4;
const LEGACY_SPRITE_COUNT: usize = 10;
const DEFAULT_START_FILE_NAME: &str = "start.mr";
const MR_FLAGS_BI: u32 = 1;
const MR_FLAGS_AI: u32 = 1 << 1;
const MR_FLAGS_RI: u32 = 1 << 2;
const MR_FLAGS_EI: u32 = 1 << 3;
const HELPER_APP_INFO_PTR_OFFSET: u32 = 0x1C0;
const LG_MEM_BASE_CELL_OFFSET: u32 = 0x00;
const LG_MEM_LEN_CELL_OFFSET: u32 = 0x04;
const LG_MEM_END_CELL_OFFSET: u32 = 0x08;
const LG_MEM_LEFT_CELL_OFFSET: u32 = 0x0C;
const LG_MEM_MIN_CELL_OFFSET: u32 = 0x10;
const LG_MEM_TOP_CELL_OFFSET: u32 = 0x14;
const VM_STATE_CELL_OFFSET: u32 = 0x00;
const MR_STATE_CELL_OFFSET: u32 = 0x04;
const BI_CELL_OFFSET: u32 = 0x08;
const MR_TIMER_P_CELL_OFFSET: u32 = 0x0C;
const MR_TIMER_STATE_CELL_OFFSET: u32 = 0x10;
const MR_TIMER_RUN_WITHOUT_PAUSE_CELL_OFFSET: u32 = 0x14;
const MR_GZ_IN_BUF_CELL_OFFSET: u32 = 0x18;
const MR_GZ_OUT_BUF_CELL_OFFSET: u32 = 0x1C;
const LG_GZINPTR_CELL_OFFSET: u32 = 0x20;
const LG_GZOUTCNT_CELL_OFFSET: u32 = 0x24;
const MR_SMS_CFG_NEED_SAVE_CELL_OFFSET: u32 = 0x28;
const MR_C_INTERNAL_TABLE_LEN: u32 = 78;
const MR_C_PORT_TABLE_LEN: u32 = 4;
const MR_M0_FILES_LEN: u32 = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MemoryManagerSnapshot {
    base: u32,
    len: u32,
    end: u32,
    left: u32,
    min: u32,
    top: u32,
}

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

        // Default unresolved entries to a safe returning stub so legacy guests
        // can still indirect-call optional hooks. Known MAP_DATA slots are
        // rewritten below to guest-visible cell pointers.
        for offset in (0..=0x240u32).step_by(4) {
            let slot = GuestAddr::new(self.mr_table_addr.get().wrapping_add(offset));
            memory.write32(slot, self.mr_free_addr.get())?;
        }

        let mr_malloc_slot =
            GuestAddr::new(self.mr_table_addr.get().wrapping_add(MR_MALLOC_OFFSET));
        memory.write32(mr_malloc_slot, self.mr_malloc_addr.get())?;

        let mr_free_slot = GuestAddr::new(self.mr_table_addr.get().wrapping_add(MR_FREE_OFFSET));
        memory.write32(mr_free_slot, self.mr_free_addr.get())?;

        let mr_realloc_slot =
            GuestAddr::new(self.mr_table_addr.get().wrapping_add(MR_REALLOC_OFFSET));
        memory.write32(mr_realloc_slot, self.mr_realloc_addr.get())?;

        let memcpy_slot = GuestAddr::new(self.mr_table_addr.get().wrapping_add(MEMCPY_OFFSET));
        memory.write32(memcpy_slot, self.memcpy_addr.get())?;
        for (offset, addr) in [
            (MEMMOVE_OFFSET, MEMMOVE_ADDR),
            (STRCPY_OFFSET, STRCPY_ADDR),
            (STRNCPY_OFFSET, STRNCPY_ADDR),
            (STRCAT_OFFSET, STRCAT_ADDR),
            (STRNCAT_OFFSET, STRNCAT_ADDR),
            (MEMCMP_OFFSET, MEMCMP_ADDR),
            (STRCMP_OFFSET, STRCMP_ADDR),
            (STRNCMP_OFFSET, STRNCMP_ADDR),
            (STRCOLL_OFFSET, STRCOLL_ADDR),
            (MEMCHR_OFFSET, MEMCHR_ADDR),
            (STRLEN_OFFSET, STRLEN_ADDR),
            (STRSTR_OFFSET, STRSTR_ADDR),
            (SPRINTF_OFFSET, SPRINTF_ADDR),
            (ATOI_OFFSET, ATOI_ADDR),
            (STRTOUL_OFFSET, STRTOUL_ADDR),
            (MR_PRINTF_OFFSET, MR_PRINTF_ADDR),
        ] {
            let slot = GuestAddr::new(self.mr_table_addr.get().wrapping_add(offset));
            memory.write32(slot, addr.get())?;
        }

        let memset_slot = GuestAddr::new(self.mr_table_addr.get().wrapping_add(MEMSET_OFFSET));
        memory.write32(memset_slot, self.memset_addr.get())?;

        let screen_info_slot = GuestAddr::new(
            self.mr_table_addr
                .get()
                .wrapping_add(MR_GET_SCREEN_INFO_OFFSET),
        );
        memory.write32(screen_info_slot, MR_GET_SCREEN_INFO_ADDR.get())?;

        let test_com_slot =
            GuestAddr::new(self.mr_table_addr.get().wrapping_add(MR_TEST_COM_OFFSET));
        memory.write32(test_com_slot, MR_TEST_COM_ADDR.get())?;
        let test_com1_slot =
            GuestAddr::new(self.mr_table_addr.get().wrapping_add(MR_TEST_COM1_OFFSET));
        memory.write32(test_com1_slot, MR_TEST_COM1_ADDR.get())?;
        let read_file_slot =
            GuestAddr::new(self.mr_table_addr.get().wrapping_add(MR_READ_FILE_OFFSET));
        memory.write32(read_file_slot, MR_READ_FILE_ADDR.get())?;

        let c_function_new_slot = GuestAddr::new(
            self.mr_table_addr
                .get()
                .wrapping_add(MR_C_FUNCTION_NEW_OFFSET),
        );
        memory.write32(c_function_new_slot, self.mr_c_function_new_addr.get())?;

        seed_memory_manager_cells(memory, self.mr_table_addr, MemoryManagerSnapshot::initial())?;
        seed_internal_runtime_tables(memory, self.mr_table_addr, self.mr_free_addr)?;
        seed_legacy_runtime_data(memory, self.mr_table_addr)?;

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
        for addr in [
            MEMMOVE_ADDR,
            STRCPY_ADDR,
            STRNCPY_ADDR,
            STRCAT_ADDR,
            STRNCAT_ADDR,
            MEMCMP_ADDR,
            STRCMP_ADDR,
            STRNCMP_ADDR,
            STRCOLL_ADDR,
            MEMCHR_ADDR,
            STRLEN_ADDR,
            STRSTR_ADDR,
            SPRINTF_ADDR,
            ATOI_ADDR,
            STRTOUL_ADDR,
            MR_PRINTF_ADDR,
            SEND_APP_EVENT_ADDR,
        ] {
            memory.write32(addr, MOV_R0_IMM0)?;
            memory.write32(GuestAddr::new(addr.get().wrapping_add(4)), BX_LR)?;
        }
        memory.write32(MR_GET_SCREEN_INFO_ADDR, MOV_R0_IMM0)?;
        memory.write32(
            GuestAddr::new(MR_GET_SCREEN_INFO_ADDR.get().wrapping_add(4)),
            BX_LR,
        )?;
        memory.write32(MR_TEST_COM_ADDR, MOV_R0_IMM0)?;
        memory.write32(
            GuestAddr::new(MR_TEST_COM_ADDR.get().wrapping_add(4)),
            BX_LR,
        )?;
        memory.write32(MR_TEST_COM1_ADDR, MOV_R0_IMM0)?;
        memory.write32(
            GuestAddr::new(MR_TEST_COM1_ADDR.get().wrapping_add(4)),
            BX_LR,
        )?;
        memory.write32(MR_READ_FILE_ADDR, MOV_R0_IMM0)?;
        memory.write32(
            GuestAddr::new(MR_READ_FILE_ADDR.get().wrapping_add(4)),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FreeBlock {
    start: u32,
    len: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HostAllocation {
    raw_addr: u32,
    requested_len: u32,
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
    mr_table_addr: GuestAddr,
    ext_helper_addr: Option<GuestAddr>,
    next_callback: u32,
    heap_blocks: Vec<FreeBlock>,
    ext_allocations: BTreeMap<u32, HostAllocation>,
    dsm_callbacks: BTreeMap<u32, DsmHostFn>,
    working_dir: PathBuf,
    verbose: bool,
    last_log_message: Option<String>,
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
    package_files: BTreeMap<String, Vec<u8>>,
    memory_manager_min_free: u32,
    memory_manager_peak_used: u32,
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
            mr_table_addr: DEFAULT_MR_TABLE_ADDR,
            ext_helper_addr: None,
            next_callback: alloc_base
                .get()
                .wrapping_add(alloc_size)
                .wrapping_add(0x1000),
            heap_blocks: vec![FreeBlock {
                start: DEFAULT_LAYOUT.memory_manager_address().get(),
                len: DEFAULT_LAYOUT.memory_manager_size(),
            }],
            ext_allocations: BTreeMap::new(),
            dsm_callbacks: BTreeMap::new(),
            working_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            verbose: false,
            last_log_message: None,
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
            package_files: BTreeMap::new(),
            memory_manager_min_free: memory_manager_total_len(),
            memory_manager_peak_used: 0,
        }
    }

    pub fn ext_helper_addr(&self) -> Option<GuestAddr> {
        self.ext_helper_addr
    }

    pub fn c_function_p_addr(&self) -> GuestAddr {
        self.mr_c_function_p_addr
    }

    pub fn set_mr_table_addr(&mut self, addr: GuestAddr) {
        self.mr_table_addr = addr;
    }

    pub fn reset_alloc<B: MemoryBus>(&mut self, memory: &mut B) -> Result<(), MemoryAccessError> {
        self.heap_blocks.clear();
        self.heap_blocks.push(FreeBlock {
            start: DEFAULT_LAYOUT.memory_manager_address().get(),
            len: DEFAULT_LAYOUT.memory_manager_size(),
        });
        self.ext_allocations.clear();
        self.memory_manager_min_free = memory_manager_total_len();
        self.memory_manager_peak_used = 0;
        self.sync_memory_manager_cells(memory)
    }

    pub fn set_working_dir<P: Into<PathBuf>>(&mut self, path: P) {
        self.working_dir = path.into();
    }

    pub fn set_verbose(&mut self, verbose: bool) {
        self.verbose = verbose;
    }

    pub fn alloc_guest_block<B: MemoryBus>(
        &mut self,
        memory: &mut B,
        len: u32,
    ) -> Result<Option<GuestAddr>, MemoryAccessError> {
        let ptr = self.alloc_ext(memory, len)?.map(GuestAddr::new);
        self.sync_memory_manager_cells(memory)?;
        Ok(ptr)
    }

    pub fn register_package_file(&mut self, name: impl Into<String>, bytes: Vec<u8>) {
        self.package_files.insert(name.into(), bytes);
    }

    pub fn take_last_log_message(&mut self) -> Option<String> {
        self.last_log_message.take()
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
            self.handle_mr_malloc(cpu)?;
            return Ok(true);
        }

        if pc == self.mr_free_addr.get() {
            self.handle_mr_free(cpu)?;
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

        if pc == MEMMOVE_ADDR.get() {
            self.handle_memmove(cpu)?;
            return Ok(true);
        }

        if pc == STRCPY_ADDR.get() {
            self.handle_strcpy(cpu)?;
            return Ok(true);
        }

        if pc == STRNCPY_ADDR.get() {
            self.handle_strncpy(cpu)?;
            return Ok(true);
        }

        if pc == STRCAT_ADDR.get() {
            self.handle_strcat(cpu)?;
            return Ok(true);
        }

        if pc == STRNCAT_ADDR.get() {
            self.handle_strncat(cpu)?;
            return Ok(true);
        }

        if pc == MEMCMP_ADDR.get() {
            self.handle_memcmp(cpu)?;
            return Ok(true);
        }

        if pc == STRCMP_ADDR.get() {
            self.handle_strcmp(cpu)?;
            return Ok(true);
        }

        if pc == STRNCMP_ADDR.get() {
            self.handle_strncmp(cpu)?;
            return Ok(true);
        }

        if pc == STRCOLL_ADDR.get() {
            self.handle_strcoll(cpu)?;
            return Ok(true);
        }

        if pc == MEMCHR_ADDR.get() {
            self.handle_memchr(cpu)?;
            return Ok(true);
        }

        if pc == self.memset_addr.get() {
            self.handle_memset(cpu)?;
            return Ok(true);
        }

        if pc == STRLEN_ADDR.get() {
            self.handle_strlen(cpu)?;
            return Ok(true);
        }

        if pc == STRSTR_ADDR.get() {
            self.handle_strstr(cpu)?;
            return Ok(true);
        }

        if pc == SPRINTF_ADDR.get() {
            self.handle_sprintf(cpu)?;
            return Ok(true);
        }

        if pc == ATOI_ADDR.get() {
            self.handle_atoi(cpu)?;
            return Ok(true);
        }

        if pc == STRTOUL_ADDR.get() {
            self.handle_strtoul(cpu)?;
            return Ok(true);
        }

        if pc == MR_PRINTF_ADDR.get() {
            self.handle_mr_printf(cpu)?;
            return Ok(true);
        }

        if pc == SEND_APP_EVENT_ADDR.get() {
            self.handle_send_app_event(cpu)?;
            return Ok(true);
        }

        if pc == MR_GET_SCREEN_INFO_ADDR.get() {
            self.handle_mr_get_screen_info(cpu)?;
            return Ok(true);
        }

        if pc == MR_TEST_COM_ADDR.get() {
            self.handle_mr_test_com(cpu)?;
            return Ok(true);
        }

        if pc == MR_TEST_COM1_ADDR.get() {
            self.handle_mr_test_com1(cpu)?;
            return Ok(true);
        }

        if pc == MR_READ_FILE_ADDR.get() {
            self.handle_mr_read_file(cpu)?;
            return Ok(true);
        }

        if let Some(callback) = self.dsm_callbacks.get(&pc).copied() {
            self.handle_dsm_callback(cpu, callback)?;
            return Ok(true);
        }

        Ok(false)
    }

    fn handle_send_app_event<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let code = cpu.regs().reg(2);
        match code {
            0 => {
                let delay = cpu.memory().read32(GuestAddr::new(cpu.regs().sp()))?;
                self.pending_timer_delay_ms = Some(delay);
                self.pending_timer_command = Some(HostTimerCommand::Start(delay));
            }
            1 => {
                self.pending_timer_delay_ms = None;
                self.pending_timer_command = Some(HostTimerCommand::Stop);
            }
            _ => {}
        }

        cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_mr_c_function_new<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let helper = cpu.regs().reg(0);
        let len = cpu.regs().reg(1).max(1);
        if helper != 0 {
            self.ext_helper_addr = Some(GuestAddr::new(helper));
        }

        let previous_context = self.mr_c_function_p_addr.get();
        if self.ext_allocations.contains_key(&previous_context) {
            self.free_ext(previous_context);
        }

        let Some(context_addr) = self.alloc_ext(cpu.memory_mut(), len)? else {
            cpu.regs_mut().set_reg(0, u32::MAX);
            return_to_lr(cpu);
            return Ok(());
        };
        for offset in 0..len {
            cpu.memory_mut()
                .write8(GuestAddr::new(context_addr.wrapping_add(offset)), 0)?;
        }

        self.mr_c_function_p_addr = GuestAddr::new(context_addr);
        cpu.memory_mut().write32(
            GuestAddr::new(DEFAULT_LAYOUT.code_address().get().wrapping_add(4)),
            context_addr,
        )?;

        cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
        self.sync_memory_manager_cells(cpu.memory_mut())?;
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_mr_malloc<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        if self.verbose {
            println!(
                "[host-stub] fn=mr_malloc lr=0x{:X} r0=0x{:X} r1=0x{:X} r2=0x{:X}",
                cpu.regs().lr(),
                cpu.regs().reg(0),
                cpu.regs().reg(1),
                cpu.regs().reg(2)
            );
        }
        let requested = cpu.regs().reg(0).max(1);
        let out = self.alloc_ext(cpu.memory_mut(), requested)?.unwrap_or(0);
        cpu.regs_mut().set_reg(0, out);
        self.sync_memory_manager_cells(cpu.memory_mut())?;
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_mr_free<B: MemoryBus>(&mut self, cpu: &mut Cpu<B>) -> Result<(), MemoryAccessError> {
        let ptr = cpu.regs().reg(0);
        self.free_ext(ptr);
        cpu.regs_mut().set_reg(0, 0);
        self.sync_memory_manager_cells(cpu.memory_mut())?;
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_mr_realloc<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let old_ptr = cpu.regs().reg(0);
        let new_len = cpu.regs().reg(1);

        let out = if old_ptr == 0 {
            if new_len == 0 {
                0
            } else {
                self.alloc_ext(cpu.memory_mut(), new_len)?.unwrap_or(0)
            }
        } else if new_len == 0 {
            self.free_ext(old_ptr);
            0
        } else if let Some(new_ptr) = self.alloc_ext(cpu.memory_mut(), new_len)? {
            let old_len = self.ext_requested_len(old_ptr, cpu.memory())?;
            let copy_len = old_len.min(new_len);
            for offset in 0..copy_len {
                let byte = cpu
                    .memory()
                    .read8(GuestAddr::new(old_ptr.wrapping_add(offset)))?;
                cpu.memory_mut()
                    .write8(GuestAddr::new(new_ptr.wrapping_add(offset)), byte)?;
            }
            self.free_ext(old_ptr);
            new_ptr
        } else {
            0
        };

        cpu.regs_mut().set_reg(0, out);
        self.sync_memory_manager_cells(cpu.memory_mut())?;
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_memcpy<B: MemoryBus>(&self, cpu: &mut Cpu<B>) -> Result<(), MemoryAccessError> {
        let dst = cpu.regs().reg(0);
        let src = cpu.regs().reg(1);
        let len = cpu.regs().reg(2);

        for offset in 0..len {
            let value = cpu
                .memory()
                .read8(GuestAddr::new(src.wrapping_add(offset)))?;
            cpu.memory_mut()
                .write8(GuestAddr::new(dst.wrapping_add(offset)), value)?;
        }

        cpu.regs_mut().set_reg(0, dst);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_memmove<B: MemoryBus>(&self, cpu: &mut Cpu<B>) -> Result<(), MemoryAccessError> {
        let dst = cpu.regs().reg(0);
        let src = cpu.regs().reg(1);
        let len = cpu.regs().reg(2);
        let mut bytes = Vec::with_capacity(len as usize);
        for offset in 0..len {
            bytes.push(
                cpu.memory()
                    .read8(GuestAddr::new(src.wrapping_add(offset)))?,
            );
        }
        for (index, byte) in bytes.into_iter().enumerate() {
            cpu.memory_mut()
                .write8(GuestAddr::new(dst.wrapping_add(index as u32)), byte)?;
        }
        cpu.regs_mut().set_reg(0, dst);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_strcpy<B: MemoryBus>(&self, cpu: &mut Cpu<B>) -> Result<(), MemoryAccessError> {
        let dst = cpu.regs().reg(0);
        let src = cpu.regs().reg(1);
        let value = read_guest_c_string(cpu, src, 4096)?;
        write_guest_c_string(cpu, dst, &value)?;
        cpu.regs_mut().set_reg(0, dst);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_strncpy<B: MemoryBus>(&self, cpu: &mut Cpu<B>) -> Result<(), MemoryAccessError> {
        let dst = cpu.regs().reg(0);
        let src = cpu.regs().reg(1);
        let len = cpu.regs().reg(2);
        let mut offset = 0u32;
        let mut terminated = false;
        while offset < len {
            let byte = if terminated {
                0
            } else {
                let value = cpu
                    .memory()
                    .read8(GuestAddr::new(src.wrapping_add(offset)))?;
                if value == 0 {
                    terminated = true;
                }
                value
            };
            cpu.memory_mut()
                .write8(GuestAddr::new(dst.wrapping_add(offset)), byte)?;
            offset = offset.wrapping_add(1);
        }
        cpu.regs_mut().set_reg(0, dst);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_strcat<B: MemoryBus>(&self, cpu: &mut Cpu<B>) -> Result<(), MemoryAccessError> {
        let dst = cpu.regs().reg(0);
        let src = cpu.regs().reg(1);
        let dst_len = guest_strlen(cpu.memory(), dst, 4096)?;
        let mut src_offset = 0u32;
        loop {
            let byte = cpu
                .memory()
                .read8(GuestAddr::new(src.wrapping_add(src_offset)))?;
            cpu.memory_mut().write8(
                GuestAddr::new(dst.wrapping_add(dst_len).wrapping_add(src_offset)),
                byte,
            )?;
            if byte == 0 {
                break;
            }
            src_offset = src_offset.wrapping_add(1);
        }
        cpu.regs_mut().set_reg(0, dst);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_strncat<B: MemoryBus>(&self, cpu: &mut Cpu<B>) -> Result<(), MemoryAccessError> {
        let dst = cpu.regs().reg(0);
        let src = cpu.regs().reg(1);
        let max_len = cpu.regs().reg(2);
        let dst_len = guest_strlen(cpu.memory(), dst, 4096)?;
        let mut copied = 0u32;
        while copied < max_len {
            let byte = cpu
                .memory()
                .read8(GuestAddr::new(src.wrapping_add(copied)))?;
            if byte == 0 {
                break;
            }
            cpu.memory_mut().write8(
                GuestAddr::new(dst.wrapping_add(dst_len).wrapping_add(copied)),
                byte,
            )?;
            copied = copied.wrapping_add(1);
        }
        cpu.memory_mut().write8(
            GuestAddr::new(dst.wrapping_add(dst_len).wrapping_add(copied)),
            0,
        )?;
        cpu.regs_mut().set_reg(0, dst);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_memcmp<B: MemoryBus>(&self, cpu: &mut Cpu<B>) -> Result<(), MemoryAccessError> {
        let left = cpu.regs().reg(0);
        let right = cpu.regs().reg(1);
        let len = cpu.regs().reg(2);
        let mut ret = 0i32;
        for offset in 0..len {
            let a = cpu
                .memory()
                .read8(GuestAddr::new(left.wrapping_add(offset)))?;
            let b = cpu
                .memory()
                .read8(GuestAddr::new(right.wrapping_add(offset)))?;
            if a != b {
                ret = a as i32 - b as i32;
                break;
            }
        }
        cpu.regs_mut().set_reg(0, ret as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_strcmp<B: MemoryBus>(&self, cpu: &mut Cpu<B>) -> Result<(), MemoryAccessError> {
        let ret =
            compare_guest_c_strings(cpu.memory(), cpu.regs().reg(0), cpu.regs().reg(1), None)?;
        cpu.regs_mut().set_reg(0, ret as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_strncmp<B: MemoryBus>(&self, cpu: &mut Cpu<B>) -> Result<(), MemoryAccessError> {
        let ret = compare_guest_c_strings(
            cpu.memory(),
            cpu.regs().reg(0),
            cpu.regs().reg(1),
            Some(cpu.regs().reg(2)),
        )?;
        cpu.regs_mut().set_reg(0, ret as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_strcoll<B: MemoryBus>(&self, cpu: &mut Cpu<B>) -> Result<(), MemoryAccessError> {
        self.handle_strcmp(cpu)
    }

    fn handle_memchr<B: MemoryBus>(&self, cpu: &mut Cpu<B>) -> Result<(), MemoryAccessError> {
        let ptr = cpu.regs().reg(0);
        let target = cpu.regs().reg(1) as u8;
        let len = cpu.regs().reg(2);
        let mut found = 0u32;
        for offset in 0..len {
            let byte = cpu
                .memory()
                .read8(GuestAddr::new(ptr.wrapping_add(offset)))?;
            if byte == target {
                found = ptr.wrapping_add(offset);
                break;
            }
        }
        cpu.regs_mut().set_reg(0, found);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_strlen<B: MemoryBus>(&self, cpu: &mut Cpu<B>) -> Result<(), MemoryAccessError> {
        let len = guest_strlen(cpu.memory(), cpu.regs().reg(0), 4096)?;
        cpu.regs_mut().set_reg(0, len);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_strstr<B: MemoryBus>(&self, cpu: &mut Cpu<B>) -> Result<(), MemoryAccessError> {
        let haystack_addr = cpu.regs().reg(0);
        let needle_addr = cpu.regs().reg(1);
        let haystack = read_guest_c_string(cpu, haystack_addr, 4096)?;
        let needle = read_guest_c_string(cpu, needle_addr, 4096)?;
        let ret = haystack
            .find(&needle)
            .map(|index| haystack_addr.wrapping_add(index as u32))
            .unwrap_or(0);
        cpu.regs_mut().set_reg(0, ret);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_sprintf<B: MemoryBus>(&self, cpu: &mut Cpu<B>) -> Result<(), MemoryAccessError> {
        let dst = cpu.regs().reg(0);
        let fmt = cpu.regs().reg(1);
        let rendered = format_guest_string(cpu, fmt, 2)?;
        write_guest_c_string(cpu, dst, &rendered)?;
        cpu.regs_mut().set_reg(0, rendered.len() as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_atoi<B: MemoryBus>(&self, cpu: &mut Cpu<B>) -> Result<(), MemoryAccessError> {
        let value = parse_guest_i32(&read_guest_c_string(cpu, cpu.regs().reg(0), 256)?);
        cpu.regs_mut().set_reg(0, value as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_strtoul<B: MemoryBus>(&self, cpu: &mut Cpu<B>) -> Result<(), MemoryAccessError> {
        let nptr = cpu.regs().reg(0);
        let endptr = cpu.regs().reg(1);
        let base = cpu.regs().reg(2);
        let raw = read_guest_c_string(cpu, nptr, 256)?;
        let (value, consumed) = parse_guest_strtoul(&raw, base);
        if endptr != 0 {
            cpu.memory_mut()
                .write32(GuestAddr::new(endptr), nptr.wrapping_add(consumed as u32))?;
        }
        cpu.regs_mut().set_reg(0, value);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_mr_printf<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let fmt = cpu.regs().reg(0);
        let rendered = format_guest_string(cpu, fmt, 1)?;
        self.last_log_message = Some(rendered.clone());
        if self.verbose {
            println!("[guest-printf] {rendered}");
        }
        cpu.regs_mut().set_reg(0, rendered.len() as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_memset<B: MemoryBus>(&self, cpu: &mut Cpu<B>) -> Result<(), MemoryAccessError> {
        if self.verbose {
            println!(
                "[host-stub] fn=memset lr=0x{:X} r0=0x{:X} r1=0x{:X} r2=0x{:X}",
                cpu.regs().lr(),
                cpu.regs().reg(0),
                cpu.regs().reg(1),
                cpu.regs().reg(2)
            );
        }
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

    fn handle_mr_test_com<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let input0 = cpu.regs().reg(1) as i32;
        let input1 = cpu.regs().reg(2);
        println!("[host-testcom] code={} input1=0x{:X}", input0, input1);
        let ret = match input0 {
            1 => self
                .uptime_epoch
                .elapsed()
                .as_millis()
                .min(u128::from(u32::MAX)) as u32,
            7 | 8 => input1,
            9 => {
                let rw_base = cpu
                    .memory()
                    .read32(GuestAddr::new(self.mr_c_function_p_addr.get()))?;
                if rw_base != 0 {
                    let app_info_ptr = cpu.memory().read32(GuestAddr::new(
                        rw_base.wrapping_add(HELPER_APP_INFO_PTR_OFFSET),
                    ))?;
                    if app_info_ptr != 0 {
                        cpu.memory_mut()
                            .write32(GuestAddr::new(app_info_ptr.wrapping_add(12)), input1)?;
                    }
                }
                0
            }
            100 => self.memory_manager_min_free,
            101 => self.memory_manager_peak_used,
            102 => self.current_memory_manager_snapshot().left,
            300 => {
                cpu.memory_mut().write32(
                    legacy_runtime_addr(self.mr_table_addr, LEGACY_SOUND_ON_CELL_OFFSET),
                    input1,
                )?;
                0
            }
            301 => {
                cpu.memory_mut().write32(
                    legacy_runtime_addr(self.mr_table_addr, LEGACY_SHAKE_ON_CELL_OFFSET),
                    input1,
                )?;
                0
            }
            302 => {
                let cell = internal_data_cell_addr(self.mr_table_addr, BI_CELL_OFFSET);
                let current = cpu.memory().read32(cell)?;
                cpu.memory_mut().write32(cell, current | MR_FLAGS_RI)?;
                0
            }
            303 => {
                let cell = internal_data_cell_addr(self.mr_table_addr, BI_CELL_OFFSET);
                let current = cpu.memory().read32(cell)?;
                cpu.memory_mut().write32(cell, current & !MR_FLAGS_RI)?;
                0
            }
            304 => {
                let cell = internal_data_cell_addr(self.mr_table_addr, BI_CELL_OFFSET);
                let current = cpu.memory().read32(cell)?;
                cpu.memory_mut().write32(cell, current | MR_FLAGS_EI)?;
                0
            }
            305 => {
                let cell = internal_data_cell_addr(self.mr_table_addr, BI_CELL_OFFSET);
                let current = cpu.memory().read32(cell)?;
                cpu.memory_mut().write32(cell, current & !MR_FLAGS_EI)?;
                0
            }
            3629 if input1 == 2913 => {
                let cell = internal_data_cell_addr(self.mr_table_addr, BI_CELL_OFFSET);
                let current = cpu.memory().read32(cell)?;
                cpu.memory_mut().write32(cell, current | MR_FLAGS_BI)?;
                0
            }
            3921 if input1 == 98352 => {
                let cell = internal_data_cell_addr(self.mr_table_addr, BI_CELL_OFFSET);
                let current = cpu.memory().read32(cell)?;
                cpu.memory_mut().write32(cell, current | MR_FLAGS_AI)?;
                0
            }
            3251 if input1 == 648826 => {
                let cell = internal_data_cell_addr(self.mr_table_addr, BI_CELL_OFFSET);
                let current = cpu.memory().read32(cell)?;
                cpu.memory_mut().write32(cell, current & !MR_FLAGS_AI)?;
                0
            }
            _ => 0,
        };
        cpu.regs_mut().set_reg(0, ret);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_mr_test_com1<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let input0 = cpu.regs().reg(1) as i32;
        println!(
            "[host-testcom1] code={} input1=0x{:X} len=0x{:X}",
            input0,
            cpu.regs().reg(2),
            cpu.regs().reg(3)
        );
        let input1 = cpu.regs().reg(2);
        let len = cpu.regs().reg(3);

        match input0 {
            2 => {
                let ram_file_cell =
                    legacy_runtime_addr(self.mr_table_addr, LEGACY_RAM_FILE_PTR_CELL_OFFSET);
                let ram_file_len_cell =
                    legacy_runtime_addr(self.mr_table_addr, LEGACY_RAM_FILE_LEN_CELL_OFFSET);
                cpu.memory_mut().write32(ram_file_cell, input1)?;
                cpu.memory_mut().write32(ram_file_len_cell, len)?;
            }
            3 => {
                let value = read_guest_c_string(cpu, input1, LEGACY_FILENAME_BUFFER_LEN)?;
                write_guest_c_string_to_memory(
                    cpu.memory_mut(),
                    legacy_runtime_addr(self.mr_table_addr, LEGACY_OLD_PACK_FILENAME_BUF_OFFSET),
                    &value,
                    LEGACY_FILENAME_BUFFER_LEN,
                )?;
                write_guest_c_string_to_memory(
                    cpu.memory_mut(),
                    legacy_runtime_addr(self.mr_table_addr, LEGACY_OLD_START_FILENAME_BUF_OFFSET),
                    DEFAULT_START_FILE_NAME,
                    LEGACY_FILENAME_BUFFER_LEN,
                )?;
            }
            4 => {
                let value = read_guest_c_string(cpu, input1, LEGACY_FILENAME_BUFFER_LEN)?;
                write_guest_c_string_to_memory(
                    cpu.memory_mut(),
                    legacy_runtime_addr(self.mr_table_addr, LEGACY_START_FILEPARAMETER_BUF_OFFSET),
                    &value,
                    LEGACY_FILENAME_BUFFER_LEN,
                )?;
            }
            5 | 6 | 9 => {}
            _ => {}
        }

        cpu.regs_mut().set_reg(0, 0);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_mr_read_file<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let name_addr = cpu.regs().reg(0);
        let filelen_ptr = cpu.regs().reg(1);
        let name = read_guest_c_string(cpu, name_addr, 1024)?;
        let key = name.replace('\\', "/");
        let bytes = if let Some(bytes) = self.package_files.get(&key) {
            Some(bytes.clone())
        } else if let Some(bytes) = self.package_files.get(name.as_str()) {
            Some(bytes.clone())
        } else {
            let path = self.resolve_guest_path(&name);
            fs::read(path).ok()
        };

        let ptr = if let Some(bytes) = bytes {
            if let Some(ptr) = self.alloc_ext(cpu.memory_mut(), bytes.len() as u32)? {
                for (index, byte) in bytes.iter().enumerate() {
                    cpu.memory_mut()
                        .write8(GuestAddr::new(ptr.wrapping_add(index as u32)), *byte)?;
                }
                if filelen_ptr != 0 {
                    cpu.memory_mut()
                        .write32(GuestAddr::new(filelen_ptr), bytes.len() as u32)?;
                }
                ptr
            } else {
                0
            }
        } else {
            0
        };
        cpu.regs_mut().set_reg(0, ptr);
        self.sync_memory_manager_cells(cpu.memory_mut())?;
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_mr_get_screen_info<B: MemoryBus>(
        &self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let out = cpu.regs().reg(0);
        if out != 0 {
            cpu.memory_mut()
                .write32(GuestAddr::new(out), SCREEN_WIDTH as u32)?;
            cpu.memory_mut()
                .write32(GuestAddr::new(out.wrapping_add(4)), SCREEN_HEIGHT as u32)?;
            cpu.memory_mut()
                .write32(GuestAddr::new(out.wrapping_add(8)), 16)?;
        }
        cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn alloc_raw(&mut self, size: u32) -> Option<u32> {
        let aligned = align_up(size.max(1), 8);
        let index = self
            .heap_blocks
            .iter()
            .position(|block| block.len >= aligned)?;
        let block = self.heap_blocks[index];
        let ptr = block.start;
        if block.len == aligned {
            self.heap_blocks.remove(index);
        } else {
            self.heap_blocks[index].start = block.start.wrapping_add(aligned);
            self.heap_blocks[index].len = block.len.wrapping_sub(aligned);
        }
        let free = self.memory_manager_free_bytes();
        self.memory_manager_min_free = self.memory_manager_min_free.min(free);
        self.memory_manager_peak_used = self
            .memory_manager_peak_used
            .max(memory_manager_total_len().saturating_sub(free));
        Some(ptr)
    }

    fn free_raw(&mut self, addr: u32, size: u32) {
        let aligned = align_up(size.max(1), 8);
        let pool_start = DEFAULT_LAYOUT.memory_manager_address().get();
        let pool_end = pool_start.wrapping_add(DEFAULT_LAYOUT.memory_manager_size());
        if addr < pool_start || addr >= pool_end {
            return;
        }
        let Some(end) = addr.checked_add(aligned) else {
            return;
        };
        if end > pool_end {
            return;
        }

        let insert_at = self
            .heap_blocks
            .iter()
            .position(|block| block.start > addr)
            .unwrap_or(self.heap_blocks.len());
        self.heap_blocks.insert(
            insert_at,
            FreeBlock {
                start: addr,
                len: aligned,
            },
        );

        let mut cursor = insert_at;
        if cursor > 0 {
            let prev = self.heap_blocks[cursor - 1];
            let current = self.heap_blocks[cursor];
            if prev.start.wrapping_add(prev.len) == current.start {
                self.heap_blocks[cursor - 1].len = prev.len.wrapping_add(current.len);
                self.heap_blocks.remove(cursor);
                cursor -= 1;
            }
        }
        if cursor + 1 < self.heap_blocks.len() {
            let current = self.heap_blocks[cursor];
            let next = self.heap_blocks[cursor + 1];
            if current.start.wrapping_add(current.len) == next.start {
                self.heap_blocks[cursor].len = current.len.wrapping_add(next.len);
                self.heap_blocks.remove(cursor + 1);
            }
        }
    }

    fn alloc_ext<B: MemoryBus>(
        &mut self,
        memory: &mut B,
        size: u32,
    ) -> Result<Option<u32>, MemoryAccessError> {
        if size == 0 {
            return Ok(None);
        }
        let total = size.saturating_add(4);
        let Some(raw_addr) = self.alloc_raw(total) else {
            return Ok(None);
        };
        memory.write32(GuestAddr::new(raw_addr), size)?;
        let user_addr = raw_addr.wrapping_add(4);
        self.ext_allocations.insert(
            user_addr,
            HostAllocation {
                raw_addr,
                requested_len: size,
            },
        );
        Ok(Some(user_addr))
    }

    fn free_ext(&mut self, ptr: u32) {
        if ptr == 0 {
            return;
        }
        if let Some(allocation) = self.ext_allocations.remove(&ptr) {
            self.free_raw(
                allocation.raw_addr,
                allocation.requested_len.saturating_add(4),
            );
        }
    }

    fn ext_requested_len<B: MemoryBus>(
        &self,
        ptr: u32,
        memory: &B,
    ) -> Result<u32, MemoryAccessError> {
        if let Some(allocation) = self.ext_allocations.get(&ptr) {
            Ok(allocation.requested_len)
        } else if ptr >= 4 {
            memory.read32(GuestAddr::new(ptr.wrapping_sub(4)))
        } else {
            Ok(0)
        }
    }

    fn memory_manager_free_bytes(&self) -> u32 {
        self.heap_blocks.iter().map(|block| block.len).sum()
    }

    fn current_memory_manager_snapshot(&self) -> MemoryManagerSnapshot {
        MemoryManagerSnapshot {
            base: memory_manager_base(),
            len: memory_manager_total_len(),
            end: memory_manager_end(),
            left: self.memory_manager_free_bytes(),
            min: self.memory_manager_min_free,
            top: self.memory_manager_peak_used,
        }
    }

    fn sync_memory_manager_cells<B: MemoryBus>(
        &self,
        memory: &mut B,
    ) -> Result<(), MemoryAccessError> {
        write_memory_manager_cell_values(
            memory,
            self.mr_table_addr,
            MemoryManagerSnapshot {
                base: memory_manager_base(),
                len: memory_manager_total_len(),
                end: memory_manager_end(),
                left: self.memory_manager_free_bytes(),
                min: self.memory_manager_min_free,
                top: self.memory_manager_peak_used,
            },
        )
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
        if self.verbose {
            println!(
                "[host-callback] fn={:?} r0=0x{:X} r1=0x{:X} r2=0x{:X} r3=0x{:X}",
                callback,
                cpu.regs().reg(0),
                cpu.regs().reg(1),
                cpu.regs().reg(2),
                cpu.regs().reg(3),
            );
        }
        match callback {
            DsmHostFn::Test => {
                return_to_lr(cpu);
            }
            DsmHostFn::Log => {
                let msg_addr = cpu.regs().reg(0);
                let msg = read_guest_c_string(cpu, msg_addr, 4096)?;
                self.last_log_message = Some(msg.clone());
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
                self.rng_state = self.rng_state.wrapping_mul(214013).wrapping_add(2531011);
                cpu.regs_mut().set_reg(0, (self.rng_state >> 16) & 0x7FFF);
                return_to_lr(cpu);
            }
            DsmHostFn::MemGet => {
                let mem_base_ptr = cpu.regs().reg(0);
                let mem_len_ptr = cpu.regs().reg(1);
                let mem_len = DEFAULT_LAYOUT.memory_manager_size().min(DSM_MEM_GET_SIZE);
                if let Some(mem_base) = self.alloc_ext(cpu.memory_mut(), mem_len)? {
                    cpu.memory_mut()
                        .write32(GuestAddr::new(mem_base_ptr), mem_base)?;
                    cpu.memory_mut()
                        .write32(GuestAddr::new(mem_len_ptr), mem_len)?;
                    cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
                } else {
                    cpu.regs_mut().set_reg(0, MR_FAILED as u32);
                }
                self.sync_memory_manager_cells(cpu.memory_mut())?;
                return_to_lr(cpu);
            }
            DsmHostFn::MemFree => {
                let mem = cpu.regs().reg(0);
                self.free_ext(mem);
                cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
                self.sync_memory_manager_cells(cpu.memory_mut())?;
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
                    let allocated = self.alloc_raw(260).unwrap_or(0);
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
                        if xx < 0
                            || yy < 0
                            || xx >= SCREEN_WIDTH as i32
                            || yy >= SCREEN_HEIGHT as i32
                        {
                            continue;
                        }

                        let src_index = i as u32 + j as u32 * w as u32;
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
            return path;
        }

        let relative = raw
            .strip_prefix("mythroad/")
            .or_else(|| raw.strip_prefix("mythroad\\"))
            .unwrap_or(raw);
        if relative.is_empty() {
            self.working_dir.clone()
        } else {
            self.working_dir.join(Path::new(relative))
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

fn memory_manager_base() -> u32 {
    align_up(DEFAULT_LAYOUT.memory_manager_address().get(), 4)
}

fn memory_manager_total_len() -> u32 {
    let adjustment =
        memory_manager_base().wrapping_sub(DEFAULT_LAYOUT.memory_manager_address().get());
    DEFAULT_LAYOUT
        .memory_manager_size()
        .wrapping_sub(adjustment)
        & !3
}

fn memory_manager_end() -> u32 {
    memory_manager_base().wrapping_add(memory_manager_total_len())
}

impl MemoryManagerSnapshot {
    fn initial() -> Self {
        let len = memory_manager_total_len();
        Self {
            base: memory_manager_base(),
            len,
            end: memory_manager_end(),
            left: len,
            min: len,
            top: 0,
        }
    }
}

fn memory_manager_cell_addr(mr_table_addr: GuestAddr, offset: u32) -> GuestAddr {
    GuestAddr::new(
        mr_table_addr
            .get()
            .wrapping_add(MR_TABLE_MEMORY_CELLS_OFFSET)
            .wrapping_add(offset),
    )
}

fn seed_memory_manager_cells<B: MemoryBus>(
    memory: &mut B,
    mr_table_addr: GuestAddr,
    snapshot: MemoryManagerSnapshot,
) -> Result<(), MemoryAccessError> {
    memory.write32(
        GuestAddr::new(mr_table_addr.get().wrapping_add(LG_MEM_BASE_OFFSET)),
        memory_manager_cell_addr(mr_table_addr, LG_MEM_BASE_CELL_OFFSET).get(),
    )?;
    memory.write32(
        GuestAddr::new(mr_table_addr.get().wrapping_add(LG_MEM_LEN_OFFSET)),
        memory_manager_cell_addr(mr_table_addr, LG_MEM_LEN_CELL_OFFSET).get(),
    )?;
    memory.write32(
        GuestAddr::new(mr_table_addr.get().wrapping_add(LG_MEM_END_OFFSET)),
        memory_manager_cell_addr(mr_table_addr, LG_MEM_END_CELL_OFFSET).get(),
    )?;
    memory.write32(
        GuestAddr::new(mr_table_addr.get().wrapping_add(LG_MEM_LEFT_OFFSET)),
        memory_manager_cell_addr(mr_table_addr, LG_MEM_LEFT_CELL_OFFSET).get(),
    )?;
    memory.write32(
        GuestAddr::new(mr_table_addr.get().wrapping_add(LG_MEM_MIN_OFFSET)),
        memory_manager_cell_addr(mr_table_addr, LG_MEM_MIN_CELL_OFFSET).get(),
    )?;
    memory.write32(
        GuestAddr::new(mr_table_addr.get().wrapping_add(LG_MEM_TOP_OFFSET)),
        memory_manager_cell_addr(mr_table_addr, LG_MEM_TOP_CELL_OFFSET).get(),
    )?;
    write_memory_manager_cell_values(memory, mr_table_addr, snapshot)
}

fn write_memory_manager_cell_values<B: MemoryBus>(
    memory: &mut B,
    mr_table_addr: GuestAddr,
    snapshot: MemoryManagerSnapshot,
) -> Result<(), MemoryAccessError> {
    memory.write32(
        memory_manager_cell_addr(mr_table_addr, LG_MEM_BASE_CELL_OFFSET),
        snapshot.base,
    )?;
    memory.write32(
        memory_manager_cell_addr(mr_table_addr, LG_MEM_LEN_CELL_OFFSET),
        snapshot.len,
    )?;
    memory.write32(
        memory_manager_cell_addr(mr_table_addr, LG_MEM_END_CELL_OFFSET),
        snapshot.end,
    )?;
    memory.write32(
        memory_manager_cell_addr(mr_table_addr, LG_MEM_LEFT_CELL_OFFSET),
        snapshot.left,
    )?;
    memory.write32(
        memory_manager_cell_addr(mr_table_addr, LG_MEM_MIN_CELL_OFFSET),
        snapshot.min,
    )?;
    memory.write32(
        memory_manager_cell_addr(mr_table_addr, LG_MEM_TOP_CELL_OFFSET),
        snapshot.top,
    )?;
    Ok(())
}

fn reserved_guest_addr(mr_table_addr: GuestAddr, offset: u32) -> GuestAddr {
    GuestAddr::new(mr_table_addr.get().wrapping_add(offset))
}

fn internal_data_cell_addr(mr_table_addr: GuestAddr, offset: u32) -> GuestAddr {
    reserved_guest_addr(
        mr_table_addr,
        MR_TABLE_INTERNAL_DATA_OFFSET.wrapping_add(offset),
    )
}

fn legacy_runtime_addr(mr_table_addr: GuestAddr, offset: u32) -> GuestAddr {
    reserved_guest_addr(
        mr_table_addr,
        MR_TABLE_LEGACY_RUNTIME_OFFSET.wrapping_add(offset),
    )
}

fn clear_guest_bytes<B: MemoryBus>(
    memory: &mut B,
    addr: GuestAddr,
    len: usize,
) -> Result<(), MemoryAccessError> {
    for index in 0..len {
        memory.write8(GuestAddr::new(addr.get().wrapping_add(index as u32)), 0)?;
    }
    Ok(())
}

fn write_guest_c_string_to_memory<B: MemoryBus>(
    memory: &mut B,
    addr: GuestAddr,
    value: &str,
    capacity: usize,
) -> Result<(), MemoryAccessError> {
    clear_guest_bytes(memory, addr, capacity)?;
    let bytes = value.as_bytes();
    let limit = bytes.len().min(capacity.saturating_sub(1));
    for (index, byte) in bytes.iter().take(limit).enumerate() {
        memory.write8(GuestAddr::new(addr.get().wrapping_add(index as u32)), *byte)?;
    }
    Ok(())
}

fn seed_legacy_runtime_data<B: MemoryBus>(
    memory: &mut B,
    mr_table_addr: GuestAddr,
) -> Result<(), MemoryAccessError> {
    let screen_buf_ptr_cell = legacy_runtime_addr(mr_table_addr, LEGACY_SCREEN_BUF_PTR_CELL_OFFSET);
    let screen_w_cell = legacy_runtime_addr(mr_table_addr, LEGACY_SCREEN_W_CELL_OFFSET);
    let screen_h_cell = legacy_runtime_addr(mr_table_addr, LEGACY_SCREEN_H_CELL_OFFSET);
    let screen_bit_cell = legacy_runtime_addr(mr_table_addr, LEGACY_SCREEN_BIT_CELL_OFFSET);
    let ram_file_ptr_cell = legacy_runtime_addr(mr_table_addr, LEGACY_RAM_FILE_PTR_CELL_OFFSET);
    let ram_file_len_cell = legacy_runtime_addr(mr_table_addr, LEGACY_RAM_FILE_LEN_CELL_OFFSET);
    let sound_on_cell = legacy_runtime_addr(mr_table_addr, LEGACY_SOUND_ON_CELL_OFFSET);
    let shake_on_cell = legacy_runtime_addr(mr_table_addr, LEGACY_SHAKE_ON_CELL_OFFSET);
    let bitmap = legacy_runtime_addr(mr_table_addr, LEGACY_BITMAP_BUF_OFFSET);
    let tile = legacy_runtime_addr(mr_table_addr, LEGACY_TILE_BUF_OFFSET);
    let map = legacy_runtime_addr(mr_table_addr, LEGACY_MAP_BUF_OFFSET);
    let sound = legacy_runtime_addr(mr_table_addr, LEGACY_SOUND_BUF_OFFSET);
    let sprite = legacy_runtime_addr(mr_table_addr, LEGACY_SPRITE_BUF_OFFSET);
    let pack_filename = legacy_runtime_addr(mr_table_addr, LEGACY_PACK_FILENAME_BUF_OFFSET);
    let start_filename = legacy_runtime_addr(mr_table_addr, LEGACY_START_FILENAME_BUF_OFFSET);
    let old_pack_filename = legacy_runtime_addr(mr_table_addr, LEGACY_OLD_PACK_FILENAME_BUF_OFFSET);
    let old_start_filename =
        legacy_runtime_addr(mr_table_addr, LEGACY_OLD_START_FILENAME_BUF_OFFSET);
    let start_fileparameter =
        legacy_runtime_addr(mr_table_addr, LEGACY_START_FILEPARAMETER_BUF_OFFSET);
    let mr_entry = legacy_runtime_addr(mr_table_addr, LEGACY_ENTRY_BUF_OFFSET);

    for (slot_offset, value) in [
        (MR_SCREEN_BUF_OFFSET, screen_buf_ptr_cell.get()),
        (MR_SCREEN_W_OFFSET, screen_w_cell.get()),
        (MR_SCREEN_H_OFFSET, screen_h_cell.get()),
        (MR_SCREEN_BIT_OFFSET, screen_bit_cell.get()),
        (MR_BITMAP_OFFSET, bitmap.get()),
        (MR_TILE_OFFSET, tile.get()),
        (MR_MAP_OFFSET, map.get()),
        (MR_SOUND_OFFSET, sound.get()),
        (MR_SPRITE_OFFSET, sprite.get()),
        (PACK_FILENAME_OFFSET, pack_filename.get()),
        (START_FILENAME_OFFSET, start_filename.get()),
        (OLD_PACK_FILENAME_OFFSET, old_pack_filename.get()),
        (OLD_START_FILENAME_OFFSET, old_start_filename.get()),
        (MR_RAM_FILE_OFFSET, ram_file_ptr_cell.get()),
        (MR_RAM_FILE_LEN_OFFSET, ram_file_len_cell.get()),
        (MR_SOUND_ON_OFFSET, sound_on_cell.get()),
        (MR_SHAKE_ON_OFFSET, shake_on_cell.get()),
        (START_FILEPARAMETER_OFFSET, start_fileparameter.get()),
        (MR_ENTRY_OFFSET, mr_entry.get()),
    ] {
        memory.write32(
            GuestAddr::new(mr_table_addr.get().wrapping_add(slot_offset)),
            value,
        )?;
    }

    for cell in [
        screen_buf_ptr_cell,
        screen_w_cell,
        screen_h_cell,
        screen_bit_cell,
        ram_file_ptr_cell,
        ram_file_len_cell,
        sound_on_cell,
        shake_on_cell,
    ] {
        memory.write32(cell, 0)?;
    }

    clear_guest_bytes(
        memory,
        bitmap,
        LEGACY_BITMAP_STRUCT_LEN * LEGACY_BITMAP_COUNT,
    )?;
    clear_guest_bytes(memory, tile, LEGACY_TILE_STRUCT_LEN * LEGACY_TILE_COUNT)?;
    clear_guest_bytes(memory, map, 4 * LEGACY_MAP_PTR_COUNT)?;
    clear_guest_bytes(memory, sound, LEGACY_SOUND_STRUCT_LEN * LEGACY_SOUND_COUNT)?;
    clear_guest_bytes(
        memory,
        sprite,
        LEGACY_SPRITE_STRUCT_LEN * LEGACY_SPRITE_COUNT,
    )?;

    for buffer in [
        pack_filename,
        start_filename,
        old_pack_filename,
        old_start_filename,
        start_fileparameter,
        mr_entry,
    ] {
        clear_guest_bytes(memory, buffer, LEGACY_FILENAME_BUFFER_LEN)?;
    }

    Ok(())
}

fn seed_internal_runtime_tables<B: MemoryBus>(
    memory: &mut B,
    mr_table_addr: GuestAddr,
    safe_stub_addr: GuestAddr,
) -> Result<(), MemoryAccessError> {
    let internal_table_addr = reserved_guest_addr(mr_table_addr, MR_TABLE_INTERNAL_TABLE_OFFSET);
    let port_table_addr = reserved_guest_addr(mr_table_addr, MR_TABLE_PORT_TABLE_OFFSET);
    let m0_files_addr = reserved_guest_addr(mr_table_addr, MR_TABLE_M0_FILES_OFFSET);

    memory.write32(
        GuestAddr::new(mr_table_addr.get().wrapping_add(MR_C_INTERNAL_TABLE_OFFSET)),
        internal_table_addr.get(),
    )?;
    memory.write32(
        GuestAddr::new(mr_table_addr.get().wrapping_add(MR_C_PORT_TABLE_OFFSET)),
        port_table_addr.get(),
    )?;

    for index in 0..MR_C_PORT_TABLE_LEN {
        memory.write32(
            GuestAddr::new(port_table_addr.get().wrapping_add(index.wrapping_mul(4))),
            0,
        )?;
    }

    for index in 0..MR_M0_FILES_LEN {
        memory.write32(
            GuestAddr::new(m0_files_addr.get().wrapping_add(index.wrapping_mul(4))),
            0,
        )?;
    }

    let data_entries = [
        m0_files_addr.get(),
        internal_data_cell_addr(mr_table_addr, VM_STATE_CELL_OFFSET).get(),
        internal_data_cell_addr(mr_table_addr, MR_STATE_CELL_OFFSET).get(),
        internal_data_cell_addr(mr_table_addr, BI_CELL_OFFSET).get(),
        internal_data_cell_addr(mr_table_addr, MR_TIMER_P_CELL_OFFSET).get(),
        internal_data_cell_addr(mr_table_addr, MR_TIMER_STATE_CELL_OFFSET).get(),
        internal_data_cell_addr(mr_table_addr, MR_TIMER_RUN_WITHOUT_PAUSE_CELL_OFFSET).get(),
        internal_data_cell_addr(mr_table_addr, MR_GZ_IN_BUF_CELL_OFFSET).get(),
        internal_data_cell_addr(mr_table_addr, MR_GZ_OUT_BUF_CELL_OFFSET).get(),
        internal_data_cell_addr(mr_table_addr, LG_GZINPTR_CELL_OFFSET).get(),
        internal_data_cell_addr(mr_table_addr, LG_GZOUTCNT_CELL_OFFSET).get(),
        internal_data_cell_addr(mr_table_addr, MR_SMS_CFG_NEED_SAVE_CELL_OFFSET).get(),
    ];

    for (index, value) in data_entries.into_iter().enumerate() {
        memory.write32(
            GuestAddr::new(
                internal_table_addr
                    .get()
                    .wrapping_add((index as u32).wrapping_mul(4)),
            ),
            value,
        )?;
    }

    for index in data_entries.len() as u32..(MR_C_INTERNAL_TABLE_LEN - 1) {
        memory.write32(
            GuestAddr::new(
                internal_table_addr
                    .get()
                    .wrapping_add(index.wrapping_mul(4)),
            ),
            safe_stub_addr.get(),
        )?;
    }
    memory.write32(
        GuestAddr::new(
            internal_table_addr
                .get()
                .wrapping_add((MR_C_INTERNAL_TABLE_LEN - 1).wrapping_mul(4)),
        ),
        0,
    )?;

    for offset in [
        VM_STATE_CELL_OFFSET,
        MR_STATE_CELL_OFFSET,
        BI_CELL_OFFSET,
        MR_TIMER_P_CELL_OFFSET,
        MR_TIMER_STATE_CELL_OFFSET,
        MR_TIMER_RUN_WITHOUT_PAUSE_CELL_OFFSET,
        MR_GZ_IN_BUF_CELL_OFFSET,
        MR_GZ_OUT_BUF_CELL_OFFSET,
        LG_GZINPTR_CELL_OFFSET,
        LG_GZOUTCNT_CELL_OFFSET,
        MR_SMS_CFG_NEED_SAVE_CELL_OFFSET,
    ] {
        memory.write32(internal_data_cell_addr(mr_table_addr, offset), 0)?;
    }

    Ok(())
}

fn align_up(value: u32, align: u32) -> u32 {
    if align <= 1 {
        return value;
    }
    let mask = align - 1;
    value.wrapping_add(mask) & !mask
}

fn guest_strlen<B: MemoryBus>(
    memory: &B,
    addr: u32,
    max_len: usize,
) -> Result<u32, MemoryAccessError> {
    let mut len = 0u32;
    while (len as usize) < max_len {
        let byte = memory.read8(GuestAddr::new(addr.wrapping_add(len)))?;
        if byte == 0 {
            break;
        }
        len = len.wrapping_add(1);
    }
    Ok(len)
}

fn compare_guest_c_strings<B: MemoryBus>(
    memory: &B,
    left: u32,
    right: u32,
    limit: Option<u32>,
) -> Result<i32, MemoryAccessError> {
    let mut offset = 0u32;
    loop {
        if let Some(limit) = limit {
            if offset >= limit {
                return Ok(0);
            }
        }
        let a = memory.read8(GuestAddr::new(left.wrapping_add(offset)))?;
        let b = memory.read8(GuestAddr::new(right.wrapping_add(offset)))?;
        if a != b {
            return Ok(a as i32 - b as i32);
        }
        if a == 0 {
            return Ok(0);
        }
        offset = offset.wrapping_add(1);
    }
}

fn guest_variadic_arg<B: MemoryBus>(
    cpu: &Cpu<B>,
    fixed_arg_count: usize,
    var_index: usize,
) -> Result<u32, MemoryAccessError> {
    let absolute_index = fixed_arg_count + var_index;
    if absolute_index < 4 {
        Ok(cpu.regs().reg(absolute_index))
    } else {
        let stack_index = absolute_index - 4;
        cpu.memory().read32(GuestAddr::new(
            cpu.regs()
                .sp()
                .wrapping_add((stack_index as u32).wrapping_mul(4)),
        ))
    }
}

fn format_guest_string<B: MemoryBus>(
    cpu: &Cpu<B>,
    fmt_addr: u32,
    fixed_arg_count: usize,
) -> Result<String, MemoryAccessError> {
    let fmt = read_guest_c_string(cpu, fmt_addr, 4096)?;
    let mut out = String::new();
    let mut chars = fmt.chars().peekable();
    let mut arg_index = 0usize;

    while let Some(ch) = chars.next() {
        if ch != '%' {
            out.push(ch);
            continue;
        }
        if matches!(chars.peek(), Some('%')) {
            chars.next();
            out.push('%');
            continue;
        }

        let mut spec = None;
        while let Some(&next) = chars.peek() {
            if next.is_ascii_digit() || matches!(next, '-' | '+' | ' ' | '#' | '0' | 'l' | 'h') {
                chars.next();
                continue;
            }
            spec = chars.next();
            break;
        }

        match spec.unwrap_or('%') {
            's' => {
                let ptr = guest_variadic_arg(cpu, fixed_arg_count, arg_index)?;
                arg_index += 1;
                out.push_str(&read_guest_c_string(cpu, ptr, 4096)?);
            }
            'c' => {
                let value = guest_variadic_arg(cpu, fixed_arg_count, arg_index)?;
                arg_index += 1;
                out.push(char::from_u32(value & 0xFF).unwrap_or('\0'));
            }
            'd' | 'i' => {
                let value = guest_variadic_arg(cpu, fixed_arg_count, arg_index)? as i32;
                arg_index += 1;
                out.push_str(&value.to_string());
            }
            'u' => {
                let value = guest_variadic_arg(cpu, fixed_arg_count, arg_index)?;
                arg_index += 1;
                out.push_str(&value.to_string());
            }
            'x' => {
                let value = guest_variadic_arg(cpu, fixed_arg_count, arg_index)?;
                arg_index += 1;
                out.push_str(&format!("{value:x}"));
            }
            'X' => {
                let value = guest_variadic_arg(cpu, fixed_arg_count, arg_index)?;
                arg_index += 1;
                out.push_str(&format!("{value:X}"));
            }
            'p' => {
                let value = guest_variadic_arg(cpu, fixed_arg_count, arg_index)?;
                arg_index += 1;
                out.push_str(&format!("0x{value:X}"));
            }
            other => {
                out.push('%');
                out.push(other);
            }
        }
    }

    Ok(out)
}

fn parse_guest_i32(raw: &str) -> i32 {
    let trimmed = raw.trim_start();
    let mut end = 0usize;
    for (index, ch) in trimmed.char_indices() {
        if index == 0 && matches!(ch, '+' | '-') {
            end = ch.len_utf8();
            continue;
        }
        if ch.is_ascii_digit() {
            end = index + ch.len_utf8();
        } else {
            break;
        }
    }
    trimmed.get(..end).unwrap_or("").parse::<i32>().unwrap_or(0)
}

fn parse_guest_strtoul(raw: &str, base: u32) -> (u32, usize) {
    let bytes = raw.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() && (bytes[index] as char).is_ascii_whitespace() {
        index += 1;
    }
    let start = index;
    if index < bytes.len() && matches!(bytes[index], b'+' | b'-') {
        index += 1;
    }

    let mut actual_base = base;
    if actual_base == 0 {
        actual_base = 10;
        if index + 1 < bytes.len()
            && bytes[index] == b'0'
            && matches!(bytes[index + 1], b'x' | b'X')
        {
            actual_base = 16;
            index += 2;
        } else if index < bytes.len() && bytes[index] == b'0' {
            actual_base = 8;
            index += 1;
        }
    } else if actual_base == 16
        && index + 1 < bytes.len()
        && bytes[index] == b'0'
        && matches!(bytes[index + 1], b'x' | b'X')
    {
        index += 2;
    }

    let digits_start = index;
    let mut value = 0u32;
    while index < bytes.len() {
        let ch = bytes[index] as char;
        let Some(digit) = ch.to_digit(actual_base) else {
            break;
        };
        value = value
            .saturating_mul(actual_base)
            .saturating_add(digit as u32);
        index += 1;
    }

    if digits_start == index {
        return (0, start);
    }
    (value, index)
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
