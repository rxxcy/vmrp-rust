use std::collections::{BTreeMap, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddrV4, TcpStream, UdpSocket};
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
pub const MR_IGNORE: i32 = 1;
pub const MR_WAITING: i32 = 2;
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
const MR_OPEN_OFFSET: u32 = 0xA0;
const MR_CLOSE_OFFSET: u32 = 0xA4;
const MR_INFO_OFFSET: u32 = 0xA8;
const MR_WRITE_OFFSET: u32 = 0xAC;
const MR_READ_OFFSET: u32 = 0xB0;
const MR_SEEK_OFFSET: u32 = 0xB4;
const MR_GET_LEN_OFFSET: u32 = 0xB8;
const MR_REMOVE_OFFSET: u32 = 0xBC;
const MR_RENAME_OFFSET: u32 = 0xC0;
const MR_MKDIR_OFFSET: u32 = 0xC4;
const MR_RMDIR_OFFSET: u32 = 0xC8;
const MR_FIND_START_OFFSET: u32 = 0xCC;
const MR_FIND_GET_NEXT_OFFSET: u32 = 0xD0;
const MR_FIND_STOP_OFFSET: u32 = 0xD4;
const MR_EXIT_OFFSET: u32 = 0xD8;
const MR_SEND_SMS_OFFSET: u32 = 0xEC;
const MR_CALL_OFFSET: u32 = 0xF0;
const MR_GET_NETWORK_ID_OFFSET: u32 = 0xF4;
const MR_CONNECT_WAP_OFFSET: u32 = 0xF8;
const MR_MENU_CREATE_OFFSET: u32 = 0xFC;
const MR_MENU_SET_ITEM_OFFSET: u32 = 0x100;
const MR_MENU_SHOW_OFFSET: u32 = 0x104;
const MR_MENU_RELEASE_OFFSET: u32 = 0x10C;
const MR_MENU_REFRESH_OFFSET: u32 = 0x110;
const MR_START_SHAKE_OFFSET: u32 = 0xDC;
const MR_STOP_SHAKE_OFFSET: u32 = 0xE0;
const MR_PLAY_SOUND_OFFSET: u32 = 0xE4;
const MR_STOP_SOUND_OFFSET: u32 = 0xE8;
const MR_DIALOG_CREATE_OFFSET: u32 = 0x114;
const MR_DIALOG_RELEASE_OFFSET: u32 = 0x118;
const MR_DIALOG_REFRESH_OFFSET: u32 = 0x11C;
const MR_TEXT_CREATE_OFFSET: u32 = 0x120;
const MR_TEXT_RELEASE_OFFSET: u32 = 0x124;
const MR_TEXT_REFRESH_OFFSET: u32 = 0x128;
const MR_EDIT_CREATE_OFFSET: u32 = 0x12C;
const MR_EDIT_RELEASE_OFFSET: u32 = 0x130;
const MR_EDIT_GET_TEXT_OFFSET: u32 = 0x134;
const MR_WIN_CREATE_OFFSET: u32 = 0x138;
const MR_WIN_RELEASE_OFFSET: u32 = 0x13C;
const MR_INIT_NETWORK_OFFSET: u32 = 0x144;
const MR_CLOSE_NETWORK_OFFSET: u32 = 0x148;
const MR_GET_HOST_BY_NAME_OFFSET: u32 = 0x14C;
const MR_SOCKET_OFFSET: u32 = 0x150;
const MR_CONNECT_OFFSET: u32 = 0x154;
const MR_CLOSE_SOCKET_OFFSET: u32 = 0x158;
const MR_RECV_OFFSET: u32 = 0x15C;
const MR_RECVFROM_OFFSET: u32 = 0x160;
const MR_SEND_OFFSET: u32 = 0x164;
const MR_SENDTO_OFFSET: u32 = 0x168;
const DISP_UP_EX_OFFSET: u32 = 0x1D8;
const DRAW_POINT_OFFSET: u32 = 0x1DC;
const DRAW_BITMAP_OFFSET: u32 = 0x1E0;
const DRAW_BITMAP_EX_OFFSET: u32 = 0x1E4;
const DRAW_RECT_OFFSET: u32 = 0x1E8;
const DRAW_TEXT_OFFSET: u32 = 0x1EC;
const BITMAP_CHECK_OFFSET: u32 = 0x1F0;
const MR_GET_SCREEN_INFO_OFFSET: u32 = 0x140;
const MR_TEST_COM_OFFSET: u32 = 0x208;
const MR_TEST_COM1_OFFSET: u32 = 0x20C;
const MR_READ_FILE_OFFSET: u32 = 0x1F4;
const MR_WSTRLEN_OFFSET: u32 = 0x1F8;
const DRAW_TEXT_EX_OFFSET: u32 = 0x200;
const DRAW_TEXT_EX_IS_UNICODE: u32 = 1;
const DRAW_TEXT_EX_IS_AUTO_NEWLINE: u32 = 2;
const MR_NET_ID_MOBILE: i32 = 0;
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
pub const MR_EXT_FUNCTION_NEW_ADDR: GuestAddr = GuestAddr::new(0x181BC0);
const RET_ZERO_STUB_ADDR: GuestAddr = GuestAddr::new(0x181580);
const MR_EXIT_ADDR: GuestAddr = GuestAddr::new(0x181590);
const MR_SEND_SMS_ADDR: GuestAddr = GuestAddr::new(0x1815A0);
const MR_MENU_CREATE_ADDR: GuestAddr = GuestAddr::new(0x181C00);
const MR_MENU_SET_ITEM_ADDR: GuestAddr = GuestAddr::new(0x181C10);
const MR_MENU_SHOW_ADDR: GuestAddr = GuestAddr::new(0x181C20);
const MR_MENU_RELEASE_ADDR: GuestAddr = GuestAddr::new(0x181C30);
const MR_MENU_REFRESH_ADDR: GuestAddr = GuestAddr::new(0x181C40);
const MR_WIN_CREATE_ADDR: GuestAddr = GuestAddr::new(0x181C50);
const MR_WIN_RELEASE_ADDR: GuestAddr = GuestAddr::new(0x181C60);
const MR_GET_HOST_BY_NAME_ADDR: GuestAddr = GuestAddr::new(0x181900);
const MR_INIT_NETWORK_ADDR: GuestAddr = GuestAddr::new(0x181940);
const MR_CLOSE_NETWORK_ADDR: GuestAddr = GuestAddr::new(0x181980);
const MR_SOCKET_ADDR: GuestAddr = GuestAddr::new(0x1819C0);
const MR_CONNECT_ADDR: GuestAddr = GuestAddr::new(0x181A00);
const MR_GET_SOCKET_STATE_ADDR: GuestAddr = GuestAddr::new(0x181A40);
const MR_CLOSE_SOCKET_ADDR: GuestAddr = GuestAddr::new(0x181A80);
const MR_RECV_ADDR: GuestAddr = GuestAddr::new(0x181AC0);
const MR_SEND_ADDR: GuestAddr = GuestAddr::new(0x181B00);
const MR_RECVFROM_ADDR: GuestAddr = GuestAddr::new(0x181B40);
const MR_SENDTO_ADDR: GuestAddr = GuestAddr::new(0x181B80);
const MR_START_SHAKE_ADDR: GuestAddr = GuestAddr::new(0x1815C0);
const MR_STOP_SHAKE_ADDR: GuestAddr = GuestAddr::new(0x181600);
const MR_PLAY_SOUND_ADDR: GuestAddr = GuestAddr::new(0x181640);
const MR_STOP_SOUND_ADDR: GuestAddr = GuestAddr::new(0x181680);
const MR_DIALOG_CREATE_ADDR: GuestAddr = GuestAddr::new(0x1816C0);
const MR_DIALOG_RELEASE_ADDR: GuestAddr = GuestAddr::new(0x181700);
const MR_DIALOG_REFRESH_ADDR: GuestAddr = GuestAddr::new(0x181740);
const MR_TEXT_CREATE_ADDR: GuestAddr = GuestAddr::new(0x181780);
const MR_TEXT_RELEASE_ADDR: GuestAddr = GuestAddr::new(0x1817C0);
const MR_TEXT_REFRESH_ADDR: GuestAddr = GuestAddr::new(0x181800);
const MR_EDIT_CREATE_ADDR: GuestAddr = GuestAddr::new(0x181840);
const MR_EDIT_RELEASE_ADDR: GuestAddr = GuestAddr::new(0x181880);
const MR_EDIT_GET_TEXT_ADDR: GuestAddr = GuestAddr::new(0x1818C0);
const MR_CALL_ADDR: GuestAddr = GuestAddr::new(0x1818D0);
const MR_GET_NETWORK_ID_ADDR: GuestAddr = GuestAddr::new(0x1818E0);
const MR_CONNECT_WAP_ADDR: GuestAddr = GuestAddr::new(0x1818F0);
const DISP_UP_EX_ADDR: GuestAddr = GuestAddr::new(0x181CC0);
const DRAW_POINT_ADDR: GuestAddr = GuestAddr::new(0x181D00);
const DRAW_BITMAP_ADDR: GuestAddr = GuestAddr::new(0x181D20);
const DRAW_BITMAP_EX_ADDR: GuestAddr = GuestAddr::new(0x181D40);
const DRAW_RECT_ADDR: GuestAddr = GuestAddr::new(0x181D80);
const DRAW_TEXT_ADDR: GuestAddr = GuestAddr::new(0x181DC0);
const BITMAP_CHECK_ADDR: GuestAddr = GuestAddr::new(0x181E00);
const DRAW_TEXT_EX_ADDR: GuestAddr = GuestAddr::new(0x181E40);
const MR_WSTRLEN_ADDR: GuestAddr = GuestAddr::new(0x181E80);
const MR_OPEN_ADDR: GuestAddr = GuestAddr::new(0x181EC0);
const MR_CLOSE_ADDR: GuestAddr = GuestAddr::new(0x181ED0);
const MR_INFO_ADDR: GuestAddr = GuestAddr::new(0x181EE0);
const MR_WRITE_ADDR: GuestAddr = GuestAddr::new(0x181EF0);
const MR_READ_ADDR: GuestAddr = GuestAddr::new(0x181F00);
const MR_SEEK_ADDR: GuestAddr = GuestAddr::new(0x181F10);
const MR_GET_LEN_ADDR: GuestAddr = GuestAddr::new(0x181F20);
const MR_REMOVE_ADDR: GuestAddr = GuestAddr::new(0x181F30);
const MR_RENAME_ADDR: GuestAddr = GuestAddr::new(0x181F40);
const MR_MKDIR_ADDR: GuestAddr = GuestAddr::new(0x181F50);
const MR_RMDIR_ADDR: GuestAddr = GuestAddr::new(0x181F60);
const MR_FIND_START_ADDR: GuestAddr = GuestAddr::new(0x181F70);
const MR_FIND_GET_NEXT_ADDR: GuestAddr = GuestAddr::new(0x181F80);
const MR_FIND_STOP_ADDR: GuestAddr = GuestAddr::new(0x181F90);
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
const MR_SMS_RETURN_FLAG_OFFSET: u32 = 0x22C;
const MR_SMS_RETURN_VAL_OFFSET: u32 = 0x230;
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
const LEGACY_SMS_RETURN_FLAG_CELL_OFFSET: u32 = 0x20;
const LEGACY_SMS_RETURN_VAL_CELL_OFFSET: u32 = 0x24;
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
struct PluginExtLoad {
    code_base: GuestAddr,
    context_addr: GuestAddr,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GuestBitmapDraw {
    p: u32,
    w: u16,
    h: u16,
    x: i16,
    y: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GuestTransMatrix {
    a: i16,
    b: i16,
    c: i16,
    d: i16,
    rop: u16,
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
            memory.write32(slot, RET_ZERO_STUB_ADDR.get())?;
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
        for (offset, addr) in [
            (MR_MENU_CREATE_OFFSET, MR_MENU_CREATE_ADDR),
            (MR_MENU_SET_ITEM_OFFSET, MR_MENU_SET_ITEM_ADDR),
            (MR_MENU_SHOW_OFFSET, MR_MENU_SHOW_ADDR),
            (MR_MENU_RELEASE_OFFSET, MR_MENU_RELEASE_ADDR),
            (MR_MENU_REFRESH_OFFSET, MR_MENU_REFRESH_ADDR),
            (MR_OPEN_OFFSET, MR_OPEN_ADDR),
            (MR_CLOSE_OFFSET, MR_CLOSE_ADDR),
            (MR_INFO_OFFSET, MR_INFO_ADDR),
            (MR_WRITE_OFFSET, MR_WRITE_ADDR),
            (MR_READ_OFFSET, MR_READ_ADDR),
            (MR_SEEK_OFFSET, MR_SEEK_ADDR),
            (MR_GET_LEN_OFFSET, MR_GET_LEN_ADDR),
            (MR_REMOVE_OFFSET, MR_REMOVE_ADDR),
            (MR_RENAME_OFFSET, MR_RENAME_ADDR),
            (MR_MKDIR_OFFSET, MR_MKDIR_ADDR),
            (MR_RMDIR_OFFSET, MR_RMDIR_ADDR),
            (MR_FIND_START_OFFSET, MR_FIND_START_ADDR),
            (MR_FIND_GET_NEXT_OFFSET, MR_FIND_GET_NEXT_ADDR),
            (MR_FIND_STOP_OFFSET, MR_FIND_STOP_ADDR),
            (MR_EXIT_OFFSET, MR_EXIT_ADDR),
            (MR_SEND_SMS_OFFSET, MR_SEND_SMS_ADDR),
            (MR_CALL_OFFSET, MR_CALL_ADDR),
            (MR_GET_NETWORK_ID_OFFSET, MR_GET_NETWORK_ID_ADDR),
            (MR_CONNECT_WAP_OFFSET, MR_CONNECT_WAP_ADDR),
            (MR_GET_HOST_BY_NAME_OFFSET, MR_GET_HOST_BY_NAME_ADDR),
            (MR_INIT_NETWORK_OFFSET, MR_INIT_NETWORK_ADDR),
            (MR_CLOSE_NETWORK_OFFSET, MR_CLOSE_NETWORK_ADDR),
            (MR_SOCKET_OFFSET, MR_SOCKET_ADDR),
            (MR_CONNECT_OFFSET, MR_CONNECT_ADDR),
            (MR_CLOSE_SOCKET_OFFSET, MR_CLOSE_SOCKET_ADDR),
            (MR_RECV_OFFSET, MR_RECV_ADDR),
            (MR_SEND_OFFSET, MR_SEND_ADDR),
            (MR_RECVFROM_OFFSET, MR_RECVFROM_ADDR),
            (MR_SENDTO_OFFSET, MR_SENDTO_ADDR),
            (MR_START_SHAKE_OFFSET, MR_START_SHAKE_ADDR),
            (MR_STOP_SHAKE_OFFSET, MR_STOP_SHAKE_ADDR),
            (MR_PLAY_SOUND_OFFSET, MR_PLAY_SOUND_ADDR),
            (MR_STOP_SOUND_OFFSET, MR_STOP_SOUND_ADDR),
            (MR_DIALOG_CREATE_OFFSET, MR_DIALOG_CREATE_ADDR),
            (MR_DIALOG_RELEASE_OFFSET, MR_DIALOG_RELEASE_ADDR),
            (MR_DIALOG_REFRESH_OFFSET, MR_DIALOG_REFRESH_ADDR),
            (MR_TEXT_CREATE_OFFSET, MR_TEXT_CREATE_ADDR),
            (MR_TEXT_RELEASE_OFFSET, MR_TEXT_RELEASE_ADDR),
            (MR_TEXT_REFRESH_OFFSET, MR_TEXT_REFRESH_ADDR),
            (MR_EDIT_CREATE_OFFSET, MR_EDIT_CREATE_ADDR),
            (MR_EDIT_RELEASE_OFFSET, MR_EDIT_RELEASE_ADDR),
            (MR_EDIT_GET_TEXT_OFFSET, MR_EDIT_GET_TEXT_ADDR),
            (MR_WIN_CREATE_OFFSET, MR_WIN_CREATE_ADDR),
            (MR_WIN_RELEASE_OFFSET, MR_WIN_RELEASE_ADDR),
            (DISP_UP_EX_OFFSET, DISP_UP_EX_ADDR),
            (DRAW_POINT_OFFSET, DRAW_POINT_ADDR),
            (DRAW_BITMAP_OFFSET, DRAW_BITMAP_ADDR),
            (DRAW_BITMAP_EX_OFFSET, DRAW_BITMAP_EX_ADDR),
            (DRAW_RECT_OFFSET, DRAW_RECT_ADDR),
            (DRAW_TEXT_OFFSET, DRAW_TEXT_ADDR),
            (BITMAP_CHECK_OFFSET, BITMAP_CHECK_ADDR),
            (MR_WSTRLEN_OFFSET, MR_WSTRLEN_ADDR),
            (DRAW_TEXT_EX_OFFSET, DRAW_TEXT_EX_ADDR),
        ] {
            let slot = GuestAddr::new(self.mr_table_addr.get().wrapping_add(offset));
            memory.write32(slot, addr.get())?;
        }

        let c_function_new_slot = GuestAddr::new(
            self.mr_table_addr
                .get()
                .wrapping_add(MR_C_FUNCTION_NEW_OFFSET),
        );
        memory.write32(c_function_new_slot, self.mr_c_function_new_addr.get())?;

        seed_memory_manager_cells(memory, self.mr_table_addr, MemoryManagerSnapshot::initial())?;
        seed_internal_runtime_tables(memory, self.mr_table_addr, RET_ZERO_STUB_ADDR)?;
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
            RET_ZERO_STUB_ADDR,
            MR_MENU_CREATE_ADDR,
            MR_MENU_SET_ITEM_ADDR,
            MR_MENU_SHOW_ADDR,
            MR_MENU_RELEASE_ADDR,
            MR_MENU_REFRESH_ADDR,
            MR_WIN_CREATE_ADDR,
            MR_WIN_RELEASE_ADDR,
            MR_OPEN_ADDR,
            MR_CLOSE_ADDR,
            MR_INFO_ADDR,
            MR_WRITE_ADDR,
            MR_READ_ADDR,
            MR_SEEK_ADDR,
            MR_GET_LEN_ADDR,
            MR_REMOVE_ADDR,
            MR_RENAME_ADDR,
            MR_MKDIR_ADDR,
            MR_RMDIR_ADDR,
            MR_FIND_START_ADDR,
            MR_FIND_GET_NEXT_ADDR,
            MR_FIND_STOP_ADDR,
            MR_EXIT_ADDR,
            MR_SEND_SMS_ADDR,
            MR_CALL_ADDR,
            MR_GET_NETWORK_ID_ADDR,
            MR_CONNECT_WAP_ADDR,
            MR_GET_HOST_BY_NAME_ADDR,
            MR_INIT_NETWORK_ADDR,
            MR_CLOSE_NETWORK_ADDR,
            MR_SOCKET_ADDR,
            MR_CONNECT_ADDR,
            MR_GET_SOCKET_STATE_ADDR,
            MR_CLOSE_SOCKET_ADDR,
            MR_RECV_ADDR,
            MR_SEND_ADDR,
            MR_RECVFROM_ADDR,
            MR_SENDTO_ADDR,
            MR_START_SHAKE_ADDR,
            MR_STOP_SHAKE_ADDR,
            MR_PLAY_SOUND_ADDR,
            MR_STOP_SOUND_ADDR,
            MR_DIALOG_CREATE_ADDR,
            MR_DIALOG_RELEASE_ADDR,
            MR_DIALOG_REFRESH_ADDR,
            MR_TEXT_CREATE_ADDR,
            MR_TEXT_RELEASE_ADDR,
            MR_TEXT_REFRESH_ADDR,
            MR_EDIT_CREATE_ADDR,
            MR_EDIT_RELEASE_ADDR,
            MR_EDIT_GET_TEXT_ADDR,
            DISP_UP_EX_ADDR,
            DRAW_POINT_ADDR,
            DRAW_BITMAP_ADDR,
            DRAW_BITMAP_EX_ADDR,
            DRAW_RECT_ADDR,
            DRAW_TEXT_ADDR,
            BITMAP_CHECK_ADDR,
            DRAW_TEXT_EX_ADDR,
            MR_WSTRLEN_ADDR,
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
    GetHostByName,
    InitNetwork,
    CloseNetwork,
    Socket,
    Connect,
    GetSocketState,
    CloseSocket,
    Recv,
    Send,
    RecvFrom,
    SendTo,
    StartShake,
    StopShake,
    PlaySound,
    StopSound,
    DialogCreate,
    DialogRelease,
    DialogRefresh,
    TextCreate,
    TextRelease,
    TextRefresh,
    EditCreate,
    EditRelease,
    EditGetText,
    UnsupportedFailed,
}

#[derive(Debug)]
enum HostFile {
    Disk { file: File },
    Package { data: Vec<u8>, cursor: usize },
}

#[derive(Debug)]
struct HostDir {
    entries: Vec<String>,
    cursor: usize,
    scratch_ptr: u32,
}

#[derive(Debug)]
struct HostMenu {
    title: String,
    items: Vec<String>,
    visible: bool,
}

#[derive(Debug)]
enum HostSocket {
    Pending {
        socket_type: i32,
        protocol: i32,
        state: i32,
    },
    Tcp {
        stream: TcpStream,
        state: i32,
    },
    Udp {
        socket: UdpSocket,
        state: i32,
    },
}

impl HostSocket {
    fn state(&self) -> i32 {
        match self {
            Self::Pending { state, .. } | Self::Tcp { state, .. } | Self::Udp { state, .. } => {
                *state
            }
        }
    }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostAppEvent {
    pub code: i32,
    pub p0: u32,
    pub p1: u32,
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
    pending_app_events: VecDeque<HostAppEvent>,
    font_sky16: Option<Vec<u8>>,
    files: BTreeMap<i32, HostFile>,
    next_file_handle: i32,
    dirs: BTreeMap<i32, HostDir>,
    next_dir_handle: i32,
    menus: BTreeMap<i32, HostMenu>,
    next_menu_handle: i32,
    dialogs: BTreeMap<i32, String>,
    next_dialog_handle: i32,
    texts: BTreeMap<i32, String>,
    next_text_handle: i32,
    edits: BTreeMap<i32, String>,
    next_edit_handle: i32,
    windows: BTreeMap<i32, ()>,
    next_window_handle: i32,
    edit_text_ptr: u32,
    network_ready: bool,
    legacy_sockets: BTreeMap<i32, HostSocket>,
    next_socket_handle: i32,
    package_files: BTreeMap<String, Vec<u8>>,
    memory_manager_min_free: u32,
    memory_manager_peak_used: u32,
    active_plugin_ext_load: Option<PluginExtLoad>,
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
            pending_app_events: VecDeque::new(),
            font_sky16: None,
            files: BTreeMap::new(),
            next_file_handle: 3,
            dirs: BTreeMap::new(),
            next_dir_handle: 1000,
            menus: BTreeMap::new(),
            next_menu_handle: 1200,
            dialogs: BTreeMap::new(),
            next_dialog_handle: 1500,
            texts: BTreeMap::new(),
            next_text_handle: 1800,
            edits: BTreeMap::new(),
            next_edit_handle: 2000,
            windows: BTreeMap::new(),
            next_window_handle: 2200,
            edit_text_ptr: 0,
            network_ready: false,
            legacy_sockets: BTreeMap::new(),
            next_socket_handle: 3000,
            package_files: BTreeMap::new(),
            memory_manager_min_free: memory_manager_total_len(),
            memory_manager_peak_used: 0,
            active_plugin_ext_load: None,
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

    pub fn begin_plugin_ext_load(&mut self, code_base: GuestAddr, context_addr: GuestAddr) {
        self.active_plugin_ext_load = Some(PluginExtLoad {
            code_base,
            context_addr,
        });
    }

    pub fn clear_plugin_ext_load(&mut self) {
        self.active_plugin_ext_load = None;
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

    fn package_file_bytes(&self, raw: &str) -> Option<Vec<u8>> {
        let normalized = raw.replace('\\', "/");
        let trimmed_dot = normalized.trim_start_matches("./");
        let trimmed_root = trimmed_dot.trim_start_matches('/');

        for key in [raw, normalized.as_str(), trimmed_dot, trimmed_root] {
            if let Some(bytes) = self.package_files.get(key) {
                return Some(bytes.clone());
            }
        }

        None
    }

    fn open_host_file(&mut self, name: &str, mode: u32) -> i32 {
        let wants_write = mode & (MR_FILE_WRONLY | MR_FILE_RDWR) != 0;
        let wants_create = mode & (MR_FILE_CREATE | MR_FILE_RECREATE) != 0;

        if !wants_write && !wants_create {
            if let Some(data) = self.package_file_bytes(name) {
                let fd = self.next_file_handle;
                self.next_file_handle = self.next_file_handle.saturating_add(1);
                self.files.insert(fd, HostFile::Package { data, cursor: 0 });
                return fd;
            }
        }

        let path = self.resolve_guest_path(name);
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

        match opts.open(path) {
            Ok(file) => {
                let fd = self.next_file_handle;
                self.next_file_handle = self.next_file_handle.saturating_add(1);
                self.files.insert(fd, HostFile::Disk { file });
                fd
            }
            Err(_) => MR_FAILED,
        }
    }

    fn close_host_file(&mut self, fd: i32) -> i32 {
        if self.files.remove(&fd).is_some() {
            MR_SUCCESS
        } else {
            MR_FAILED
        }
    }

    fn read_host_file<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
        fd: i32,
        buffer_ptr: u32,
        len: usize,
    ) -> Result<i32, MemoryAccessError> {
        let Some(file) = self.files.get_mut(&fd) else {
            return Ok(MR_FAILED);
        };

        let mut buffer = vec![0u8; len];
        let ret = match file {
            HostFile::Disk { file } => match file.read(&mut buffer) {
                Ok(read_len) => {
                    for (index, byte) in buffer[..read_len].iter().enumerate() {
                        cpu.memory_mut().write8(
                            GuestAddr::new(buffer_ptr.wrapping_add(index as u32)),
                            *byte,
                        )?;
                    }
                    read_len as i32
                }
                Err(_) => MR_FAILED,
            },
            HostFile::Package { data, cursor } => {
                let available = data.len().saturating_sub(*cursor);
                let read_len = available.min(len);
                for (index, byte) in data[*cursor..(*cursor + read_len)].iter().enumerate() {
                    cpu.memory_mut().write8(
                        GuestAddr::new(buffer_ptr.wrapping_add(index as u32)),
                        *byte,
                    )?;
                }
                *cursor = cursor.saturating_add(read_len);
                read_len as i32
            }
        };

        Ok(ret)
    }

    fn write_host_file<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
        fd: i32,
        buffer_ptr: u32,
        len: usize,
    ) -> Result<i32, MemoryAccessError> {
        let Some(file) = self.files.get_mut(&fd) else {
            return Ok(MR_FAILED);
        };

        let mut buffer = vec![0u8; len];
        for (index, byte) in buffer.iter_mut().enumerate() {
            *byte = cpu
                .memory()
                .read8(GuestAddr::new(buffer_ptr.wrapping_add(index as u32)))?;
        }

        Ok(match file {
            HostFile::Disk { file } => match file.write(&buffer) {
                Ok(write_len) => write_len as i32,
                Err(_) => MR_FAILED,
            },
            HostFile::Package { .. } => MR_FAILED,
        })
    }

    fn seek_host_file(&mut self, fd: i32, pos: i32, method: u32, return_pos: bool) -> i32 {
        let Some(file) = self.files.get_mut(&fd) else {
            return MR_FAILED;
        };

        let Some(from) = (match method {
            MR_SEEK_SET => Some(SeekFrom::Start(pos.max(0) as u64)),
            MR_SEEK_CUR => Some(SeekFrom::Current(pos as i64)),
            MR_SEEK_END => Some(SeekFrom::End(pos as i64)),
            _ => None,
        }) else {
            return MR_FAILED;
        };

        match file {
            HostFile::Disk { file } => match file.seek(from) {
                Ok(new_pos) => {
                    if return_pos {
                        i32::try_from(new_pos).unwrap_or(MR_FAILED)
                    } else {
                        MR_SUCCESS
                    }
                }
                Err(_) => MR_FAILED,
            },
            HostFile::Package { data, cursor } => {
                let base = match from {
                    SeekFrom::Start(offset) => offset as i64,
                    SeekFrom::Current(offset) => (*cursor as i64).saturating_add(offset),
                    SeekFrom::End(offset) => (data.len() as i64).saturating_add(offset),
                };
                if base < 0 {
                    MR_FAILED
                } else {
                    *cursor = usize::try_from(base).unwrap_or(usize::MAX);
                    if return_pos {
                        i32::try_from(*cursor).unwrap_or(MR_FAILED)
                    } else {
                        MR_SUCCESS
                    }
                }
            }
        }
    }

    fn guest_file_info(&self, name: &str) -> i32 {
        if self.package_file_bytes(name).is_some() {
            return MR_IS_FILE;
        }

        let path = self.resolve_guest_path(name);
        match fs::metadata(path) {
            Ok(meta) if meta.is_file() => MR_IS_FILE,
            Ok(meta) if meta.is_dir() => MR_IS_DIR,
            Ok(_) => MR_IS_INVALID,
            Err(_) => MR_IS_INVALID,
        }
    }

    fn guest_file_len(&self, name: &str) -> i32 {
        if let Some(bytes) = self.package_file_bytes(name) {
            return i32::try_from(bytes.len()).unwrap_or(MR_FAILED);
        }

        let path = self.resolve_guest_path(name);
        match fs::metadata(path) {
            Ok(meta) if meta.is_file() => i32::try_from(meta.len()).unwrap_or(MR_FAILED),
            _ => MR_FAILED,
        }
    }

    fn remove_host_file(&self, name: &str) -> i32 {
        let path = self.resolve_guest_path(name);
        if fs::remove_file(path).is_ok() {
            MR_SUCCESS
        } else {
            MR_FAILED
        }
    }

    fn rename_host_file(&self, old_name: &str, new_name: &str) -> i32 {
        let old_path = self.resolve_guest_path(old_name);
        let new_path = self.resolve_guest_path(new_name);
        if fs::rename(old_path, new_path).is_ok() {
            MR_SUCCESS
        } else {
            MR_FAILED
        }
    }

    fn mkdir_host_path(&self, name: &str) -> i32 {
        let path = self.resolve_guest_path(name);
        if fs::create_dir_all(path).is_ok() {
            MR_SUCCESS
        } else {
            MR_FAILED
        }
    }

    fn rmdir_host_path(&self, name: &str) -> i32 {
        let path = self.resolve_guest_path(name);
        if fs::remove_dir(path).is_ok() {
            MR_SUCCESS
        } else {
            MR_FAILED
        }
    }

    fn open_host_dir(&mut self, name: &str) -> i32 {
        let path = self.resolve_guest_path(name);
        match fs::read_dir(path) {
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
        }
    }

    fn fill_dir_entry_buffer<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
        handle: i32,
        buffer_ptr: u32,
        len: usize,
    ) -> Result<i32, MemoryAccessError> {
        if buffer_ptr == 0 || len == 0 {
            return Ok(MR_FAILED);
        }

        let Some(dir) = self.dirs.get_mut(&handle) else {
            return Ok(MR_FAILED);
        };
        if dir.cursor >= dir.entries.len() {
            return Ok(MR_FAILED);
        }

        let name = &dir.entries[dir.cursor];
        dir.cursor += 1;
        let limit = len.saturating_sub(1).min(name.len());
        for (index, byte) in name.as_bytes()[..limit].iter().enumerate() {
            cpu.memory_mut()
                .write8(GuestAddr::new(buffer_ptr.wrapping_add(index as u32)), *byte)?;
        }
        cpu.memory_mut()
            .write8(GuestAddr::new(buffer_ptr.wrapping_add(limit as u32)), 0)?;
        Ok(MR_SUCCESS)
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

    pub fn take_app_event(&mut self) -> Option<HostAppEvent> {
        self.pending_app_events.pop_front()
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
            (0x6C, DsmHostFn::GetHostByName),
            (0x70, DsmHostFn::InitNetwork),
            (0x74, DsmHostFn::CloseNetwork),
            (0x78, DsmHostFn::Socket),
            (0x7C, DsmHostFn::Connect),
            (0x80, DsmHostFn::GetSocketState),
            (0x84, DsmHostFn::CloseSocket),
            (0x88, DsmHostFn::Recv),
            (0x8C, DsmHostFn::Send),
            (0x90, DsmHostFn::RecvFrom),
            (0x94, DsmHostFn::SendTo),
            (0x98, DsmHostFn::StartShake),
            (0x9C, DsmHostFn::StopShake),
            (0xA0, DsmHostFn::PlaySound),
            (0xA4, DsmHostFn::StopSound),
            (0xA8, DsmHostFn::DialogCreate),
            (0xAC, DsmHostFn::DialogRelease),
            (0xB0, DsmHostFn::DialogRefresh),
            (0xB4, DsmHostFn::TextCreate),
            (0xB8, DsmHostFn::TextRelease),
            (0xBC, DsmHostFn::TextRefresh),
            (0xC0, DsmHostFn::EditCreate),
            (0xC4, DsmHostFn::EditRelease),
            (0xC8, DsmHostFn::EditGetText),
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
        if pc == MR_EXT_FUNCTION_NEW_ADDR.get() {
            self.handle_plugin_ext_function_new(cpu)?;
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

        if pc == RET_ZERO_STUB_ADDR.get() {
            cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
            return_to_lr(cpu);
            return Ok(true);
        }

        if pc == MR_MENU_CREATE_ADDR.get() {
            self.handle_menu_create(cpu)?;
            return Ok(true);
        }

        if pc == MR_MENU_SET_ITEM_ADDR.get() {
            self.handle_menu_set_item(cpu)?;
            return Ok(true);
        }

        if pc == MR_MENU_SHOW_ADDR.get() {
            self.handle_menu_show(cpu)?;
            return Ok(true);
        }

        if pc == MR_MENU_RELEASE_ADDR.get() {
            self.handle_menu_release(cpu)?;
            return Ok(true);
        }

        if pc == MR_MENU_REFRESH_ADDR.get() {
            self.handle_menu_refresh(cpu)?;
            return Ok(true);
        }

        if pc == MR_OPEN_ADDR.get() {
            self.handle_legacy_open(cpu)?;
            return Ok(true);
        }

        if pc == MR_CLOSE_ADDR.get() {
            self.handle_legacy_close(cpu)?;
            return Ok(true);
        }

        if pc == MR_INFO_ADDR.get() {
            self.handle_legacy_info(cpu)?;
            return Ok(true);
        }

        if pc == MR_WRITE_ADDR.get() {
            self.handle_legacy_write(cpu)?;
            return Ok(true);
        }

        if pc == MR_READ_ADDR.get() {
            self.handle_legacy_read(cpu)?;
            return Ok(true);
        }

        if pc == MR_SEEK_ADDR.get() {
            self.handle_legacy_seek(cpu)?;
            return Ok(true);
        }

        if pc == MR_GET_LEN_ADDR.get() {
            self.handle_legacy_get_len(cpu)?;
            return Ok(true);
        }

        if pc == MR_REMOVE_ADDR.get() {
            self.handle_legacy_remove(cpu)?;
            return Ok(true);
        }

        if pc == MR_RENAME_ADDR.get() {
            self.handle_legacy_rename(cpu)?;
            return Ok(true);
        }

        if pc == MR_MKDIR_ADDR.get() {
            self.handle_legacy_mkdir(cpu)?;
            return Ok(true);
        }

        if pc == MR_RMDIR_ADDR.get() {
            self.handle_legacy_rmdir(cpu)?;
            return Ok(true);
        }

        if pc == MR_FIND_START_ADDR.get() {
            self.handle_legacy_find_start(cpu)?;
            return Ok(true);
        }

        if pc == MR_FIND_GET_NEXT_ADDR.get() {
            self.handle_legacy_find_get_next(cpu)?;
            return Ok(true);
        }

        if pc == MR_FIND_STOP_ADDR.get() {
            self.handle_legacy_find_stop(cpu)?;
            return Ok(true);
        }

        if pc == MR_EXIT_ADDR.get() {
            self.handle_legacy_exit(cpu)?;
            return Ok(true);
        }

        if pc == MR_SEND_SMS_ADDR.get() {
            self.handle_legacy_send_sms(cpu)?;
            return Ok(true);
        }

        if pc == MR_CALL_ADDR.get() {
            self.handle_legacy_call(cpu)?;
            return Ok(true);
        }

        if pc == MR_GET_NETWORK_ID_ADDR.get() {
            self.handle_legacy_get_network_id(cpu)?;
            return Ok(true);
        }

        if pc == MR_CONNECT_WAP_ADDR.get() {
            self.handle_legacy_connect_wap(cpu)?;
            return Ok(true);
        }

        if pc == MR_GET_HOST_BY_NAME_ADDR.get() {
            self.handle_legacy_get_host_by_name(cpu)?;
            return Ok(true);
        }

        if pc == MR_INIT_NETWORK_ADDR.get() {
            self.handle_legacy_init_network(cpu)?;
            return Ok(true);
        }

        if pc == MR_CLOSE_NETWORK_ADDR.get() {
            self.handle_legacy_close_network(cpu)?;
            return Ok(true);
        }

        if pc == MR_SOCKET_ADDR.get() {
            self.handle_legacy_socket(cpu)?;
            return Ok(true);
        }

        if pc == MR_CONNECT_ADDR.get() {
            self.handle_legacy_connect(cpu)?;
            return Ok(true);
        }

        if pc == MR_GET_SOCKET_STATE_ADDR.get() {
            self.handle_legacy_get_socket_state(cpu)?;
            return Ok(true);
        }

        if pc == MR_CLOSE_SOCKET_ADDR.get() {
            self.handle_legacy_close_socket(cpu)?;
            return Ok(true);
        }

        if pc == MR_RECV_ADDR.get() {
            self.handle_legacy_recv(cpu)?;
            return Ok(true);
        }

        if pc == MR_SEND_ADDR.get() {
            self.handle_legacy_send(cpu)?;
            return Ok(true);
        }

        if pc == MR_RECVFROM_ADDR.get() {
            self.handle_legacy_recvfrom(cpu)?;
            return Ok(true);
        }

        if pc == MR_SENDTO_ADDR.get() {
            self.handle_legacy_sendto(cpu)?;
            return Ok(true);
        }

        if matches!(
            pc,
            value if value == MR_START_SHAKE_ADDR.get()
                || value == MR_STOP_SHAKE_ADDR.get()
                || value == MR_PLAY_SOUND_ADDR.get()
                || value == MR_STOP_SOUND_ADDR.get()
        ) {
            self.handle_legacy_success_stub(cpu)?;
            return Ok(true);
        }

        if pc == MR_DIALOG_CREATE_ADDR.get() {
            self.handle_dialog_create(cpu)?;
            return Ok(true);
        }

        if pc == MR_DIALOG_RELEASE_ADDR.get() {
            self.handle_dialog_release(cpu)?;
            return Ok(true);
        }

        if pc == MR_DIALOG_REFRESH_ADDR.get() {
            self.handle_dialog_refresh(cpu)?;
            return Ok(true);
        }

        if pc == MR_TEXT_CREATE_ADDR.get() {
            self.handle_text_create(cpu)?;
            return Ok(true);
        }

        if pc == MR_TEXT_RELEASE_ADDR.get() {
            self.handle_text_release(cpu)?;
            return Ok(true);
        }

        if pc == MR_TEXT_REFRESH_ADDR.get() {
            self.handle_text_refresh(cpu)?;
            return Ok(true);
        }

        if pc == MR_EDIT_CREATE_ADDR.get() {
            self.handle_edit_create(cpu)?;
            return Ok(true);
        }

        if pc == MR_EDIT_RELEASE_ADDR.get() {
            self.handle_edit_release(cpu)?;
            return Ok(true);
        }

        if pc == MR_EDIT_GET_TEXT_ADDR.get() {
            self.handle_edit_get_text(cpu)?;
            return Ok(true);
        }

        if pc == MR_WIN_CREATE_ADDR.get() {
            self.handle_win_create(cpu)?;
            return Ok(true);
        }

        if pc == MR_WIN_RELEASE_ADDR.get() {
            self.handle_win_release(cpu)?;
            return Ok(true);
        }

        if pc == DISP_UP_EX_ADDR.get() {
            self.handle_legacy_disp_up_ex(cpu)?;
            return Ok(true);
        }

        if pc == DRAW_POINT_ADDR.get() {
            self.handle_legacy_draw_point(cpu)?;
            return Ok(true);
        }

        if pc == DRAW_BITMAP_ADDR.get() {
            self.handle_legacy_draw_bitmap(cpu)?;
            return Ok(true);
        }

        if pc == DRAW_RECT_ADDR.get() {
            self.handle_legacy_draw_rect(cpu)?;
            return Ok(true);
        }

        if pc == DRAW_TEXT_ADDR.get() {
            self.handle_legacy_draw_text(cpu)?;
            return Ok(true);
        }

        if pc == DRAW_TEXT_EX_ADDR.get() {
            self.handle_legacy_draw_text_ex(cpu)?;
            return Ok(true);
        }

        if pc == BITMAP_CHECK_ADDR.get() {
            self.handle_legacy_bitmap_check(cpu)?;
            return Ok(true);
        }

        if pc == DRAW_BITMAP_EX_ADDR.get() {
            self.handle_legacy_draw_bitmap_ex(cpu)?;
            return Ok(true);
        }

        if pc == MR_WSTRLEN_ADDR.get() {
            self.handle_legacy_wstrlen(cpu)?;
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
        let r0 = cpu.regs().reg(0);
        let r1 = cpu.regs().reg(1);
        let r2 = cpu.regs().reg(2);
        let r3 = cpu.regs().reg(3);

        if looks_like_legacy_send_app_event(r1, r3) {
            match r2 {
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
        } else {
            match r0 {
                0 => match r1 {
                    0 => {
                        self.pending_timer_delay_ms = Some(r3);
                        self.pending_timer_command = Some(HostTimerCommand::Start(r3));
                    }
                    1 => {
                        self.pending_timer_delay_ms = None;
                        self.pending_timer_command = Some(HostTimerCommand::Stop);
                    }
                    _ => {}
                },
                1 => {
                    self.pending_app_events.push_back(HostAppEvent {
                        code: r1 as i32,
                        p0: r2,
                        p1: r3,
                    });
                }
                _ => {}
            }
        }

        cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_success_stub<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_open<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let name = read_guest_c_string(cpu, cpu.regs().reg(0), 1024)?;
        let mode = cpu.regs().reg(1);
        let ret = self.open_host_file(&name, mode);
        if self.verbose {
            println!("[legacy-open] name={} mode=0x{:X} ret={}", name, mode, ret);
        }
        cpu.regs_mut().set_reg(0, ret as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_close<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let fd = cpu.regs().reg(0) as i32;
        let ret = self.close_host_file(fd);
        cpu.regs_mut().set_reg(0, ret as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_info<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let name = read_guest_c_string(cpu, cpu.regs().reg(0), 1024)?;
        let ret = self.guest_file_info(&name);
        cpu.regs_mut().set_reg(0, ret as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_write<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let fd = cpu.regs().reg(0) as i32;
        let buffer_ptr = cpu.regs().reg(1);
        let len = cpu.regs().reg(2) as usize;
        let ret = self.write_host_file(cpu, fd, buffer_ptr, len)?;
        cpu.regs_mut().set_reg(0, ret as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_read<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let fd = cpu.regs().reg(0) as i32;
        let buffer_ptr = cpu.regs().reg(1);
        let len = cpu.regs().reg(2) as usize;
        let ret = self.read_host_file(cpu, fd, buffer_ptr, len)?;
        cpu.regs_mut().set_reg(0, ret as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_seek<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let fd = cpu.regs().reg(0) as i32;
        let pos = cpu.regs().reg(1) as i32;
        let method = cpu.regs().reg(2);
        let ret = self.seek_host_file(fd, pos, method, false);
        cpu.regs_mut().set_reg(0, ret as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_get_len<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let name = read_guest_c_string(cpu, cpu.regs().reg(0), 1024)?;
        let ret = self.guest_file_len(&name);
        cpu.regs_mut().set_reg(0, ret as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_remove<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let name = read_guest_c_string(cpu, cpu.regs().reg(0), 1024)?;
        let ret = self.remove_host_file(&name);
        cpu.regs_mut().set_reg(0, ret as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_rename<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let old_name = read_guest_c_string(cpu, cpu.regs().reg(0), 1024)?;
        let new_name = read_guest_c_string(cpu, cpu.regs().reg(1), 1024)?;
        let ret = self.rename_host_file(&old_name, &new_name);
        cpu.regs_mut().set_reg(0, ret as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_mkdir<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let name = read_guest_c_string(cpu, cpu.regs().reg(0), 1024)?;
        let ret = self.mkdir_host_path(&name);
        cpu.regs_mut().set_reg(0, ret as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_rmdir<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let name = read_guest_c_string(cpu, cpu.regs().reg(0), 1024)?;
        let ret = self.rmdir_host_path(&name);
        cpu.regs_mut().set_reg(0, ret as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_find_start<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let name = read_guest_c_string(cpu, cpu.regs().reg(0), 1024)?;
        let buffer_ptr = cpu.regs().reg(1);
        let len = cpu.regs().reg(2) as usize;
        let handle = self.open_host_dir(&name);
        if handle != MR_FAILED {
            let _ = self.fill_dir_entry_buffer(cpu, handle, buffer_ptr, len)?;
        }
        cpu.regs_mut().set_reg(0, handle as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_find_get_next<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let handle = cpu.regs().reg(0) as i32;
        let buffer_ptr = cpu.regs().reg(1);
        let len = cpu.regs().reg(2) as usize;
        let ret = self.fill_dir_entry_buffer(cpu, handle, buffer_ptr, len)?;
        cpu.regs_mut().set_reg(0, ret as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_find_stop<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let handle = cpu.regs().reg(0) as i32;
        let ret = if self.dirs.remove(&handle).is_some() {
            MR_SUCCESS
        } else {
            MR_FAILED
        };
        cpu.regs_mut().set_reg(0, ret as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_disp_up_ex<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let x = cpu.regs().reg(0) as i32;
        let y = cpu.regs().reg(1) as i32;
        let w = cpu.regs().reg(2) as u16;
        let h = cpu.regs().reg(3) as u16;
        self.mark_dirty_region(x, y, w, h);
        cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_draw_point<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let x = cpu.regs().reg(0) as i32;
        let y = cpu.regs().reg(1) as i32;
        let color = cpu.regs().reg(2) as u16;
        self.plot_screen_pixel(x, y, color);
        cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_draw_rect<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let x = cpu.regs().reg(0) as i32;
        let y = cpu.regs().reg(1) as i32;
        let w = cpu.regs().reg(2) as i32;
        let h = cpu.regs().reg(3) as i32;
        let sp = cpu.regs().sp();
        let r = cpu.memory().read32(GuestAddr::new(sp))? as u8;
        let g = cpu.memory().read32(GuestAddr::new(sp.wrapping_add(4)))? as u8;
        let b = cpu.memory().read32(GuestAddr::new(sp.wrapping_add(8)))? as u8;
        self.fill_screen_rect(x, y, w, h, rgb888_to_rgb565(r, g, b));
        cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_draw_bitmap<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let bmp_ptr = cpu.regs().reg(0);
        let x = cpu.regs().reg(1) as i32;
        let y = cpu.regs().reg(2) as i32;
        let w = cpu.regs().reg(3) as u16;
        let h = cpu.memory().read32(GuestAddr::new(cpu.regs().sp()))? as u16;
        self.blit_raw_rgb565_bitmap(cpu.memory(), bmp_ptr, x, y, w, h)?;
        cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_bitmap_check<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let bmp_ptr = cpu.regs().reg(0);
        let x = cpu.regs().reg(1) as i32;
        let y = cpu.regs().reg(2) as i32;
        let w = cpu.regs().reg(3) as u16;
        let sp = cpu.regs().sp();
        let h = cpu.memory().read32(GuestAddr::new(sp))? as u16;
        let transparent = cpu.memory().read32(GuestAddr::new(sp.wrapping_add(4)))? as u16;
        let color_check = cpu.memory().read32(GuestAddr::new(sp.wrapping_add(8)))? as u16;
        let result = self.count_non_matching_bitmap_pixels(
            cpu.memory(),
            bmp_ptr,
            x,
            y,
            w,
            h,
            transparent,
            color_check,
        )?;
        cpu.regs_mut().set_reg(0, result as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_draw_text<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let text_ptr = cpu.regs().reg(0);
        let x = cpu.regs().reg(1) as i32;
        let y = cpu.regs().reg(2) as i32;
        let r = cpu.regs().reg(3) as u8;
        let sp = cpu.regs().sp();
        let g = cpu.memory().read32(GuestAddr::new(sp))? as u8;
        let b = cpu.memory().read32(GuestAddr::new(sp.wrapping_add(4)))? as u8;
        let is_unicode = cpu.memory().read32(GuestAddr::new(sp.wrapping_add(8)))? != 0;
        let _font = cpu.memory().read32(GuestAddr::new(sp.wrapping_add(12)))? as u16;
        let color = rgb888_to_rgb565(r, g, b);
        let chars = read_guest_text_chars(cpu.memory(), text_ptr, is_unicode, 1024)?;
        let mut cursor_x = x;
        let mut total_w = 0u16;

        for ch in chars {
            let width = self.draw_sky16_char(ch, cursor_x, y, color);
            cursor_x += width as i32;
            total_w = total_w.saturating_add(width);
        }

        if total_w > 0 {
            self.mark_dirty_region(x, y, total_w, 16);
        }

        cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_draw_text_ex<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let text_ptr = cpu.regs().reg(0);
        let x = cpu.regs().reg(1) as i32;
        let y = cpu.regs().reg(2) as i32;
        let sp = cpu.regs().sp();
        let rect_w = cpu.memory().read16(GuestAddr::new(sp.wrapping_add(4)))? as i32;
        let rect_h = cpu.memory().read16(GuestAddr::new(sp.wrapping_add(6)))? as i32;
        let r = cpu.memory().read8(GuestAddr::new(sp.wrapping_add(8)))?;
        let g = cpu.memory().read8(GuestAddr::new(sp.wrapping_add(9)))?;
        let b = cpu.memory().read8(GuestAddr::new(sp.wrapping_add(10)))?;
        let flag = cpu.memory().read32(GuestAddr::new(sp.wrapping_add(12)))?;
        let _font = cpu.memory().read32(GuestAddr::new(sp.wrapping_add(16)))? as u16;
        let is_unicode = flag & DRAW_TEXT_EX_IS_UNICODE != 0;
        let auto_newline = flag & DRAW_TEXT_EX_IS_AUTO_NEWLINE != 0;
        let color = rgb888_to_rgb565(r, g, b);
        let chars = read_guest_text_chars(cpu.memory(), text_ptr, is_unicode, 4096)?;
        let clip_right = x.saturating_add(rect_w);
        let clip_bottom = y.saturating_add(rect_h);
        let mut cursor_x = x;
        let mut cursor_y = y;
        let mut line_height = 0i32;
        let mut consumed_bytes = 0u32;
        let mut endchar_index = 0u32;
        let total_bytes = (chars.len() as u32).saturating_mul(2);
        let mut exhausted = true;

        for ch in chars {
            let render_ch = if matches!(ch, 0x0A | 0x0D) { 0x20 } else { ch };
            let width = if render_ch < 128 { 8i32 } else { 16i32 };
            let height = 16i32;

            if auto_newline {
                if cursor_x.saturating_add(width) > clip_right || ch == 0x0A {
                    if cursor_y.saturating_add(line_height) < clip_bottom {
                        endchar_index = consumed_bytes;
                    }
                    cursor_x = x;
                    cursor_y = cursor_y.saturating_add(line_height).saturating_add(2);
                    line_height = 0;
                    if cursor_y > clip_bottom {
                        exhausted = false;
                        break;
                    }
                }
                line_height = line_height.max(height);
            } else {
                if cursor_x > clip_right || ch == 0x0A {
                    exhausted = false;
                    break;
                }
                if cursor_x.saturating_add(width) > clip_right {
                    endchar_index = consumed_bytes;
                }
            }

            if matches!(ch, 0x0A | 0x0D) {
                consumed_bytes = consumed_bytes.saturating_add(2);
                continue;
            }

            self.draw_sky16_char_clipped(
                render_ch,
                cursor_x,
                cursor_y,
                color,
                x,
                y,
                clip_right,
                clip_bottom,
            );
            cursor_x = cursor_x.saturating_add(width);
            consumed_bytes = consumed_bytes.saturating_add(2);
        }

        if exhausted && consumed_bytes == total_bytes {
            if auto_newline {
                if cursor_y.saturating_add(line_height) < clip_bottom {
                    endchar_index = total_bytes;
                }
            } else if cursor_x <= clip_right {
                endchar_index = total_bytes;
            }
        }

        cpu.regs_mut().set_reg(0, endchar_index);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_draw_bitmap_ex<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let src = read_guest_bitmap_draw(cpu.memory(), cpu.regs().reg(0))?;
        let dst = read_guest_bitmap_draw(cpu.memory(), cpu.regs().reg(1))?;
        let w = cpu.regs().reg(2) as u16;
        let h = cpu.regs().reg(3) as u16;
        let sp = cpu.regs().sp();
        let trans = read_guest_trans_matrix(cpu.memory(), cpu.memory().read32(GuestAddr::new(sp))?)?;
        let transcolor = cpu
            .memory()
            .read32(GuestAddr::new(sp.wrapping_add(4)))? as u16;

        if trans.b == 0 && trans.c == 0 && trans.a == 0x0100 && trans.d == 0x0100 {
            let transparent = match trans.rop {
                2 => None,
                6 => Some(transcolor),
                _ => None,
            };
            self.blit_guest_bitmap(cpu.memory(), src, dst, w, h, transparent)?;
        }

        cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_wstrlen<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let len = guest_wstrlen(cpu.memory(), cpu.regs().reg(0), 4096)?;
        cpu.regs_mut().set_reg(0, len);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_menu_create<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let title_addr = cpu.regs().reg(0);
        let item_count = cpu.regs().reg(1) as usize;
        let title = if title_addr == 0 {
            String::new()
        } else {
            read_guest_c_string(cpu, title_addr, 4096)?
        };
        let handle = self.next_menu_handle;
        self.next_menu_handle = self.next_menu_handle.saturating_add(1);
        self.menus.insert(
            handle,
            HostMenu {
                title,
                items: vec![String::new(); item_count],
                visible: false,
            },
        );
        cpu.regs_mut().set_reg(0, handle as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_menu_set_item<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let handle = cpu.regs().reg(0) as i32;
        let text_addr = cpu.regs().reg(1);
        let index = cpu.regs().reg(2) as usize;
        let text = if text_addr == 0 {
            String::new()
        } else {
            read_guest_c_string(cpu, text_addr, 4096)?
        };
        let ret = if let Some(menu) = self.menus.get_mut(&handle) {
            if index >= menu.items.len() {
                menu.items.resize(index + 1, String::new());
            }
            menu.items[index] = text;
            MR_SUCCESS
        } else {
            MR_FAILED
        };
        cpu.regs_mut().set_reg(0, ret as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_menu_show<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let handle = cpu.regs().reg(0) as i32;
        let ret = if let Some(menu) = self.menus.get_mut(&handle) {
            let _ = &menu.title;
            menu.visible = true;
            MR_SUCCESS
        } else {
            MR_FAILED
        };
        cpu.regs_mut().set_reg(0, ret as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_menu_release<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let handle = cpu.regs().reg(0) as i32;
        let ret = if self.menus.remove(&handle).is_some() {
            MR_SUCCESS
        } else {
            MR_FAILED
        };
        cpu.regs_mut().set_reg(0, ret as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_menu_refresh<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let handle = cpu.regs().reg(0) as i32;
        let ret = if self.menus.contains_key(&handle) {
            MR_SUCCESS
        } else {
            MR_FAILED
        };
        cpu.regs_mut().set_reg(0, ret as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_edit_create<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let text_addr = cpu.regs().reg(1);
        let text = if text_addr == 0 {
            String::new()
        } else {
            read_guest_c_string(cpu, text_addr, 4096)?
        };
        let handle = self.next_edit_handle;
        self.next_edit_handle = self.next_edit_handle.saturating_add(1);
        self.edits.insert(handle, text);
        cpu.regs_mut().set_reg(0, handle as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_edit_release<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let handle = cpu.regs().reg(0) as i32;
        self.edits.remove(&handle);
        cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_edit_get_text<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let handle = cpu.regs().reg(0) as i32;
        let text = self.edits.get(&handle).cloned().unwrap_or_default();
        if self.edit_text_ptr != 0 {
            self.free_ext(self.edit_text_ptr);
            self.edit_text_ptr = 0;
        }

        let ptr = self
            .alloc_ext(cpu.memory_mut(), text.len().max(1) as u32 + 1)?
            .unwrap_or(0);
        if ptr != 0 {
            write_guest_c_string(cpu, ptr, &text)?;
        }
        self.edit_text_ptr = ptr;
        self.sync_memory_manager_cells(cpu.memory_mut())?;
        cpu.regs_mut().set_reg(0, ptr);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_dialog_create<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let text = self.read_optional_legacy_text(cpu)?;
        let handle = self.next_dialog_handle;
        self.next_dialog_handle = self.next_dialog_handle.saturating_add(1);
        self.dialogs.insert(handle, text);
        cpu.regs_mut().set_reg(0, handle as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_dialog_release<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let handle = cpu.regs().reg(0) as i32;
        self.dialogs.remove(&handle);
        cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_dialog_refresh<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_text_create<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let text = self.read_optional_legacy_text(cpu)?;
        let handle = self.next_text_handle;
        self.next_text_handle = self.next_text_handle.saturating_add(1);
        self.texts.insert(handle, text);
        cpu.regs_mut().set_reg(0, handle as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_text_release<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let handle = cpu.regs().reg(0) as i32;
        self.texts.remove(&handle);
        cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_text_refresh<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_win_create<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let handle = self.next_window_handle;
        self.next_window_handle = self.next_window_handle.saturating_add(1);
        self.windows.insert(handle, ());
        cpu.regs_mut().set_reg(0, handle as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_win_release<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let handle = cpu.regs().reg(0) as i32;
        let ret = if self.windows.remove(&handle).is_some() {
            MR_SUCCESS
        } else {
            MR_FAILED
        };
        cpu.regs_mut().set_reg(0, ret as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn read_optional_legacy_text<B: MemoryBus>(
        &self,
        cpu: &Cpu<B>,
    ) -> Result<String, MemoryAccessError> {
        for addr in [cpu.regs().reg(1), cpu.regs().reg(0)] {
            if addr != 0 {
                return read_guest_c_string(cpu, addr, 4096);
            }
        }
        Ok(String::new())
    }

    fn handle_legacy_get_host_by_name<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let name_addr = cpu.regs().reg(0);
        let ret = if name_addr == 0 {
            MR_FAILED
        } else {
            let name = read_guest_c_string(cpu, name_addr, 1024)?;
            if name.trim().is_empty() {
                MR_FAILED
            } else {
                0x7F00_0001u32 as i32
            }
        };
        cpu.regs_mut().set_reg(0, ret as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_exit<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        self.exit_requested = true;
        cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_send_sms<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_call<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_get_network_id<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        cpu.regs_mut().set_reg(0, MR_NET_ID_MOBILE as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_connect_wap<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        self.network_ready = true;
        cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_init_network<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        self.network_ready = true;
        cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_close_network<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        self.network_ready = false;
        self.legacy_sockets.clear();
        cpu.regs_mut().set_reg(0, MR_SUCCESS as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_socket<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        if !self.network_ready {
            cpu.regs_mut().set_reg(0, MR_FAILED as u32);
        } else {
            let handle = self.next_socket_handle;
            self.next_socket_handle = self.next_socket_handle.saturating_add(1);
            self.legacy_sockets.insert(
                handle,
                HostSocket::Pending {
                    socket_type: cpu.regs().reg(0) as i32,
                    protocol: cpu.regs().reg(1) as i32,
                    state: MR_SUCCESS,
                },
            );
            cpu.regs_mut().set_reg(0, handle as u32);
        }
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_connect<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let handle = cpu.regs().reg(0) as i32;
        let ip = cpu.regs().reg(1);
        let port = cpu.regs().reg(2) as u16;
        let ret = if !self.network_ready {
            MR_FAILED
        } else if let Some(socket) = self.legacy_sockets.remove(&handle) {
            let addr = SocketAddrV4::new(Ipv4Addr::from(ip), port);
            let (ret, next_socket) = match socket {
                HostSocket::Pending {
                    socket_type,
                    protocol,
                    ..
                } if socket_type == 0 && protocol == 0 => {
                    match TcpStream::connect_timeout(&addr.into(), Duration::from_millis(500)) {
                        Ok(stream) => {
                            let _ = stream.set_nodelay(true);
                            let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                            let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
                            (
                                MR_SUCCESS,
                                Some(HostSocket::Tcp {
                                    stream,
                                    state: MR_SUCCESS,
                                }),
                            )
                        }
                        Err(_) => (
                            MR_FAILED,
                            Some(HostSocket::Pending {
                                socket_type,
                                protocol,
                                state: MR_FAILED,
                            }),
                        ),
                    }
                }
                HostSocket::Pending {
                    socket_type,
                    protocol,
                    ..
                } if socket_type == 1 && protocol == 1 => match UdpSocket::bind(("0.0.0.0", 0)) {
                    Ok(udp) => {
                        let _ = udp.connect(addr);
                        let _ = udp.set_read_timeout(Some(Duration::from_millis(500)));
                        let _ = udp.set_write_timeout(Some(Duration::from_millis(500)));
                        (
                            MR_SUCCESS,
                            Some(HostSocket::Udp {
                                socket: udp,
                                state: MR_SUCCESS,
                            }),
                        )
                    }
                    Err(_) => (
                        MR_FAILED,
                        Some(HostSocket::Pending {
                            socket_type,
                            protocol,
                            state: MR_FAILED,
                        }),
                    ),
                },
                other => (other.state(), Some(other)),
            };
            if let Some(next_socket) = next_socket {
                self.legacy_sockets.insert(handle, next_socket);
            }
            ret
        } else {
            MR_FAILED
        };
        cpu.regs_mut().set_reg(0, ret as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_get_socket_state<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let handle = cpu.regs().reg(0) as i32;
        let ret = if self.network_ready {
            self.legacy_sockets
                .get(&handle)
                .map(HostSocket::state)
                .unwrap_or(MR_FAILED)
        } else {
            MR_FAILED
        };
        cpu.regs_mut().set_reg(0, ret as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_close_socket<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let handle = cpu.regs().reg(0) as i32;
        let ret = if let Some(socket) = self.legacy_sockets.remove(&handle) {
            if let HostSocket::Tcp { stream, .. } = socket {
                let _ = stream.shutdown(Shutdown::Both);
            }
            MR_SUCCESS
        } else {
            MR_FAILED
        };
        cpu.regs_mut().set_reg(0, ret as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_recv<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let handle = cpu.regs().reg(0) as i32;
        let buffer_ptr = cpu.regs().reg(1);
        let len = cpu.regs().reg(2) as usize;
        let mut read_len = MR_FAILED;
        let mut buffer = vec![0u8; len];

        if self.network_ready {
            if let Some(socket) = self.legacy_sockets.get_mut(&handle) {
                read_len = match socket {
                    HostSocket::Tcp { stream, state } => match stream.read(&mut buffer) {
                        Ok(size) => {
                            *state = MR_SUCCESS;
                            size as i32
                        }
                        Err(err)
                            if matches!(
                                err.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                            ) =>
                        {
                            *state = MR_SUCCESS;
                            0
                        }
                        Err(_) => {
                            *state = MR_FAILED;
                            MR_FAILED
                        }
                    },
                    HostSocket::Udp { socket, state } => match socket.recv(&mut buffer) {
                        Ok(size) => {
                            *state = MR_SUCCESS;
                            size as i32
                        }
                        Err(err)
                            if matches!(
                                err.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                            ) =>
                        {
                            *state = MR_SUCCESS;
                            0
                        }
                        Err(_) => {
                            *state = MR_FAILED;
                            MR_FAILED
                        }
                    },
                    HostSocket::Pending { state, .. } => {
                        *state = MR_FAILED;
                        MR_FAILED
                    }
                };
            }
        }

        if read_len > 0 {
            for (index, byte) in buffer.into_iter().take(read_len as usize).enumerate() {
                if cpu
                    .memory_mut()
                    .write8(GuestAddr::new(buffer_ptr.wrapping_add(index as u32)), byte)
                    .is_err()
                {
                    cpu.regs_mut().set_reg(0, MR_FAILED as u32);
                    return_to_lr(cpu);
                    return Ok(());
                }
            }
        }
        cpu.regs_mut().set_reg(0, read_len as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_send<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let handle = cpu.regs().reg(0) as i32;
        let buffer_ptr = cpu.regs().reg(1);
        let len = cpu.regs().reg(2) as usize;
        let mut buffer = vec![0u8; len];
        for (index, byte) in buffer.iter_mut().enumerate() {
            *byte = match cpu
                .memory()
                .read8(GuestAddr::new(buffer_ptr.wrapping_add(index as u32)))
            {
                Ok(value) => value,
                Err(_) => {
                    cpu.regs_mut().set_reg(0, MR_FAILED as u32);
                    return_to_lr(cpu);
                    return Ok(());
                }
            };
        }

        let ret = if self.network_ready {
            if let Some(socket) = self.legacy_sockets.get_mut(&handle) {
                match socket {
                    HostSocket::Tcp { stream, state } => match stream.write(&buffer) {
                        Ok(size) => {
                            *state = MR_SUCCESS;
                            size as i32
                        }
                        Err(_) => {
                            *state = MR_FAILED;
                            MR_FAILED
                        }
                    },
                    HostSocket::Udp { socket, state } => match socket.send(&buffer) {
                        Ok(size) => {
                            *state = MR_SUCCESS;
                            size as i32
                        }
                        Err(_) => {
                            *state = MR_FAILED;
                            MR_FAILED
                        }
                    },
                    HostSocket::Pending { state, .. } => {
                        *state = MR_FAILED;
                        MR_FAILED
                    }
                }
            } else {
                MR_FAILED
            }
        } else {
            MR_FAILED
        };
        cpu.regs_mut().set_reg(0, ret as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_recvfrom<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let handle = cpu.regs().reg(0) as i32;
        let buffer_ptr = cpu.regs().reg(1);
        let len = cpu.regs().reg(2) as usize;
        let ip_ptr = cpu.regs().reg(3);
        let port_ptr = match cpu.memory().read32(GuestAddr::new(cpu.regs().sp())) {
            Ok(value) => value,
            Err(_) => {
                cpu.regs_mut().set_reg(0, MR_FAILED as u32);
                return_to_lr(cpu);
                return Ok(());
            }
        };
        let mut buffer = vec![0u8; len];

        let (ret, source_addr, next_socket) = if !self.network_ready {
            (MR_FAILED, None, None)
        } else if let Some(socket) = self.legacy_sockets.remove(&handle) {
            match socket {
                HostSocket::Pending {
                    socket_type,
                    protocol,
                    ..
                } if socket_type == 1 && protocol == 1 => match UdpSocket::bind(("0.0.0.0", 0)) {
                    Ok(udp) => {
                        let _ = udp.set_read_timeout(Some(Duration::from_millis(500)));
                        let _ = udp.set_write_timeout(Some(Duration::from_millis(500)));
                        match udp.recv_from(&mut buffer) {
                            Ok((size, from)) => (
                                size as i32,
                                Some(from),
                                Some(HostSocket::Udp {
                                    socket: udp,
                                    state: MR_SUCCESS,
                                }),
                            ),
                            Err(err)
                                if matches!(
                                    err.kind(),
                                    std::io::ErrorKind::WouldBlock
                                        | std::io::ErrorKind::TimedOut
                                ) =>
                            {
                                (
                                    0,
                                    None,
                                    Some(HostSocket::Udp {
                                        socket: udp,
                                        state: MR_SUCCESS,
                                    }),
                                )
                            }
                            Err(_) => (
                                MR_FAILED,
                                None,
                                Some(HostSocket::Udp {
                                    socket: udp,
                                    state: MR_FAILED,
                                }),
                            ),
                        }
                    }
                    Err(_) => (
                        MR_FAILED,
                        None,
                        Some(HostSocket::Pending {
                            socket_type,
                            protocol,
                            state: MR_FAILED,
                        }),
                    ),
                },
                HostSocket::Udp { socket, .. } => match socket.recv_from(&mut buffer) {
                    Ok((size, from)) => (
                        size as i32,
                        Some(from),
                        Some(HostSocket::Udp {
                            socket,
                            state: MR_SUCCESS,
                        }),
                    ),
                    Err(err)
                        if matches!(
                            err.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) =>
                    {
                        (
                            0,
                            None,
                            Some(HostSocket::Udp {
                                socket,
                                state: MR_SUCCESS,
                            }),
                        )
                    }
                    Err(_) => (
                        MR_FAILED,
                        None,
                        Some(HostSocket::Udp {
                            socket,
                            state: MR_FAILED,
                        }),
                    ),
                },
                HostSocket::Pending {
                    socket_type,
                    protocol,
                    ..
                } => (
                    MR_FAILED,
                    None,
                    Some(HostSocket::Pending {
                        socket_type,
                        protocol,
                        state: MR_FAILED,
                    }),
                ),
                other => (MR_FAILED, None, Some(other)),
            }
        } else {
            (MR_FAILED, None, None)
        };

        if let Some(next_socket) = next_socket {
            self.legacy_sockets.insert(handle, next_socket);
        }
        if ret > 0 {
            for (index, byte) in buffer.into_iter().take(ret as usize).enumerate() {
                if cpu
                    .memory_mut()
                    .write8(GuestAddr::new(buffer_ptr.wrapping_add(index as u32)), byte)
                    .is_err()
                {
                    cpu.regs_mut().set_reg(0, MR_FAILED as u32);
                    return_to_lr(cpu);
                    return Ok(());
                }
            }
            if let Some(from) = source_addr {
                if ip_ptr != 0 {
                    let ip = match from.ip() {
                        std::net::IpAddr::V4(ipv4) => u32::from(ipv4),
                        std::net::IpAddr::V6(_) => 0,
                    };
                    if cpu.memory_mut().write32(GuestAddr::new(ip_ptr), ip).is_err() {
                        cpu.regs_mut().set_reg(0, MR_FAILED as u32);
                        return_to_lr(cpu);
                        return Ok(());
                    }
                }
                if port_ptr != 0 {
                    if cpu
                        .memory_mut()
                        .write16(GuestAddr::new(port_ptr), from.port())
                        .is_err()
                    {
                        cpu.regs_mut().set_reg(0, MR_FAILED as u32);
                        return_to_lr(cpu);
                        return Ok(());
                    }
                }
            }
        };
        cpu.regs_mut().set_reg(0, ret as u32);
        return_to_lr(cpu);
        Ok(())
    }

    fn handle_legacy_sendto<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let handle = cpu.regs().reg(0) as i32;
        let buffer_ptr = cpu.regs().reg(1);
        let len = cpu.regs().reg(2) as usize;
        let ip = cpu.regs().reg(3);
        let port = match cpu.memory().read32(GuestAddr::new(cpu.regs().sp())) {
            Ok(value) => value as u16,
            Err(_) => {
                cpu.regs_mut().set_reg(0, MR_FAILED as u32);
                return_to_lr(cpu);
                return Ok(());
            }
        };
        let mut buffer = vec![0u8; len];
        for (index, byte) in buffer.iter_mut().enumerate() {
            *byte = match cpu
                .memory()
                .read8(GuestAddr::new(buffer_ptr.wrapping_add(index as u32)))
            {
                Ok(value) => value,
                Err(_) => {
                    cpu.regs_mut().set_reg(0, MR_FAILED as u32);
                    return_to_lr(cpu);
                    return Ok(());
                }
            };
        }

        let ret = if !self.network_ready {
            MR_FAILED
        } else if let Some(socket) = self.legacy_sockets.remove(&handle) {
            let addr = SocketAddrV4::new(Ipv4Addr::from(ip), port);
            let (ret, next_socket) = match socket {
                HostSocket::Pending {
                    socket_type,
                    protocol,
                    ..
                } if socket_type == 1 && protocol == 1 => match UdpSocket::bind(("0.0.0.0", 0)) {
                    Ok(udp) => {
                        let _ = udp.set_read_timeout(Some(Duration::from_millis(500)));
                        let _ = udp.set_write_timeout(Some(Duration::from_millis(500)));
                        match udp.send_to(&buffer, addr) {
                            Ok(size) => (
                                size as i32,
                                Some(HostSocket::Udp {
                                    socket: udp,
                                    state: MR_SUCCESS,
                                }),
                            ),
                            Err(err)
                                if matches!(
                                    err.kind(),
                                    std::io::ErrorKind::WouldBlock
                                        | std::io::ErrorKind::TimedOut
                                ) =>
                            {
                                (
                                    0,
                                    Some(HostSocket::Udp {
                                        socket: udp,
                                        state: MR_SUCCESS,
                                    }),
                                )
                            }
                            Err(_) => (
                                MR_FAILED,
                                Some(HostSocket::Udp {
                                    socket: udp,
                                    state: MR_FAILED,
                                }),
                            ),
                        }
                    }
                    Err(_) => (
                        MR_FAILED,
                        Some(HostSocket::Pending {
                            socket_type,
                            protocol,
                            state: MR_FAILED,
                        }),
                    ),
                },
                HostSocket::Udp { socket, .. } => match socket.send_to(&buffer, addr) {
                    Ok(size) => (
                        size as i32,
                        Some(HostSocket::Udp {
                            socket,
                            state: MR_SUCCESS,
                        }),
                    ),
                    Err(err)
                        if matches!(
                            err.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) =>
                    {
                        (
                            0,
                            Some(HostSocket::Udp {
                                socket,
                                state: MR_SUCCESS,
                            }),
                        )
                    }
                    Err(_) => (
                        MR_FAILED,
                        Some(HostSocket::Udp {
                            socket,
                            state: MR_FAILED,
                        }),
                    ),
                },
                HostSocket::Pending {
                    socket_type,
                    protocol,
                    ..
                } => (
                    MR_FAILED,
                    Some(HostSocket::Pending {
                        socket_type,
                        protocol,
                        state: MR_FAILED,
                    }),
                ),
                other => (MR_FAILED, Some(other)),
            };
            if let Some(next_socket) = next_socket {
                self.legacy_sockets.insert(handle, next_socket);
            }
            ret
        } else {
            MR_FAILED
        };
        cpu.regs_mut().set_reg(0, ret as u32);
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

    fn handle_plugin_ext_function_new<B: MemoryBus>(
        &mut self,
        cpu: &mut Cpu<B>,
    ) -> Result<(), MemoryAccessError> {
        let Some(load) = self.active_plugin_ext_load else {
            cpu.regs_mut().set_reg(0, u32::MAX);
            return_to_lr(cpu);
            return Ok(());
        };
        let helper = cpu.regs().reg(0);
        if helper != 0 {
            self.ext_helper_addr = Some(GuestAddr::new(helper));
        }
        let context_addr = if load.context_addr.get() != 0 {
            load.context_addr.get()
        } else {
            let len = cpu.regs().reg(1).max(1);
            let Some(context_addr) = self.alloc_ext(cpu.memory_mut(), len)? else {
                cpu.regs_mut().set_reg(0, u32::MAX);
                return_to_lr(cpu);
                return Ok(());
            };
            for offset in 0..len {
                cpu.memory_mut()
                    .write8(GuestAddr::new(context_addr.wrapping_add(offset)), 0)?;
            }
            context_addr
        };
        cpu.memory_mut().write32(
            GuestAddr::new(load.code_base.get().wrapping_add(4)),
            context_addr,
        )?;
        self.mr_c_function_p_addr = GuestAddr::new(context_addr);
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
        if self.verbose {
            println!("[host-testcom] code={} input1=0x{:X}", input0, input1);
        }
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
            306 => {
                cpu.memory_mut().write32(
                    legacy_runtime_addr(self.mr_table_addr, LEGACY_SMS_RETURN_FLAG_CELL_OFFSET),
                    1,
                )?;
                cpu.memory_mut().write32(
                    legacy_runtime_addr(self.mr_table_addr, LEGACY_SMS_RETURN_VAL_CELL_OFFSET),
                    input1,
                )?;
                0
            }
            307 => {
                cpu.memory_mut().write32(
                    legacy_runtime_addr(self.mr_table_addr, LEGACY_SMS_RETURN_FLAG_CELL_OFFSET),
                    0,
                )?;
                0
            }
            400 => {
                if input1 > 0 {
                    thread::sleep(Duration::from_millis(input1 as u64));
                }
                0
            }
            401 => {
                let cell = legacy_runtime_addr(self.mr_table_addr, LEGACY_SCREEN_W_CELL_OFFSET);
                let current = cpu.memory().read32(cell)?;
                cpu.memory_mut().write32(cell, input1)?;
                current
            }
            405 => {
                self.network_ready = false;
                self.legacy_sockets.clear();
                MR_SUCCESS as u32
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
            406 => {
                let cell = legacy_runtime_addr(self.mr_table_addr, LEGACY_SCREEN_H_CELL_OFFSET);
                let current = cpu.memory().read32(cell)?;
                cpu.memory_mut().write32(cell, input1)?;
                current
            }
            407 => {
                let cell =
                    internal_data_cell_addr(self.mr_table_addr, MR_TIMER_RUN_WITHOUT_PAUSE_CELL_OFFSET);
                cpu.memory_mut().write32(cell, input1)?;
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
        if self.verbose {
            println!(
                "[host-testcom1] code={} input1=0x{:X} len=0x{:X}",
                input0,
                cpu.regs().reg(2),
                cpu.regs().reg(3)
            );
        }
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
        if self.verbose {
            println!(
                "[mr-read-file] name={} ptr=0x{:X} len_ptr=0x{:X}",
                name, ptr, filelen_ptr
            );
        }
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
                let wants_write = mode & (MR_FILE_WRONLY | MR_FILE_RDWR) != 0;
                let wants_create = mode & (MR_FILE_CREATE | MR_FILE_RECREATE) != 0;

                let ret = if !wants_write && !wants_create {
                    if let Some(data) = self.package_file_bytes(&name) {
                        let fd = self.next_file_handle;
                        self.next_file_handle = self.next_file_handle.saturating_add(1);
                        self.files.insert(fd, HostFile::Package { data, cursor: 0 });
                        fd
                    } else {
                        let path = self.resolve_guest_path(&name);
                        let mut opts = OpenOptions::new();
                        opts.read(true);
                        match opts.open(path) {
                            Ok(file) => {
                                let fd = self.next_file_handle;
                                self.next_file_handle = self.next_file_handle.saturating_add(1);
                                self.files.insert(fd, HostFile::Disk { file });
                                fd
                            }
                            Err(_) => MR_FAILED,
                        }
                    }
                } else {
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

                    match opts.open(path) {
                        Ok(file) => {
                            let fd = self.next_file_handle;
                            self.next_file_handle = self.next_file_handle.saturating_add(1);
                            self.files.insert(fd, HostFile::Disk { file });
                            fd
                        }
                        Err(_) => MR_FAILED,
                    }
                };
                if self.verbose {
                    println!(
                        "[dsm-open] name={} mode=0x{:X} ret={}",
                        name, mode, ret
                    );
                }
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
                if self.verbose {
                    println!("[dsm-close] fd={} ret={}", fd, ret);
                }
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
                    match file {
                        HostFile::Disk { file } => match file.read(&mut buffer) {
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
                        },
                        HostFile::Package { data, cursor } => {
                            let available = data.len().saturating_sub(*cursor);
                            let read_len = available.min(len);
                            for (index, byte) in data[*cursor..(*cursor + read_len)].iter().enumerate()
                            {
                                cpu.memory_mut().write8(
                                    GuestAddr::new(buffer_ptr.wrapping_add(index as u32)),
                                    *byte,
                                )?;
                            }
                            *cursor = cursor.saturating_add(read_len);
                            ret = read_len as i32;
                        }
                    }
                }

                if self.verbose {
                    println!("[dsm-read] fd={} len={} ret={}", fd, len, ret);
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
                    ret = match file {
                        HostFile::Disk { file } => match file.write(&buffer) {
                            Ok(write_len) => write_len as i32,
                            Err(_) => MR_FAILED,
                        },
                        HostFile::Package { .. } => MR_FAILED,
                    };
                }

                cpu.regs_mut().set_reg(0, ret as u32);
                return_to_lr(cpu);
            }
            DsmHostFn::Seek => {
                let fd = cpu.regs().reg(0) as i32;
                let pos = cpu.regs().reg(1) as i32;
                let method = cpu.regs().reg(2);
                let ret = self.seek_host_file(fd, pos, method, true);

                if self.verbose {
                    println!(
                        "[dsm-seek] fd={} pos={} method={} ret={}",
                        fd, pos, method, ret
                    );
                }
                cpu.regs_mut().set_reg(0, ret as u32);
                return_to_lr(cpu);
            }
            DsmHostFn::Info => {
                let name_addr = cpu.regs().reg(0);
                let name = read_guest_c_string(cpu, name_addr, 1024)?;
                let ret = if self.package_file_bytes(&name).is_some() {
                    MR_IS_FILE
                } else {
                    let path = self.resolve_guest_path(&name);
                    match fs::metadata(path) {
                        Ok(meta) if meta.is_file() => MR_IS_FILE,
                        Ok(meta) if meta.is_dir() => MR_IS_DIR,
                        Ok(_) => MR_IS_INVALID,
                        Err(_) => MR_IS_INVALID,
                    }
                };
                if self.verbose {
                    println!("[dsm-info] name={} ret={}", name, ret);
                }
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
                let ret = if let Some(bytes) = self.package_file_bytes(&name) {
                    i32::try_from(bytes.len()).unwrap_or(MR_FAILED)
                } else {
                    let path = self.resolve_guest_path(&name);
                    match fs::metadata(path) {
                        Ok(meta) if meta.is_file() => i32::try_from(meta.len()).unwrap_or(MR_FAILED),
                        _ => MR_FAILED,
                    }
                };
                if self.verbose {
                    println!("[dsm-getlen] name={} ret={}", name, ret);
                }
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
            DsmHostFn::GetHostByName => {
                self.handle_legacy_get_host_by_name(cpu)?;
            }
            DsmHostFn::InitNetwork => {
                self.handle_legacy_init_network(cpu)?;
            }
            DsmHostFn::CloseNetwork => {
                self.handle_legacy_close_network(cpu)?;
            }
            DsmHostFn::Socket => {
                self.handle_legacy_socket(cpu)?;
            }
            DsmHostFn::Connect => {
                self.handle_legacy_connect(cpu)?;
            }
            DsmHostFn::GetSocketState => {
                self.handle_legacy_get_socket_state(cpu)?;
            }
            DsmHostFn::CloseSocket => {
                self.handle_legacy_close_socket(cpu)?;
            }
            DsmHostFn::Recv => {
                self.handle_legacy_recv(cpu)?;
            }
            DsmHostFn::Send => {
                self.handle_legacy_send(cpu)?;
            }
            DsmHostFn::RecvFrom => {
                self.handle_legacy_recvfrom(cpu)?;
            }
            DsmHostFn::SendTo => {
                self.handle_legacy_sendto(cpu)?;
            }
            DsmHostFn::StartShake
            | DsmHostFn::StopShake
            | DsmHostFn::PlaySound
            | DsmHostFn::StopSound => {
                self.handle_legacy_success_stub(cpu)?;
            }
            DsmHostFn::DialogCreate => {
                self.handle_dialog_create(cpu)?;
            }
            DsmHostFn::DialogRelease => {
                self.handle_dialog_release(cpu)?;
            }
            DsmHostFn::DialogRefresh => {
                self.handle_dialog_refresh(cpu)?;
            }
            DsmHostFn::TextCreate => {
                self.handle_text_create(cpu)?;
            }
            DsmHostFn::TextRelease => {
                self.handle_text_release(cpu)?;
            }
            DsmHostFn::TextRefresh => {
                self.handle_text_refresh(cpu)?;
            }
            DsmHostFn::EditCreate => {
                self.handle_edit_create(cpu)?;
            }
            DsmHostFn::EditRelease => {
                self.handle_edit_release(cpu)?;
            }
            DsmHostFn::EditGetText => {
                self.handle_edit_get_text(cpu)?;
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

    fn plot_screen_pixel(&mut self, x: i32, y: i32, color: u16) {
        if x < 0 || y < 0 || x >= SCREEN_WIDTH as i32 || y >= SCREEN_HEIGHT as i32 {
            return;
        }
        let index = y as usize * SCREEN_WIDTH + x as usize;
        self.screen_buffer[index] = color;
        self.mark_dirty_region(x, y, 1, 1);
    }

    fn fill_screen_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u16) {
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + w).min(SCREEN_WIDTH as i32);
        let y1 = (y + h).min(SCREEN_HEIGHT as i32);
        if x0 >= x1 || y0 >= y1 {
            return;
        }

        for yy in y0..y1 {
            let row_start = yy as usize * SCREEN_WIDTH;
            for xx in x0..x1 {
                self.screen_buffer[row_start + xx as usize] = color;
            }
        }

        self.mark_dirty_region(x0, y0, (x1 - x0) as u16, (y1 - y0) as u16);
    }

    fn mark_dirty_region(&mut self, x: i32, y: i32, w: u16, h: u16) {
        let x0 = x.max(0).min(SCREEN_WIDTH as i32);
        let y0 = y.max(0).min(SCREEN_HEIGHT as i32);
        let x1 = (x.saturating_add(w as i32)).max(0).min(SCREEN_WIDTH as i32);
        let y1 = (y.saturating_add(h as i32)).max(0).min(SCREEN_HEIGHT as i32);
        if x0 >= x1 || y0 >= y1 {
            return;
        }

        let next = HostScreenRegion {
            x: x0,
            y: y0,
            w: (x1 - x0) as u16,
            h: (y1 - y0) as u16,
        };
        self.dirty_region = Some(match self.dirty_region.take() {
            Some(current) => {
                let left = current.x.min(next.x);
                let top = current.y.min(next.y);
                let right = (current.x + current.w as i32).max(next.x + next.w as i32);
                let bottom = (current.y + current.h as i32).max(next.y + next.h as i32);
                HostScreenRegion {
                    x: left,
                    y: top,
                    w: (right - left) as u16,
                    h: (bottom - top) as u16,
                }
            }
            None => next,
        });
    }

    fn blit_guest_bitmap<B: MemoryBus>(
        &mut self,
        memory: &B,
        src: GuestBitmapDraw,
        dst: GuestBitmapDraw,
        w: u16,
        h: u16,
        transparent: Option<u16>,
    ) -> Result<(), MemoryAccessError> {
        for j in 0..h as i32 {
            let sy = src.y as i32 + j;
            let dy = dst.y as i32 + j;
            if sy < 0 || sy >= src.h as i32 || dy < 0 || dy >= SCREEN_HEIGHT as i32 {
                continue;
            }
            for i in 0..w as i32 {
                let sx = src.x as i32 + i;
                let dx = dst.x as i32 + i;
                if sx < 0 || sx >= src.w as i32 || dx < 0 || dx >= SCREEN_WIDTH as i32 {
                    continue;
                }

                let src_index = (sy as u32)
                    .wrapping_mul(src.w as u32)
                    .wrapping_add(sx as u32);
                let pixel = memory.read16(GuestAddr::new(
                    src.p.wrapping_add(src_index.wrapping_mul(2)),
                ))?;
                if transparent.is_some_and(|color| color == pixel) {
                    continue;
                }

                let dst_index = dy as usize * SCREEN_WIDTH + dx as usize;
                self.screen_buffer[dst_index] = pixel;
            }
        }

        self.mark_dirty_region(dst.x as i32, dst.y as i32, w, h);
        Ok(())
    }

    fn blit_raw_rgb565_bitmap<B: MemoryBus>(
        &mut self,
        memory: &B,
        bmp_ptr: u32,
        x: i32,
        y: i32,
        w: u16,
        h: u16,
    ) -> Result<(), MemoryAccessError> {
        for j in 0..h as i32 {
            for i in 0..w as i32 {
                let xx = x + i;
                let yy = y + j;
                if xx < 0 || yy < 0 || xx >= SCREEN_WIDTH as i32 || yy >= SCREEN_HEIGHT as i32 {
                    continue;
                }

                let src_index = i as u32 + j as u32 * w as u32;
                let pixel =
                    memory.read16(GuestAddr::new(bmp_ptr.wrapping_add(src_index.wrapping_mul(2))))?;
                let dst_index = yy as usize * SCREEN_WIDTH + xx as usize;
                self.screen_buffer[dst_index] = pixel;
            }
        }

        self.mark_dirty_region(x, y, w, h);
        Ok(())
    }

    fn count_non_matching_bitmap_pixels<B: MemoryBus>(
        &self,
        memory: &B,
        bmp_ptr: u32,
        x: i32,
        y: i32,
        w: u16,
        h: u16,
        transparent: u16,
        color_check: u16,
    ) -> Result<i32, MemoryAccessError> {
        let mut count = 0i32;
        for j in 0..h as i32 {
            for i in 0..w as i32 {
                let xx = x + i;
                let yy = y + j;
                if xx < 0 || yy < 0 || xx >= SCREEN_WIDTH as i32 || yy >= SCREEN_HEIGHT as i32 {
                    continue;
                }

                let src_index = i as u32 + j as u32 * w as u32;
                let pixel =
                    memory.read16(GuestAddr::new(bmp_ptr.wrapping_add(src_index.wrapping_mul(2))))?;
                if pixel == transparent {
                    continue;
                }

                let dst_index = yy as usize * SCREEN_WIDTH + xx as usize;
                if self.screen_buffer[dst_index] != color_check {
                    count += 1;
                }
            }
        }
        Ok(count)
    }

    fn load_sky16_font(&mut self) -> Option<&[u8]> {
        if self.font_sky16.is_none() {
            let path = self.resolve_guest_path("system/gb16.uc2");
            self.font_sky16 = Some(fs::read(path).unwrap_or_default());
        }
        self.font_sky16
            .as_deref()
            .filter(|bytes| !bytes.is_empty())
    }

    fn load_sky16_glyph(&mut self, ch: u16) -> Option<[u8; 32]> {
        let font = self.load_sky16_font()?;
        let start = ch as usize * 32;
        let glyph = font.get(start..start + 32)?;
        let mut out = [0u8; 32];
        out.copy_from_slice(glyph);
        Some(out)
    }

    fn draw_sky16_char(&mut self, ch: u16, x: i32, y: i32, color: u16) -> u16 {
        let Some(glyph) = self.load_sky16_glyph(ch) else {
            return 0;
        };
        let width = if ch < 128 { 8u16 } else { 16u16 };
        for row in 0..16usize {
            let data = ((glyph[row * 2] as u16) << 8) | glyph[row * 2 + 1] as u16;
            for col in 0..width as usize {
                if data & (1 << (15 - col)) == 0 {
                    continue;
                }
                let xx = x + col as i32;
                let yy = y + row as i32;
                if xx < 0 || yy < 0 || xx >= SCREEN_WIDTH as i32 || yy >= SCREEN_HEIGHT as i32 {
                    continue;
                }
                let dst_index = yy as usize * SCREEN_WIDTH + xx as usize;
                self.screen_buffer[dst_index] = color;
            }
        }
        width
    }

    fn draw_sky16_char_clipped(
        &mut self,
        ch: u16,
        x: i32,
        y: i32,
        color: u16,
        clip_left: i32,
        clip_top: i32,
        clip_right: i32,
        clip_bottom: i32,
    ) -> u16 {
        let Some(glyph) = self.load_sky16_glyph(ch) else {
            return 0;
        };
        let width = if ch < 128 { 8u16 } else { 16u16 };
        let dirty_x0 = x.max(clip_left).max(0);
        let dirty_y0 = y.max(clip_top).max(0);
        let dirty_x1 = x
            .saturating_add(width as i32)
            .min(clip_right)
            .min(SCREEN_WIDTH as i32);
        let dirty_y1 = y.saturating_add(16).min(clip_bottom).min(SCREEN_HEIGHT as i32);

        for row in 0..16usize {
            let yy = y + row as i32;
            if yy < clip_top || yy >= clip_bottom || yy < 0 || yy >= SCREEN_HEIGHT as i32 {
                continue;
            }
            let data = ((glyph[row * 2] as u16) << 8) | glyph[row * 2 + 1] as u16;
            for col in 0..width as usize {
                if data & (1 << (15 - col)) == 0 {
                    continue;
                }
                let xx = x + col as i32;
                if xx < clip_left || xx >= clip_right || xx < 0 || xx >= SCREEN_WIDTH as i32 {
                    continue;
                }
                let dst_index = yy as usize * SCREEN_WIDTH + xx as usize;
                self.screen_buffer[dst_index] = color;
            }
        }

        if dirty_x0 < dirty_x1 && dirty_y0 < dirty_y1 {
            self.mark_dirty_region(
                dirty_x0,
                dirty_y0,
                (dirty_x1 - dirty_x0) as u16,
                (dirty_y1 - dirty_y0) as u16,
            );
        }
        width
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
    let sms_return_flag_cell =
        legacy_runtime_addr(mr_table_addr, LEGACY_SMS_RETURN_FLAG_CELL_OFFSET);
    let sms_return_val_cell = legacy_runtime_addr(mr_table_addr, LEGACY_SMS_RETURN_VAL_CELL_OFFSET);
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
        (MR_SMS_RETURN_FLAG_OFFSET, sms_return_flag_cell.get()),
        (MR_SMS_RETURN_VAL_OFFSET, sms_return_val_cell.get()),
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
        sms_return_flag_cell,
        sms_return_val_cell,
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

fn rgb888_to_rgb565(r: u8, g: u8, b: u8) -> u16 {
    (((r as u16) >> 3) << 11) | (((g as u16) >> 2) << 5) | ((b as u16) >> 3)
}

fn looks_like_legacy_send_app_event(r1: u32, r3: u32) -> bool {
    r1 >= DEFAULT_LAYOUT.code_address().get() && r3 >= DEFAULT_LAYOUT.code_address().get()
}

fn read_guest_bitmap_draw<B: MemoryBus>(
    memory: &B,
    addr: u32,
) -> Result<GuestBitmapDraw, MemoryAccessError> {
    Ok(GuestBitmapDraw {
        p: memory.read32(GuestAddr::new(addr))?,
        w: memory.read16(GuestAddr::new(addr.wrapping_add(4)))?,
        h: memory.read16(GuestAddr::new(addr.wrapping_add(6)))?,
        x: memory.read16(GuestAddr::new(addr.wrapping_add(8)))? as i16,
        y: memory.read16(GuestAddr::new(addr.wrapping_add(10)))? as i16,
    })
}

fn read_guest_trans_matrix<B: MemoryBus>(
    memory: &B,
    addr: u32,
) -> Result<GuestTransMatrix, MemoryAccessError> {
    Ok(GuestTransMatrix {
        a: memory.read16(GuestAddr::new(addr))? as i16,
        b: memory.read16(GuestAddr::new(addr.wrapping_add(2)))? as i16,
        c: memory.read16(GuestAddr::new(addr.wrapping_add(4)))? as i16,
        d: memory.read16(GuestAddr::new(addr.wrapping_add(6)))? as i16,
        rop: memory.read16(GuestAddr::new(addr.wrapping_add(8)))?,
    })
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

fn guest_wstrlen<B: MemoryBus>(
    memory: &B,
    addr: u32,
    max_bytes: usize,
) -> Result<u32, MemoryAccessError> {
    if addr == 0 {
        return Ok(0);
    }
    let mut len = 0u32;
    while (len as usize) + 1 < max_bytes {
        let hi = memory.read8(GuestAddr::new(addr.wrapping_add(len)))?;
        let lo = memory.read8(GuestAddr::new(addr.wrapping_add(len).wrapping_add(1)))?;
        if hi == 0 && lo == 0 {
            break;
        }
        len = len.wrapping_add(2);
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

fn read_guest_text_chars<B: MemoryBus>(
    memory: &B,
    addr: u32,
    is_unicode: bool,
    max_chars: usize,
) -> Result<Vec<u16>, MemoryAccessError> {
    if addr == 0 {
        return Ok(Vec::new());
    }
    let mut chars = Vec::new();
    if is_unicode {
        for index in 0..max_chars {
            let offset = (index as u32).wrapping_mul(2);
            let hi = memory.read8(GuestAddr::new(addr.wrapping_add(offset)))?;
            let lo = memory.read8(GuestAddr::new(addr.wrapping_add(offset).wrapping_add(1)))?;
            let ch = u16::from_be_bytes([hi, lo]);
            if ch == 0 {
                break;
            }
            chars.push(ch);
        }
    } else {
        for index in 0..max_chars {
            let ch = memory.read8(GuestAddr::new(addr.wrapping_add(index as u32)))?;
            if ch == 0 {
                break;
            }
            chars.push(ch as u16);
        }
    }
    Ok(chars)
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
