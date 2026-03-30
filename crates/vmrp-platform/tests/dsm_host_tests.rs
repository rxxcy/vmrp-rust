use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use vmrp_abi::MrpFile;
use vmrp_core::{GuestAddr, DEFAULT_LAYOUT};
use vmrp_cpu::{Cpu, ExecutionMode, MemoryBus, TestMemory};
use vmrp_platform::{ExtHost, HostScreenRegion, HostTimerCommand, FLAG_USE_UTF8_EDIT};

fn make_temp_dir() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("vmrp-platform-test-{nonce}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn new_host() -> ExtHost {
    ExtHost::new(
        GuestAddr::new(0x181000),
        GuestAddr::new(0x182000),
        GuestAddr::new(0x181100),
        GuestAddr::new(0x181140),
        GuestAddr::new(0x181180),
        GuestAddr::new(0x181200),
        GuestAddr::new(0x181300),
        GuestAddr::new(0x183000),
        0x20000,
    )
}

fn write_c_string(memory: &mut TestMemory, addr: u32, value: &str) {
    for (index, byte) in value.as_bytes().iter().enumerate() {
        memory
            .write8(GuestAddr::new(addr.wrapping_add(index as u32)), *byte)
            .unwrap();
    }
    memory
        .write8(GuestAddr::new(addr.wrapping_add(value.len() as u32)), 0)
        .unwrap();
}

fn read_c_string(memory: &TestMemory, addr: u32) -> String {
    let mut bytes = Vec::new();
    let mut cursor = addr;
    loop {
        let byte = memory.read8(GuestAddr::new(cursor)).unwrap();
        if byte == 0 {
            break;
        }
        bytes.push(byte);
        cursor = cursor.wrapping_add(1);
    }
    String::from_utf8(bytes).unwrap()
}

fn read_bytes(memory: &TestMemory, addr: u32, len: usize) -> Vec<u8> {
    (0..len)
        .map(|index| {
            memory
                .read8(GuestAddr::new(addr.wrapping_add(index as u32)))
                .unwrap()
        })
        .collect()
}

#[derive(Debug)]
struct DateTimeFields {
    year: u16,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
}

#[cfg(windows)]
fn current_local_time() -> DateTimeFields {
    #[repr(C)]
    struct SystemTime {
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
        fn GetLocalTime(system_time: *mut SystemTime);
    }

    let mut system_time = SystemTime {
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
    DateTimeFields {
        year: system_time.year,
        month: system_time.month as u8,
        day: system_time.day as u8,
        hour: system_time.hour as u8,
        minute: system_time.minute as u8,
        second: system_time.second as u8,
    }
}

#[test]
fn dsm_log_callback_exposes_last_guest_message() {
    let mut host = new_host();
    let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x200000);
    host.install_dsm_require_funcs(&mut memory, GuestAddr::new(0x190000), FLAG_USE_UTF8_EDIT)
        .unwrap();
    write_c_string(&mut memory, 0x191000, "diagnostic");

    let mut cpu = Cpu::new(memory);
    let log_pc = cpu.memory().read32(GuestAddr::new(0x190004)).unwrap();
    cpu.regs_mut().set_pc(log_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_reg(0, 0x191000);

    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(host.take_last_log_message(), Some(String::from("diagnostic")));
    assert_eq!(host.take_last_log_message(), None);
}

#[test]
fn install_dsm_require_funcs_sets_flags_and_callbacks() {
    let mut host = new_host();
    let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x200000);

    host.install_dsm_require_funcs(&mut memory, GuestAddr::new(0x190000), FLAG_USE_UTF8_EDIT)
        .unwrap();

    assert_ne!(memory.read32(GuestAddr::new(0x190030)).unwrap(), 0);
    assert_ne!(memory.read32(GuestAddr::new(0x190038)).unwrap(), 0);
    assert_eq!(
        memory.read32(GuestAddr::new(0x1900CC)).unwrap(),
        FLAG_USE_UTF8_EDIT
    );
}

#[test]
fn dsm_mem_get_returns_memory_manager_region_even_when_scratch_alloc_is_small() {
    let mut host = new_host();
    let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x900000);
    host.install_dsm_require_funcs(&mut memory, GuestAddr::new(0x190000), FLAG_USE_UTF8_EDIT)
        .unwrap();

    let mut cpu = Cpu::new(memory);
    let mem_get_pc = cpu.memory().read32(GuestAddr::new(0x190014)).unwrap();
    cpu.regs_mut().set_pc(mem_get_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_reg(0, 0x191000);
    cpu.regs_mut().set_reg(1, 0x191004);

    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);

    let base = cpu.memory().read32(GuestAddr::new(0x191000)).unwrap();
    let len = cpu.memory().read32(GuestAddr::new(0x191004)).unwrap();
    let manager_base = DEFAULT_LAYOUT.memory_manager_address().get();
    let manager_end = manager_base.wrapping_add(DEFAULT_LAYOUT.memory_manager_size());

    assert!(base >= manager_base);
    assert!(base < manager_end);
    assert!(len > 0);
    assert!(base.wrapping_add(len) <= manager_end);
}

#[test]
fn dsm_file_callbacks_open_read_close() {
    let dir = make_temp_dir();
    let file_path = dir.join("sample.bin");
    fs::write(&file_path, b"ABCD").unwrap();

    let mut host = new_host();
    host.set_working_dir(&dir);

    let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x200000);
    host.install_dsm_require_funcs(&mut memory, GuestAddr::new(0x190000), FLAG_USE_UTF8_EDIT)
        .unwrap();
    write_c_string(&mut memory, 0x191000, "sample.bin");

    let mut cpu = Cpu::new(memory);

    let open_pc = cpu.memory().read32(GuestAddr::new(0x190030)).unwrap();
    cpu.regs_mut().set_pc(open_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_reg(0, 0x191000);
    cpu.regs_mut().set_reg(1, 1);
    assert!(host.handle(&mut cpu).unwrap());
    let fd = cpu.regs().reg(0) as i32;
    assert!(fd >= 0);

    let read_pc = cpu.memory().read32(GuestAddr::new(0x190038)).unwrap();
    cpu.regs_mut().set_pc(read_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80004);
    cpu.regs_mut().set_reg(0, fd as u32);
    cpu.regs_mut().set_reg(1, 0x191100);
    cpu.regs_mut().set_reg(2, 4);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 4);
    assert_eq!(
        cpu.memory().read32(GuestAddr::new(0x191100)).unwrap(),
        0x44434241
    );

    let close_pc = cpu.memory().read32(GuestAddr::new(0x190034)).unwrap();
    cpu.regs_mut().set_pc(close_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80008);
    cpu.regs_mut().set_reg(0, fd as u32);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0) as i32, 0);

    let _ = fs::remove_file(file_path);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn dsm_open_strips_mythroad_prefix_from_guest_paths() {
    let dir = make_temp_dir();
    let file_path = dir.join("sample.bin");
    fs::write(&file_path, b"ABCD").unwrap();

    let mut host = new_host();
    host.set_working_dir(&dir);

    let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x200000);
    host.install_dsm_require_funcs(&mut memory, GuestAddr::new(0x190000), FLAG_USE_UTF8_EDIT)
        .unwrap();
    write_c_string(&mut memory, 0x191000, "mythroad/sample.bin");

    let mut cpu = Cpu::new(memory);

    let open_pc = cpu.memory().read32(GuestAddr::new(0x190030)).unwrap();
    cpu.regs_mut().set_pc(open_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_reg(0, 0x191000);
    cpu.regs_mut().set_reg(1, 1);
    assert!(host.handle(&mut cpu).unwrap());
    let fd = cpu.regs().reg(0) as i32;
    assert!(fd >= 0);

    let _ = fs::remove_file(file_path);
    let _ = fs::remove_dir_all(dir);
}

fn real_fallback_mrp_path() -> PathBuf {
    PathBuf::from(r"D:\opt\rust\vmrp\wasm\dist\fs\mythroad\ydqtwo.mrp")
}

#[test]
fn dsm_real_mrp_open_seek_read_matches_start_mr_payload() {
    let mrp_path = real_fallback_mrp_path();
    let mrp = MrpFile::from_path(&mrp_path).unwrap();
    let entry = mrp.entry("start.mr").unwrap();
    let expected = mrp.file_bytes("start.mr").unwrap().to_vec();

    let mut host = new_host();
    host.set_working_dir(mrp_path.parent().unwrap());

    let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x400000);
    host.install_dsm_require_funcs(&mut memory, GuestAddr::new(0x190000), FLAG_USE_UTF8_EDIT)
        .unwrap();
    write_c_string(&mut memory, 0x191000, "mythroad/ydqtwo.mrp");

    let mut cpu = Cpu::new(memory);

    let open_pc = cpu.memory().read32(GuestAddr::new(0x190030)).unwrap();
    cpu.regs_mut().set_pc(open_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_reg(0, 0x191000);
    cpu.regs_mut().set_reg(1, 1);
    assert!(host.handle(&mut cpu).unwrap());
    let fd = cpu.regs().reg(0) as i32;
    assert!(fd >= 0);

    let seek_pc = cpu.memory().read32(GuestAddr::new(0x190040)).unwrap();
    cpu.regs_mut().set_pc(seek_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80004);
    cpu.regs_mut().set_reg(0, fd as u32);
    cpu.regs_mut().set_reg(1, entry.offset());
    cpu.regs_mut().set_reg(2, 0);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), entry.offset());

    let read_pc = cpu.memory().read32(GuestAddr::new(0x190038)).unwrap();
    cpu.regs_mut().set_pc(read_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80008);
    cpu.regs_mut().set_reg(0, fd as u32);
    cpu.regs_mut().set_reg(1, 0x192000);
    cpu.regs_mut().set_reg(2, entry.len());
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), entry.len());

    let actual = read_bytes(cpu.memory(), 0x192000, entry.len() as usize);
    assert_eq!(actual, expected);
}

#[test]
fn dsm_file_info_and_getlen_callbacks_report_metadata() {
    let dir = make_temp_dir();
    let file_path = dir.join("sample.bin");
    fs::write(&file_path, b"ABCDE").unwrap();

    let mut host = new_host();
    host.set_working_dir(&dir);

    let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x200000);
    host.install_dsm_require_funcs(&mut memory, GuestAddr::new(0x190000), FLAG_USE_UTF8_EDIT)
        .unwrap();
    write_c_string(&mut memory, 0x191000, "sample.bin");
    write_c_string(&mut memory, 0x191040, ".");

    let mut cpu = Cpu::new(memory);

    let info_pc = cpu.memory().read32(GuestAddr::new(0x190044)).unwrap();
    cpu.regs_mut().set_pc(info_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_reg(0, 0x191000);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 1);

    cpu.regs_mut().set_pc(info_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80004);
    cpu.regs_mut().set_reg(0, 0x191040);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 2);

    let get_len_pc = cpu.memory().read32(GuestAddr::new(0x190064)).unwrap();
    cpu.regs_mut().set_pc(get_len_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80008);
    cpu.regs_mut().set_reg(0, 0x191000);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 5);

    let _ = fs::remove_file(file_path);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn dsm_directory_callbacks_iterate_entries() {
    let dir = make_temp_dir();
    fs::write(dir.join("a.txt"), b"A").unwrap();
    fs::write(dir.join("b.txt"), b"B").unwrap();

    let mut host = new_host();
    host.set_working_dir(&dir);

    let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x900000);
    host.install_dsm_require_funcs(&mut memory, GuestAddr::new(0x190000), FLAG_USE_UTF8_EDIT)
        .unwrap();
    write_c_string(&mut memory, 0x191000, ".");

    let mut cpu = Cpu::new(memory);

    let open_dir_pc = cpu.memory().read32(GuestAddr::new(0x190058)).unwrap();
    cpu.regs_mut().set_pc(open_dir_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_reg(0, 0x191000);
    assert!(host.handle(&mut cpu).unwrap());
    let handle = cpu.regs().reg(0) as i32;
    assert!(handle >= 0);

    let read_dir_pc = cpu.memory().read32(GuestAddr::new(0x19005C)).unwrap();
    let mut entries = Vec::new();
    loop {
        cpu.regs_mut().set_pc(read_dir_pc);
        cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
        cpu.regs_mut().set_lr(0x80004);
        cpu.regs_mut().set_reg(0, handle as u32);
        assert!(host.handle(&mut cpu).unwrap());
        let ptr = cpu.regs().reg(0);
        if ptr == 0 {
            break;
        }
        entries.push(read_c_string(cpu.memory(), ptr));
    }
    entries.sort();
    assert_eq!(entries, vec![String::from("a.txt"), String::from("b.txt")]);

    let close_dir_pc = cpu.memory().read32(GuestAddr::new(0x190060)).unwrap();
    cpu.regs_mut().set_pc(close_dir_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80008);
    cpu.regs_mut().set_reg(0, handle as u32);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn dsm_datetime_callback_rejects_null_pointer_and_reports_local_time() {
    let mut host = new_host();
    let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x200000);
    host.install_dsm_require_funcs(&mut memory, GuestAddr::new(0x190000), FLAG_USE_UTF8_EDIT)
        .unwrap();

    let mut cpu = Cpu::new(memory);
    let datetime_pc = cpu.memory().read32(GuestAddr::new(0x190028)).unwrap();

    cpu.regs_mut().set_pc(datetime_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_reg(0, 0);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0) as i32, -1);

    let expected = current_local_time();
    cpu.regs_mut().set_pc(datetime_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80004);
    cpu.regs_mut().set_reg(0, 0x191000);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(
        cpu.memory().read16(GuestAddr::new(0x191000)).unwrap(),
        expected.year
    );
    assert_eq!(
        cpu.memory().read8(GuestAddr::new(0x191002)).unwrap(),
        expected.month
    );
    assert_eq!(
        cpu.memory().read8(GuestAddr::new(0x191003)).unwrap(),
        expected.day
    );
    assert_eq!(
        cpu.memory().read8(GuestAddr::new(0x191004)).unwrap(),
        expected.hour
    );
    assert_eq!(
        cpu.memory().read8(GuestAddr::new(0x191005)).unwrap(),
        expected.minute
    );
    let second = cpu.memory().read8(GuestAddr::new(0x191006)).unwrap();
    assert!(second == expected.second || second == expected.second.saturating_add(1));
}

#[test]
fn dsm_file_mutation_callbacks_round_trip_data_and_paths() {
    let dir = make_temp_dir();

    let mut host = new_host();
    host.set_working_dir(&dir);

    let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x200000);
    host.install_dsm_require_funcs(&mut memory, GuestAddr::new(0x190000), FLAG_USE_UTF8_EDIT)
        .unwrap();
    write_c_string(&mut memory, 0x191000, "subdir");
    write_c_string(&mut memory, 0x191040, "subdir/from.bin");
    write_c_string(&mut memory, 0x191080, "subdir/to.bin");
    write_c_string(&mut memory, 0x1910C0, "XYZ");

    let mut cpu = Cpu::new(memory);

    let mkdir_pc = cpu.memory().read32(GuestAddr::new(0x190050)).unwrap();
    cpu.regs_mut().set_pc(mkdir_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_reg(0, 0x191000);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);

    let open_pc = cpu.memory().read32(GuestAddr::new(0x190030)).unwrap();
    cpu.regs_mut().set_pc(open_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80004);
    cpu.regs_mut().set_reg(0, 0x191040);
    cpu.regs_mut().set_reg(1, 4 | 8 | 16);
    assert!(host.handle(&mut cpu).unwrap());
    let fd = cpu.regs().reg(0) as i32;
    assert!(fd >= 0);

    let write_pc = cpu.memory().read32(GuestAddr::new(0x19003C)).unwrap();
    cpu.regs_mut().set_pc(write_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80008);
    cpu.regs_mut().set_reg(0, fd as u32);
    cpu.regs_mut().set_reg(1, 0x1910C0);
    cpu.regs_mut().set_reg(2, 3);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 3);

    let seek_pc = cpu.memory().read32(GuestAddr::new(0x190040)).unwrap();
    cpu.regs_mut().set_pc(seek_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x8000C);
    cpu.regs_mut().set_reg(0, fd as u32);
    cpu.regs_mut().set_reg(1, 0);
    cpu.regs_mut().set_reg(2, 0);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);

    let read_pc = cpu.memory().read32(GuestAddr::new(0x190038)).unwrap();
    cpu.regs_mut().set_pc(read_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80010);
    cpu.regs_mut().set_reg(0, fd as u32);
    cpu.regs_mut().set_reg(1, 0x191100);
    cpu.regs_mut().set_reg(2, 3);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 3);
    assert_eq!(read_c_string(cpu.memory(), 0x191100), "XYZ");

    let close_pc = cpu.memory().read32(GuestAddr::new(0x190034)).unwrap();
    cpu.regs_mut().set_pc(close_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80014);
    cpu.regs_mut().set_reg(0, fd as u32);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);

    let rename_pc = cpu.memory().read32(GuestAddr::new(0x19004C)).unwrap();
    cpu.regs_mut().set_pc(rename_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80018);
    cpu.regs_mut().set_reg(0, 0x191040);
    cpu.regs_mut().set_reg(1, 0x191080);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);

    let remove_pc = cpu.memory().read32(GuestAddr::new(0x190048)).unwrap();
    cpu.regs_mut().set_pc(remove_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x8001C);
    cpu.regs_mut().set_reg(0, 0x191080);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);

    let rmdir_pc = cpu.memory().read32(GuestAddr::new(0x190054)).unwrap();
    cpu.regs_mut().set_pc(rmdir_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80020);
    cpu.regs_mut().set_reg(0, 0x191000);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);

    assert!(!dir.join("subdir").exists());

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn dsm_sleep_callback_respects_requested_delay() {
    let mut host = new_host();
    let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x200000);
    host.install_dsm_require_funcs(&mut memory, GuestAddr::new(0x190000), FLAG_USE_UTF8_EDIT)
        .unwrap();

    let mut cpu = Cpu::new(memory);
    let uptime_pc = cpu.memory().read32(GuestAddr::new(0x190024)).unwrap();
    let sleep_pc = cpu.memory().read32(GuestAddr::new(0x19002C)).unwrap();

    cpu.regs_mut().set_pc(uptime_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    assert!(host.handle(&mut cpu).unwrap());
    let before = cpu.regs().reg(0);

    cpu.regs_mut().set_pc(sleep_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80004);
    cpu.regs_mut().set_reg(0, 120);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);

    cpu.regs_mut().set_pc(uptime_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80008);
    assert!(host.handle(&mut cpu).unwrap());
    let after = cpu.regs().reg(0);

    assert!(
        after >= before + 100,
        "sleep(120) should advance uptime by about 120ms, before={before}, after={after}"
    );
}

#[test]
fn dsm_rand_callbacks_follow_msvc_sequence() {
    let mut host = new_host();
    let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x200000);
    host.install_dsm_require_funcs(&mut memory, GuestAddr::new(0x190000), FLAG_USE_UTF8_EDIT)
        .unwrap();

    let mut cpu = Cpu::new(memory);
    let srand_pc = cpu.memory().read32(GuestAddr::new(0x19000C)).unwrap();
    let rand_pc = cpu.memory().read32(GuestAddr::new(0x190010)).unwrap();

    cpu.regs_mut().set_pc(srand_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_reg(0, 1);
    assert!(host.handle(&mut cpu).unwrap());

    cpu.regs_mut().set_pc(rand_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80004);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 41);

    cpu.regs_mut().set_pc(rand_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80008);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 18467);
}

#[test]
fn dsm_timer_start_records_delay() {
    let mut host = new_host();
    let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x200000);
    host.install_dsm_require_funcs(&mut memory, GuestAddr::new(0x190000), FLAG_USE_UTF8_EDIT)
        .unwrap();

    let mut cpu = Cpu::new(memory);
    let timer_start_pc = cpu.memory().read32(GuestAddr::new(0x19001C)).unwrap();

    cpu.regs_mut().set_pc(timer_start_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_reg(0, 123);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(host.pending_timer_delay_ms(), Some(123));
}

#[test]
fn dsm_timer_restart_replaces_previous_delay() {
    let mut host = new_host();
    let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x200000);
    host.install_dsm_require_funcs(&mut memory, GuestAddr::new(0x190000), FLAG_USE_UTF8_EDIT)
        .unwrap();

    let mut cpu = Cpu::new(memory);
    let timer_start_pc = cpu.memory().read32(GuestAddr::new(0x19001C)).unwrap();

    cpu.regs_mut().set_pc(timer_start_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_reg(0, 50);
    assert!(host.handle(&mut cpu).unwrap());

    cpu.regs_mut().set_pc(timer_start_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80004);
    cpu.regs_mut().set_reg(0, 75);
    assert!(host.handle(&mut cpu).unwrap());

    assert_eq!(host.pending_timer_delay_ms(), Some(75));
}

#[test]
fn dsm_timer_stop_clears_pending_delay() {
    let mut host = new_host();
    let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x200000);
    host.install_dsm_require_funcs(&mut memory, GuestAddr::new(0x190000), FLAG_USE_UTF8_EDIT)
        .unwrap();

    let mut cpu = Cpu::new(memory);
    let timer_start_pc = cpu.memory().read32(GuestAddr::new(0x19001C)).unwrap();
    let timer_stop_pc = cpu.memory().read32(GuestAddr::new(0x190020)).unwrap();

    cpu.regs_mut().set_pc(timer_start_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_reg(0, 40);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(host.pending_timer_delay_ms(), Some(40));

    cpu.regs_mut().set_pc(timer_stop_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80004);
    assert!(host.handle(&mut cpu).unwrap());
    assert_eq!(cpu.regs().reg(0), 0);
    assert_eq!(host.pending_timer_delay_ms(), None);
}

#[test]
fn dsm_timer_command_can_be_consumed_once() {
    let mut host = new_host();
    let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x200000);
    host.install_dsm_require_funcs(&mut memory, GuestAddr::new(0x190000), FLAG_USE_UTF8_EDIT)
        .unwrap();

    let mut cpu = Cpu::new(memory);
    let timer_start_pc = cpu.memory().read32(GuestAddr::new(0x19001C)).unwrap();

    cpu.regs_mut().set_pc(timer_start_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_reg(0, 88);
    assert!(host.handle(&mut cpu).unwrap());

    assert_eq!(host.take_timer_command(), Some(HostTimerCommand::Start(88)));
    assert_eq!(host.take_timer_command(), None);
}

#[test]
fn dsm_timer_stop_emits_stop_command() {
    let mut host = new_host();
    let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x200000);
    host.install_dsm_require_funcs(&mut memory, GuestAddr::new(0x190000), FLAG_USE_UTF8_EDIT)
        .unwrap();

    let mut cpu = Cpu::new(memory);
    let timer_stop_pc = cpu.memory().read32(GuestAddr::new(0x190020)).unwrap();

    cpu.regs_mut().set_pc(timer_stop_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    assert!(host.handle(&mut cpu).unwrap());

    assert_eq!(host.take_timer_command(), Some(HostTimerCommand::Stop));
    assert_eq!(host.take_timer_command(), None);
}

#[test]
fn dsm_draw_bitmap_refreshes_host_framebuffer_region() {
    let mut host = new_host();
    let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x200000);
    host.install_dsm_require_funcs(&mut memory, GuestAddr::new(0x190000), FLAG_USE_UTF8_EDIT)
        .unwrap();

    memory.write16(GuestAddr::new(0x191000), 0x1111).unwrap();
    memory.write16(GuestAddr::new(0x191002), 0x2222).unwrap();
    memory.write16(GuestAddr::new(0x1911E0), 0x3333).unwrap();
    memory.write16(GuestAddr::new(0x1911E2), 0x4444).unwrap();
    memory.write32(GuestAddr::new(0x192000), 2).unwrap();

    let mut cpu = Cpu::new(memory);
    let draw_pc = cpu.memory().read32(GuestAddr::new(0x190068)).unwrap();

    cpu.regs_mut().set_pc(draw_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_sp(0x192000);
    cpu.regs_mut().set_reg(0, 0x191000);
    cpu.regs_mut().set_reg(1, 0);
    cpu.regs_mut().set_reg(2, 0);
    cpu.regs_mut().set_reg(3, 2);
    assert!(host.handle(&mut cpu).unwrap());

    let framebuffer = host.screen_buffer();
    assert_eq!(framebuffer[0], 0x1111);
    assert_eq!(framebuffer[1], 0x2222);
    assert_eq!(framebuffer[240], 0x3333);
    assert_eq!(framebuffer[241], 0x4444);
}

#[test]
fn dsm_draw_bitmap_marks_dirty_region() {
    let mut host = new_host();
    let mut memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x200000);
    host.install_dsm_require_funcs(&mut memory, GuestAddr::new(0x190000), FLAG_USE_UTF8_EDIT)
        .unwrap();

    memory.write16(GuestAddr::new(0x191000), 0x7777).unwrap();
    memory.write32(GuestAddr::new(0x192000), 1).unwrap();

    let mut cpu = Cpu::new(memory);
    let draw_pc = cpu.memory().read32(GuestAddr::new(0x190068)).unwrap();

    cpu.regs_mut().set_pc(draw_pc);
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_sp(0x192000);
    cpu.regs_mut().set_reg(0, 0x191000);
    cpu.regs_mut().set_reg(1, 5);
    cpu.regs_mut().set_reg(2, 7);
    cpu.regs_mut().set_reg(3, 1);
    assert!(host.handle(&mut cpu).unwrap());

    assert_eq!(
        host.take_dirty_region(),
        Some(HostScreenRegion {
            x: 5,
            y: 7,
            w: 1,
            h: 1,
        })
    );
    assert_eq!(host.take_dirty_region(), None);
}



#[test]
fn host_mr_malloc_uses_memory_manager_region_and_reuses_freed_block() {
    let mut host = new_host();
    let memory = TestMemory::with_ram(DEFAULT_LAYOUT.code_address(), 0x900000);
    let mut cpu = Cpu::new(memory);

    cpu.regs_mut().set_pc(host.mr_malloc_addr.get());
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80000);
    cpu.regs_mut().set_reg(0, 32);
    assert!(host.handle(&mut cpu).unwrap());
    let first = cpu.regs().reg(0);

    let manager_base = DEFAULT_LAYOUT.memory_manager_address().get();
    let manager_end = manager_base.wrapping_add(DEFAULT_LAYOUT.memory_manager_size());
    assert!(
        first >= manager_base && first < manager_end,
        "mr_malloc should allocate from shared memory manager region: first=0x{first:X}, base=0x{manager_base:X}, end=0x{manager_end:X}"
    );

    cpu.regs_mut().set_pc(host.mr_free_addr.get());
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80004);
    cpu.regs_mut().set_reg(0, first);
    cpu.regs_mut().set_reg(1, 32);
    assert!(host.handle(&mut cpu).unwrap());

    cpu.regs_mut().set_pc(host.mr_malloc_addr.get());
    cpu.regs_mut().set_execution_mode(ExecutionMode::Arm);
    cpu.regs_mut().set_lr(0x80008);
    cpu.regs_mut().set_reg(0, 32);
    assert!(host.handle(&mut cpu).unwrap());
    let second = cpu.regs().reg(0);

    assert_eq!(second, first);
}

