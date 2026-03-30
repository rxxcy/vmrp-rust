use std::ffi::c_void;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirtyRect {
    pub x: i32,
    pub y: i32,
    pub w: u16,
    pub h: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuestInputEvent {
    pub code: i32,
    pub p0: u32,
    pub p1: u32,
}

const MR_KEY_0: i32 = 0;
const MR_KEY_STAR: i32 = 10;
const MR_KEY_POUND: i32 = 11;
const MR_KEY_UP: i32 = 12;
const MR_KEY_DOWN: i32 = 13;
const MR_KEY_LEFT: i32 = 14;
const MR_KEY_RIGHT: i32 = 15;
const MR_KEY_POWER: i32 = 16;
const MR_KEY_SOFTLEFT: i32 = 17;
const MR_KEY_SOFTRIGHT: i32 = 18;
const MR_KEY_SEND: i32 = 19;
const MR_KEY_SELECT: i32 = 20;
const MR_KEY_PRESS: i32 = 0;
const MR_KEY_RELEASE: i32 = 1;
const MR_MOUSE_DOWN: i32 = 2;
const MR_MOUSE_UP: i32 = 3;
const MR_MOUSE_MOVE: i32 = 12;
const MR_EVENT_EXIT: i32 = 8;

fn should_keep_window_open(closed: bool) -> bool {
    !closed
}

pub fn copy_rgb565_region_to_bgra(
    source: &[u16],
    width: usize,
    height: usize,
    rect: DirtyRect,
    dest: &mut [u8],
) {
    let x0 = rect.x.max(0) as usize;
    let y0 = rect.y.max(0) as usize;
    let x1 = (rect.x + rect.w as i32).max(0).min(width as i32) as usize;
    let y1 = (rect.y + rect.h as i32).max(0).min(height as i32) as usize;

    if x0 >= x1 || y0 >= y1 {
        return;
    }

    for y in y0..y1 {
        for x in x0..x1 {
            let src = source[y * width + x];
            let offset = (y * width + x) * 4;
            dest[offset] = ((src & 0x1F) << 3) as u8;
            dest[offset + 1] = (((src >> 5) & 0x3F) << 2) as u8;
            dest[offset + 2] = (((src >> 11) & 0x1F) << 3) as u8;
            dest[offset + 3] = 0xFF;
        }
    }
}

#[cfg(windows)]
pub struct WindowPresenter {
    hwnd: *mut c_void,
    width: usize,
    height: usize,
    pixels: Vec<u8>,
    pending_events: Vec<GuestInputEvent>,
    has_presented_frame: bool,
    _class_name: Vec<u16>,
    closed: bool,
}

#[cfg(not(windows))]
pub struct WindowPresenter {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
    pending_events: Vec<GuestInputEvent>,
    has_presented_frame: bool,
    closed: bool,
}

#[cfg(windows)]
impl WindowPresenter {
    pub fn new(title: &str, width: usize, height: usize) -> Result<Self, String> {
        use std::ptr::null;

        let class_name = wide("vmrp-rust-window");
        let title = wide(title);
        let instance = unsafe { GetModuleHandleW(null()) };
        let wnd_class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
        };
        unsafe {
            RegisterClassW(&wnd_class);
        }
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class_name.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                width as i32 + 16,
                height as i32 + 39,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                instance,
                std::ptr::null_mut(),
            )
        };
        if hwnd.is_null() {
            return Err(String::from("create window failed"));
        }
        unsafe {
            ShowWindow(hwnd, SW_SHOW);
            UpdateWindow(hwnd);
        }
        Ok(Self {
            hwnd,
            width,
            height,
            pixels: vec![0; width * height * 4],
            pending_events: Vec::new(),
            has_presented_frame: false,
            _class_name: class_name,
            closed: false,
        })
    }

    pub fn present(&mut self, source: &[u16], rect: DirtyRect) -> Result<(), String> {
        if self.closed {
            return Ok(());
        }
        self.pump();
        if self.closed {
            return Ok(());
        }

        copy_rgb565_region_to_bgra(source, self.width, self.height, rect, &mut self.pixels);
        self.has_presented_frame = true;
        let dc = unsafe { GetDC(self.hwnd) };
        if dc.is_null() {
            return Err(String::from("get dc failed"));
        }
        let bitmap_info = bitmap_info(self.width as i32, self.height as i32);
        unsafe {
            StretchDIBits(
                dc,
                0,
                0,
                self.width as i32,
                self.height as i32,
                0,
                0,
                self.width as i32,
                self.height as i32,
                self.pixels.as_ptr() as *const c_void,
                &bitmap_info,
                DIB_RGB_COLORS,
                SRCCOPY,
            );
            ReleaseDC(self.hwnd, dc);
        }
        Ok(())
    }

    pub fn pump(&mut self) {
        let mut msg = MSG::default();
        loop {
            let has_message =
                unsafe { PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) };
            if has_message == 0 {
                break;
            }
            if msg.message == WM_QUIT {
                self.closed = true;
                self.pending_events.push(GuestInputEvent {
                    code: MR_EVENT_EXIT,
                    p0: 0,
                    p1: 0,
                });
                break;
            }
            if let Some(event) = translate_win32_key_event(msg.message, msg.w_param) {
                self.pending_events.push(event);
            } else if let Some(event) = translate_win32_pointer_event(msg.message, msg.l_param) {
                self.pending_events.push(event);
            }
            unsafe {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    pub fn take_guest_events(&mut self) -> Vec<GuestInputEvent> {
        std::mem::take(&mut self.pending_events)
    }

    pub fn should_stay_open(&self) -> bool {
        should_keep_window_open(self.closed)
    }
}

#[cfg(not(windows))]
impl WindowPresenter {
    pub fn new(_title: &str, width: usize, height: usize) -> Result<Self, String> {
        Ok(Self {
            width,
            height,
            pixels: vec![0; width * height * 4],
            pending_events: Vec::new(),
            has_presented_frame: false,
            closed: false,
        })
    }

    pub fn present(&mut self, source: &[u16], rect: DirtyRect) -> Result<(), String> {
        copy_rgb565_region_to_bgra(source, self.width, self.height, rect, &mut self.pixels);
        Ok(())
    }

    pub fn pump(&mut self) {}

    pub fn take_guest_events(&mut self) -> Vec<GuestInputEvent> {
        std::mem::take(&mut self.pending_events)
    }

    pub fn should_stay_open(&self) -> bool {
        should_keep_window_open(self.closed)
    }
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
const WS_OVERLAPPEDWINDOW: u32 = 0x00CF0000;
#[cfg(windows)]
const WS_VISIBLE: u32 = 0x10000000;
#[cfg(windows)]
const CW_USEDEFAULT: i32 = 0x80000000u32 as i32;
#[cfg(windows)]
const SW_SHOW: i32 = 5;
#[cfg(windows)]
const PM_REMOVE: u32 = 0x0001;
#[cfg(windows)]
const WM_DESTROY: u32 = 0x0002;
#[cfg(windows)]
const WM_QUIT: u32 = 0x0012;
#[cfg(windows)]
const WM_KEYDOWN: u32 = 0x0100;
#[cfg(windows)]
const WM_KEYUP: u32 = 0x0101;
#[cfg(windows)]
const WM_SYSKEYDOWN: u32 = 0x0104;
#[cfg(windows)]
const WM_SYSKEYUP: u32 = 0x0105;
#[cfg(windows)]
const WM_MOUSEMOVE: u32 = 0x0200;
#[cfg(windows)]
const WM_LBUTTONDOWN: u32 = 0x0201;
#[cfg(windows)]
const WM_LBUTTONUP: u32 = 0x0202;
#[cfg(windows)]
const DIB_RGB_COLORS: u32 = 0;
#[cfg(windows)]
const SRCCOPY: u32 = 0x00CC0020;
#[cfg(windows)]
const VK_RETURN: u32 = 0x0D;
#[cfg(windows)]
const VK_TAB: u32 = 0x09;
#[cfg(windows)]
const VK_ESCAPE: u32 = 0x1B;
#[cfg(windows)]
const VK_UP: u32 = 0x26;
#[cfg(windows)]
const VK_DOWN: u32 = 0x28;
#[cfg(windows)]
const VK_LEFT: u32 = 0x25;
#[cfg(windows)]
const VK_RIGHT: u32 = 0x27;
#[cfg(windows)]
const VK_NUMPAD0: u32 = 0x60;
#[cfg(windows)]
const VK_NUMPAD9: u32 = 0x69;
#[cfg(windows)]
const VK_OEM_PLUS: u32 = 0xBB;
#[cfg(windows)]
const VK_OEM_MINUS: u32 = 0xBD;
#[cfg(windows)]
const VK_OEM_4: u32 = 0xDB;
#[cfg(windows)]
const VK_OEM_6: u32 = 0xDD;

#[cfg(windows)]
fn translate_win32_key_event(message: u32, w_param: usize) -> Option<GuestInputEvent> {
    let code = match message {
        WM_KEYDOWN | WM_SYSKEYDOWN => MR_KEY_PRESS,
        WM_KEYUP | WM_SYSKEYUP => MR_KEY_RELEASE,
        _ => return None,
    };
    let key = map_virtual_key(w_param as u32)?;
    Some(GuestInputEvent {
        code,
        p0: key as u32,
        p1: 0,
    })
}

#[cfg(not(windows))]
fn translate_win32_key_event(_message: u32, _w_param: usize) -> Option<GuestInputEvent> {
    None
}

#[cfg(windows)]
fn translate_win32_pointer_event(message: u32, l_param: isize) -> Option<GuestInputEvent> {
    let code = match message {
        WM_MOUSEMOVE => MR_MOUSE_MOVE,
        WM_LBUTTONDOWN => MR_MOUSE_DOWN,
        WM_LBUTTONUP => MR_MOUSE_UP,
        _ => return None,
    };
    let packed = l_param as u32;
    let x = (packed & 0xFFFF) as u16 as u32;
    let y = ((packed >> 16) & 0xFFFF) as u16 as u32;
    Some(GuestInputEvent { code, p0: x, p1: y })
}

#[cfg(not(windows))]
fn translate_win32_pointer_event(_message: u32, _l_param: isize) -> Option<GuestInputEvent> {
    None
}

fn map_virtual_key(vk: u32) -> Option<i32> {
    match vk {
        0x30..=0x39 => Some(MR_KEY_0 + (vk - 0x30) as i32),
        VK_NUMPAD0..=VK_NUMPAD9 => Some(MR_KEY_0 + (vk - VK_NUMPAD0) as i32),
        VK_RETURN => Some(MR_KEY_SELECT),
        VK_OEM_PLUS => Some(MR_KEY_POUND),
        VK_OEM_MINUS => Some(MR_KEY_STAR),
        VK_UP | 0x57 => Some(MR_KEY_UP),
        VK_DOWN | 0x53 => Some(MR_KEY_DOWN),
        VK_LEFT | 0x41 => Some(MR_KEY_LEFT),
        VK_RIGHT | 0x44 => Some(MR_KEY_RIGHT),
        VK_OEM_4 | 0x51 => Some(MR_KEY_SOFTLEFT),
        VK_OEM_6 | 0x45 => Some(MR_KEY_SOFTRIGHT),
        VK_TAB => Some(MR_KEY_SEND),
        VK_ESCAPE => Some(MR_KEY_POWER),
        _ => None,
    }
}

#[cfg(windows)]
type WndProc = Option<unsafe extern "system" fn(*mut c_void, u32, usize, isize) -> isize>;

#[cfg(windows)]
#[allow(non_snake_case)]
#[repr(C)]
struct WNDCLASSW {
    style: u32,
    lpfnWndProc: WndProc,
    cbClsExtra: i32,
    cbWndExtra: i32,
    hInstance: *mut c_void,
    hIcon: *mut c_void,
    hCursor: *mut c_void,
    hbrBackground: *mut c_void,
    lpszMenuName: *const u16,
    lpszClassName: *const u16,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct POINT {
    x: i32,
    y: i32,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct MSG {
    hwnd: *mut c_void,
    message: u32,
    w_param: usize,
    l_param: isize,
    time: u32,
    pt: POINT,
    l_private: u32,
}

#[cfg(windows)]
#[repr(C)]
struct RGBQUAD {
    rgb_blue: u8,
    rgb_green: u8,
    rgb_red: u8,
    rgb_reserved: u8,
}

#[cfg(windows)]
#[repr(C)]
struct BITMAPINFOHEADER {
    bi_size: u32,
    bi_width: i32,
    bi_height: i32,
    bi_planes: u16,
    bi_bit_count: u16,
    bi_compression: u32,
    bi_size_image: u32,
    bi_x_pels_per_meter: i32,
    bi_y_pels_per_meter: i32,
    bi_clr_used: u32,
    bi_clr_important: u32,
}

#[cfg(windows)]
#[repr(C)]
struct BITMAPINFO {
    bmi_header: BITMAPINFOHEADER,
    bmi_colors: [RGBQUAD; 1],
}

#[cfg(windows)]
fn bitmap_info(width: i32, height: i32) -> BITMAPINFO {
    BITMAPINFO {
        bmi_header: BITMAPINFOHEADER {
            bi_size: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            bi_width: width,
            bi_height: -height,
            bi_planes: 1,
            bi_bit_count: 32,
            bi_compression: 0,
            bi_size_image: (width * height * 4) as u32,
            bi_x_pels_per_meter: 0,
            bi_y_pels_per_meter: 0,
            bi_clr_used: 0,
            bi_clr_important: 0,
        },
        bmi_colors: [RGBQUAD {
            rgb_blue: 0,
            rgb_green: 0,
            rgb_red: 0,
            rgb_reserved: 0,
        }],
    }
}

#[cfg(windows)]
unsafe extern "system" fn window_proc(
    hwnd: *mut c_void,
    msg: u32,
    w_param: usize,
    l_param: isize,
) -> isize {
    if msg == WM_DESTROY {
        unsafe { PostQuitMessage(0) };
        return 0;
    }
    unsafe { DefWindowProcW(hwnd, msg, w_param, l_param) }
}

#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassW(class: *const WNDCLASSW) -> u16;
    fn CreateWindowExW(
        ex_style: u32,
        class_name: *const u16,
        window_name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: *mut c_void,
        menu: *mut c_void,
        instance: *mut c_void,
        param: *mut c_void,
    ) -> *mut c_void;
    fn DefWindowProcW(hwnd: *mut c_void, msg: u32, w_param: usize, l_param: isize) -> isize;
    fn ShowWindow(hwnd: *mut c_void, cmd_show: i32) -> i32;
    fn UpdateWindow(hwnd: *mut c_void) -> i32;
    fn PeekMessageW(msg: *mut MSG, hwnd: *mut c_void, min: u32, max: u32, remove: u32) -> i32;
    fn TranslateMessage(msg: *const MSG) -> i32;
    fn DispatchMessageW(msg: *const MSG) -> isize;
    fn PostQuitMessage(exit_code: i32);
    fn GetDC(hwnd: *mut c_void) -> *mut c_void;
    fn ReleaseDC(hwnd: *mut c_void, dc: *mut c_void) -> i32;
}

#[cfg(windows)]
#[link(name = "gdi32")]
unsafe extern "system" {
    fn StretchDIBits(
        dc: *mut c_void,
        x_dest: i32,
        y_dest: i32,
        dest_width: i32,
        dest_height: i32,
        x_src: i32,
        y_src: i32,
        src_width: i32,
        src_height: i32,
        bits: *const c_void,
        bitmap_info: *const BITMAPINFO,
        usage: u32,
        rop: u32,
    ) -> i32;
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
}

#[cfg(test)]
mod tests {
    use super::{
        copy_rgb565_region_to_bgra, should_keep_window_open, translate_win32_key_event,
        translate_win32_pointer_event, DirtyRect, GuestInputEvent, MR_EVENT_EXIT, MR_KEY_0,
        MR_KEY_POWER, MR_KEY_PRESS, MR_KEY_RELEASE, MR_KEY_SELECT, MR_KEY_UP, MR_MOUSE_DOWN,
        MR_MOUSE_MOVE, MR_MOUSE_UP, VK_ESCAPE, VK_RETURN, VK_UP, WM_KEYDOWN, WM_KEYUP,
        WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    };

    #[test]
    fn copy_region_converts_rgb565_pixels_to_bgra() {
        let source = vec![0xF800u16, 0x07E0u16, 0x001Fu16, 0xFFFFu16];
        let mut dest = vec![0u8; 4 * 4];

        copy_rgb565_region_to_bgra(
            &source,
            2,
            2,
            DirtyRect {
                x: 0,
                y: 0,
                w: 2,
                h: 2,
            },
            &mut dest,
        );

        assert_eq!(&dest[0..4], &[0x00, 0x00, 0xF8, 0xFF]);
        assert_eq!(&dest[4..8], &[0x00, 0xFC, 0x00, 0xFF]);
        assert_eq!(&dest[8..12], &[0xF8, 0x00, 0x00, 0xFF]);
        assert_eq!(&dest[12..16], &[0xF8, 0xFC, 0xF8, 0xFF]);
    }

    #[test]
    fn copy_region_clips_dirty_rect_to_screen_bounds() {
        let source = vec![0x0000u16; 4];
        let mut dest = vec![0xAAu8; 4 * 4];

        copy_rgb565_region_to_bgra(
            &source,
            2,
            2,
            DirtyRect {
                x: 1,
                y: 1,
                w: 4,
                h: 4,
            },
            &mut dest,
        );

        assert_eq!(&dest[0..4], &[0xAA, 0xAA, 0xAA, 0xAA]);
        assert_eq!(&dest[12..16], &[0x00, 0x00, 0x00, 0xFF]);
    }

    #[test]
    fn maps_return_key_to_select_press_event() {
        assert_eq!(
            translate_win32_key_event(WM_KEYDOWN, VK_RETURN as usize),
            Some(GuestInputEvent {
                code: MR_KEY_PRESS,
                p0: MR_KEY_SELECT as u32,
                p1: 0,
            })
        );
    }

    #[test]
    fn maps_arrow_key_to_release_event() {
        assert_eq!(
            translate_win32_key_event(WM_KEYUP, VK_UP as usize),
            Some(GuestInputEvent {
                code: MR_KEY_RELEASE,
                p0: MR_KEY_UP as u32,
                p1: 0,
            })
        );
    }

    #[test]
    fn maps_digit_and_escape_keys_to_mythroad_codes() {
        assert_eq!(
            translate_win32_key_event(WM_KEYDOWN, b'1' as usize),
            Some(GuestInputEvent {
                code: MR_KEY_PRESS,
                p0: (MR_KEY_0 + 1) as u32,
                p1: 0,
            })
        );
        assert_eq!(
            translate_win32_key_event(WM_KEYDOWN, VK_ESCAPE as usize),
            Some(GuestInputEvent {
                code: MR_KEY_PRESS,
                p0: MR_KEY_POWER as u32,
                p1: 0,
            })
        );
    }

    #[test]
    fn window_close_uses_exit_event_code() {
        assert_eq!(MR_EVENT_EXIT, 8);
    }

    #[test]
    fn maps_mouse_move_to_guest_coordinates() {
        assert_eq!(
            translate_win32_pointer_event(WM_MOUSEMOVE, pack_point(12, 34)),
            Some(GuestInputEvent {
                code: MR_MOUSE_MOVE,
                p0: 12,
                p1: 34,
            })
        );
    }

    #[test]
    fn maps_mouse_button_transitions() {
        assert_eq!(
            translate_win32_pointer_event(WM_LBUTTONDOWN, pack_point(7, 9)),
            Some(GuestInputEvent {
                code: MR_MOUSE_DOWN,
                p0: 7,
                p1: 9,
            })
        );
        assert_eq!(
            translate_win32_pointer_event(WM_LBUTTONUP, pack_point(7, 9)),
            Some(GuestInputEvent {
                code: MR_MOUSE_UP,
                p0: 7,
                p1: 9,
            })
        );
    }

    #[test]
    fn window_loop_stays_alive_until_presenter_is_closed() {
        assert!(should_keep_window_open(false));
        assert!(!should_keep_window_open(true));
    }

    fn pack_point(x: u16, y: u16) -> isize {
        ((y as u32) << 16 | x as u32) as isize
    }
}
