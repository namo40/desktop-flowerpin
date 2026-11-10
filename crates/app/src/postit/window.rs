//! 포스트잇의 창, 글을 적는 칸, 메뉴.
//!
//! 포스트잇 한 장이 창 하나다. 꽃핀의 창과 같은 종류로, 픽셀마다 투명도가 있고 테두리와 제목 줄이 없다.
//! 완전히 투명한 곳을 누르면 클릭이 뒤에 있는 창으로 넘어간다. 눌러도 쓰던 창의 포커스를 빼앗지 않는다.
//!
//! 그림 버퍼는 창마다 두지 않는다. 모든 포스트잇이 [`Canvas`] 하나에 차례로 그려서 화면에 넘긴다.
//! 넘긴 그림은 Windows가 들고 있으므로, 가만히 붙어 있는 포스트잇은 이 프로그램의 메모리를 거의 쓰지 않는다.
//!
//! 창에서 일어난 일은 그 자리에서 처리하지 않고 [`Event`]로 적어 두기만 한다. 앱이 한 바퀴 돌 때 꺼내 처리한다.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};

use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CreateCompatibleDC,
    CreateDIBSection, CreateSolidBrush, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetMonitorInfoW, HBITMAP, HBRUSH, HDC,
    HFONT, HGDIOBJ, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTONULL, MONITORINFO, MonitorFromPoint, ScreenToClient,
    SelectObject, SetBkColor, SetTextColor,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetCapture, ReleaseCapture, SetCapture, SetFocus, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CS_DBLCLKS, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow, GWL_EXSTYLE,
    GetWindowLongW, GetWindowTextLengthW, GetWindowTextW, HWND_BOTTOM, HWND_NOTOPMOST, HWND_TOP, HWND_TOPMOST,
    IDC_ARROW, IDC_HAND, IDC_SIZEALL, IDC_SIZENWSE, LoadCursorW, MA_NOACTIVATE, MF_CHECKED, MF_SEPARATOR, MF_STRING,
    PostMessageW, RegisterClassExW, SW_SHOW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SendMessageW, SetCursor, SetForegroundWindow, SetWindowPos, SetWindowTextW, ShowWindow, TPM_NONOTIFY,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx, ULW_ALPHA, UPDATELAYEREDWINDOWINFO, UpdateLayeredWindowIndirect,
    WM_ACTIVATE, WM_CAPTURECHANGED, WM_CLOSE, WM_COMMAND, WM_CTLCOLOREDIT, WM_KEYDOWN, WM_LBUTTONDBLCLK,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_NULL, WM_RBUTTONUP, WM_SETCURSOR, WM_SETFONT,
    WNDCLASSEXW, WS_CHILD, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};

use crate::win::wide;

/// 확인하려고 스크립트로 돌리는 중이다. 적는 칸을 띄워도, 쓰고 있던 창에서 키보드 입력을 가져오지 않는다.
pub static QUIET: AtomicBool = AtomicBool::new(false);

/// 포스트잇 창과 판의 종류 이름. 꽃핀이 올라설 창을 찾을 때 이 이름의 창은 건너뛴다.
pub const NOTE_CLASS: &str = "DesktopFlowerpinMemo";
const HOST_CLASS: &str = "DesktopFlowerpinMemoEdit";
const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_MOUSEWHEEL: u32 = 0x020A;
const TME_LEAVE: u32 = 2;
/// 여러 줄 입력 칸의 생김새: 여러 줄, 세로로 저절로 넘김, Enter로 줄 바꿈.
const EDIT_STYLE: u32 = 0x0004 | 0x0040 | 0x1000;
/// 한 줄 입력 칸의 생김새: 글이 길어지면 옆으로 넘긴다.
const LINE_STYLE: u32 = 0x0080;
const EM_SETSEL: u32 = 0x00B1;
const EM_SETLIMITTEXT: u32 = 0x00C5;
/// 입력 칸의 글이 바뀌었다는 알림.
const EN_CHANGE: usize = 0x0300;
/// Esc 키.
pub const KEY_ESCAPE: u32 = 0x1B;
/// Enter 키.
pub const KEY_ENTER: u32 = 0x0D;

/// 포스트잇 창에서 일어난 일. `x`, `y`는 창 안의 좌표(px)다.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    Press {
        hwnd: HWND,
        x: i32,
        y: i32,
    },
    Move {
        hwnd: HWND,
        x: i32,
        y: i32,
    },
    /// 버튼을 뗐다. 누르고 있던 것이 다른 까닭으로 끊겼을 때도 온다.
    Lift {
        hwnd: HWND,
    },
    /// 마우스가 창을 벗어났다.
    Leave {
        hwnd: HWND,
    },
    Double {
        hwnd: HWND,
        x: i32,
        y: i32,
    },
    Menu {
        hwnd: HWND,
    },
    /// 글을 적는 칸에서 다른 곳으로 갔거나 Esc를 눌렀다. `host`는 그 칸을 담은 창이다.
    EditDone {
        host: HWND,
    },
    /// 글을 적는 칸의 글이 바뀌었다.
    EditChanged {
        host: HWND,
    },
    /// 키보드를 받는 창(메모 목록)에서 키를 눌렀다.
    Key {
        hwnd: HWND,
        key: u32,
    },
    /// 마우스 휠을 굴렸다. 위로 굴리면 양수다.
    Wheel {
        hwnd: HWND,
        x: i32,
        y: i32,
        up: bool,
    },
}

/// 포스트잇 위에서 보여 줄 커서.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Cursor {
    Arrow,
    Hand,
    Move,
    /// 크기를 바꾸는 중(비스듬한 화살표).
    Resize,
}

thread_local! {
    static EVENTS: RefCell<Vec<Event>> = const { RefCell::new(Vec::new()) };
    static CURSOR: Cell<Cursor> = const { Cell::new(Cursor::Arrow) };
    /// 글을 적는 칸의 바탕 붓, 바탕색, 글자색. 칸이 떠 있는 동안에만 들어 있다.
    static EDIT_PAINT: Cell<(HBRUSH, COLORREF, COLORREF)> = const { Cell::new((null_mut(), 0, 0)) };
}

fn push(event: Event) {
    EVENTS.with(|events| events.borrow_mut().push(event));
}

pub fn take_events() -> Vec<Event> {
    EVENTS.with(|events| std::mem::take(&mut *events.borrow_mut()))
}

pub fn set_cursor(cursor: Cursor) {
    CURSOR.set(cursor);
}

/// 누르고 있던 것을 그만둔다. 끌 것이 아닌 곳을 눌렀을 때 쓴다.
pub fn stop_pressing() {
    unsafe { ReleaseCapture() };
}

fn register(name: &str, style: u32, procedure: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT) {
    let class = wide(name);
    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style,
        lpfnWndProc: Some(procedure),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: unsafe { GetModuleHandleW(null()) },
        hIcon: null_mut(),
        hCursor: null_mut(),
        hbrBackground: null_mut(),
        lpszMenuName: null(),
        lpszClassName: class.as_ptr(),
        hIconSm: null_mut(),
    };
    unsafe { RegisterClassExW(&wc) };
}

/// 창의 종류들을 등록한다. 포스트잇을 만들기 전에 부른다. 여러 번 불러도 한 번만 한다.
pub fn start() {
    static STARTED: Once = Once::new();
    STARTED.call_once(|| {
        register(NOTE_CLASS, CS_DBLCLKS, note_proc);
        register(HOST_CLASS, 0, host_proc);
    });
}

/// 모든 포스트잇이 함께 쓰는 그림 버퍼. 한 장을 그려 화면에 넘기고 나면 다음 장이 다시 쓴다.
pub struct Canvas {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u32,
    pub width: i32,
    pub height: i32,
}

impl Canvas {
    pub fn new(width: i32, height: i32) -> Canvas {
        let dc = unsafe { CreateCompatibleDC(null_mut()) };
        let mut canvas = Canvas { dc, bitmap: null_mut(), old: null_mut(), bits: null_mut(), width: 0, height: 0 };
        canvas.fit(width, height);
        canvas
    }

    /// 버퍼가 `width`×`height`보다 작으면 키운다. 포스트잇을 크게 늘렸을 때만 일어난다.
    pub fn fit(&mut self, width: i32, height: i32) {
        if width <= self.width && height <= self.height {
            return;
        }
        let (width, height) = (width.max(self.width), height.max(self.height));
        let mut info: BITMAPINFO = unsafe { std::mem::zeroed() };
        info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = width;
        info.bmiHeader.biHeight = -height;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        let mut bits: *mut c_void = null_mut();
        unsafe {
            let bitmap = CreateDIBSection(self.dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            let old = SelectObject(self.dc, bitmap);
            if self.bitmap.is_null() {
                self.old = old;
            } else {
                DeleteObject(self.bitmap);
            }
            self.bitmap = bitmap;
        }
        (self.bits, self.width, self.height) = (bits as *mut u32, width, height);
    }

    /// 한 줄이 `width`개이고 위에서 아래로 놓인다. 값은 미리 곱한 알파의 BGRA다.
    pub fn pixels(&mut self) -> &mut [u32] {
        unsafe { std::slice::from_raw_parts_mut(self.bits, (self.width * self.height) as usize) }
    }

    /// 버퍼의 왼쪽 위 `size`만큼을 창에 넘기고, 창을 화면의 `at`으로 옮긴다. 창의 크기도 `size`가 된다.
    pub fn present(&self, hwnd: HWND, at: (i32, i32), size: (i32, i32)) {
        let (position, size, source) =
            (POINT { x: at.0, y: at.1 }, SIZE { cx: size.0, cy: size.1 }, POINT { x: 0, y: 0 });
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let info = UPDATELAYEREDWINDOWINFO {
            cbSize: size_of::<UPDATELAYEREDWINDOWINFO>() as u32,
            hdcDst: null_mut(),
            pptDst: &position,
            psize: &size,
            hdcSrc: self.dc,
            pptSrc: &source,
            crKey: 0,
            pblend: &blend,
            dwFlags: ULW_ALPHA,
            prcDirty: null(),
        };
        unsafe { UpdateLayeredWindowIndirect(hwnd, &info) };
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}

/// 포스트잇 창을 만든다. 처음에는 보이지 않는다. 그림을 넘긴 뒤 [`show`]로 보여 준다.
pub fn create(at: (i32, i32), size: (i32, i32), top: bool) -> HWND {
    let class = wide(NOTE_CLASS);
    // 작업표시줄과 Alt+Tab에 나오지 않고, 눌러도 다른 창의 포커스를 빼앗지 않는다.
    let style = WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | if top { WS_EX_TOPMOST } else { 0 };
    unsafe {
        CreateWindowExW(
            style,
            class.as_ptr(),
            class.as_ptr(),
            WS_POPUP,
            at.0,
            at.1,
            size.0,
            size.1,
            null_mut(),
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        )
    }
}

pub fn show(hwnd: HWND) {
    unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
}

/// 누르면 앞으로 나오고 키보드를 받는 창을 만든다. 메모 목록이 이런 창이다.
/// 포스트잇과 같은 종류라서 그리는 법과 일어난 일을 받는 법은 같다. 작업표시줄에는 나오지 않는다.
pub fn create_active(at: (i32, i32), size: (i32, i32)) -> HWND {
    let class = wide(NOTE_CLASS);
    unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW,
            class.as_ptr(),
            class.as_ptr(),
            WS_POPUP,
            at.0,
            at.1,
            size.0,
            size.1,
            null_mut(),
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        )
    }
}

/// 키보드를 받는 창을 보여 주고 앞으로 가져온다. 확인용으로 도는 중이면 쓰던 창의 입력을 가져오지 않는다.
pub fn show_active(hwnd: HWND) {
    unsafe {
        if QUIET.load(Ordering::Relaxed) {
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            SetWindowPos(hwnd, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        } else {
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
        }
    }
}

pub fn destroy(hwnd: HWND) {
    unsafe { DestroyWindow(hwnd) };
}

/// 그림은 그대로 두고 창만 옮긴다.
pub fn move_to(hwnd: HWND, at: (i32, i32)) {
    unsafe { SetWindowPos(hwnd, null_mut(), at.0, at.1, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE) };
}

/// 창이 놓이는 층을 정한다. `top`이면 다른 창들보다 늘 위에, 아니면 다른 창들 뒤(바탕화면 바로 위)에 둔다.
pub fn set_layer(hwnd: HWND, top: bool) {
    let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
    unsafe {
        if top {
            SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, flags);
        } else {
            SetWindowPos(hwnd, HWND_NOTOPMOST, 0, 0, 0, 0, flags);
            SetWindowPos(hwnd, HWND_BOTTOM, 0, 0, 0, 0, flags);
        }
    }
}

/// 다른 창들 앞으로 한 번 가져온다. 늘 위에 두는 것은 아니어서, 다른 창을 누르면 그 창에 다시 가려진다.
pub fn raise(hwnd: HWND) {
    unsafe { SetWindowPos(hwnd, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
}

/// 화면의 한 점이 들어 있는 모니터의 작업 영역(왼쪽, 위, 오른쪽, 아래). `nearest`가 아니고 점이 어느 모니터에도
/// 없으면 `None`이다.
pub fn work_area(at: (i32, i32), nearest: bool) -> Option<(i32, i32, i32, i32)> {
    let flag = if nearest { MONITOR_DEFAULTTONEAREST } else { MONITOR_DEFAULTTONULL };
    let monitor = unsafe { MonitorFromPoint(POINT { x: at.0, y: at.1 }, flag) };
    if monitor.is_null() {
        return None;
    }
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = size_of::<MONITORINFO>() as u32;
    unsafe { GetMonitorInfoW(monitor, &mut info) };
    let RECT { left, top, right, bottom } = info.rcWork;
    Some((left, top, right, bottom))
}

/// 화면의 `at`에 메뉴를 띄우고 고른 것의 번호를 돌려준다. 고르지 않았으면 0이다.
/// 항목은 번호, 이름, 표시(체크)가 되어 있는지다. 번호가 0이면 나누는 줄이다.
pub fn menu(owner: HWND, at: (i32, i32), items: &[(u32, &str, bool)]) -> u32 {
    unsafe {
        let menu = CreatePopupMenu();
        for &(id, label, checked) in items {
            if id == 0 {
                AppendMenuW(menu, MF_SEPARATOR, 0, null());
            } else {
                let flags = MF_STRING | if checked { MF_CHECKED } else { 0 };
                AppendMenuW(menu, flags, id as usize, wide(label).as_ptr());
            }
        }
        // 주인 창을 앞으로 가져와야 메뉴 밖을 눌렀을 때 메뉴가 닫힌다.
        SetForegroundWindow(owner);
        let picked = TrackPopupMenuEx(menu, TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY, at.0, at.1, owner, null());
        PostMessageW(owner, WM_NULL, 0, 0);
        DestroyMenu(menu);
        picked as u32
    }
}

/// 글을 적는 칸. 포스트잇이 반듯하게 펴진 뒤, 글이 있던 자리에 종이와 같은 색으로 올라온다.
///
/// 포스트잇의 창에는 Windows의 입력 칸을 넣을 수 없어서(픽셀마다 투명도가 있는 창이라) 작은 창을 따로 띄운다.
/// 한글 입력은 Windows의 입력 칸이 하던 대로 한다.
pub struct Editor {
    host: HWND,
    edit: HWND,
    font: HFONT,
    brush: HBRUSH,
}

impl Editor {
    /// `rect`는 화면에서의 자리(왼쪽, 위, 너비, 높이), `colors`는 종이 색과 글자색(0x00BBGGRR)이다.
    pub fn open(
        owner: HWND,
        rect: (i32, i32, i32, i32),
        text: &str,
        colors: (COLORREF, COLORREF),
        font: HFONT,
        top: bool,
    ) -> Editor {
        Editor::build(owner, rect, text, colors, font, top, (EDIT_STYLE, 300))
    }

    /// 한 줄만 적는 칸. 메모 목록의 찾기 칸에 쓴다. 글자는 `limit`자까지다.
    pub fn open_line(
        owner: HWND,
        rect: (i32, i32, i32, i32),
        text: &str,
        colors: (COLORREF, COLORREF),
        font: HFONT,
        limit: usize,
    ) -> Editor {
        Editor::build(owner, rect, text, colors, font, false, (LINE_STYLE, limit))
    }

    /// `kind`는 입력 칸의 생김새와 적을 수 있는 글자 수다.
    fn build(
        owner: HWND,
        rect: (i32, i32, i32, i32),
        text: &str,
        colors: (COLORREF, COLORREF),
        font: HFONT,
        top: bool,
        kind: (u32, usize),
    ) -> Editor {
        let brush = unsafe { CreateSolidBrush(colors.0) };
        EDIT_PAINT.set((brush, colors.0, colors.1));
        let (host_class, edit_class) = (wide(HOST_CLASS), wide("EDIT"));
        let instance = unsafe { GetModuleHandleW(null()) };
        let layer = WS_EX_TOOLWINDOW | if top { WS_EX_TOPMOST } else { 0 };
        let content = wide(&text.replace('\n', "\r\n"));
        unsafe {
            // 포스트잇 창을 주인으로 두면 늘 그 바로 위에 놓인다.
            let host = CreateWindowExW(
                layer,
                host_class.as_ptr(),
                host_class.as_ptr(),
                WS_POPUP,
                rect.0,
                rect.1,
                rect.2,
                rect.3,
                owner,
                null_mut(),
                instance,
                null(),
            );
            let style = WS_CHILD | WS_VISIBLE | kind.0;
            let edit = CreateWindowExW(
                0,
                edit_class.as_ptr(),
                content.as_ptr(),
                style,
                0,
                0,
                rect.2,
                rect.3,
                host,
                null_mut(),
                instance,
                null(),
            );
            SendMessageW(edit, WM_SETFONT, font as WPARAM, 1);
            SendMessageW(edit, EM_SETLIMITTEXT, kind.1, 0);
            // 커서를 글의 끝에 둔다.
            SendMessageW(edit, EM_SETSEL, 32_767, 32_767);
            if QUIET.load(Ordering::Relaxed) {
                ShowWindow(host, SW_SHOWNOACTIVATE);
            } else {
                ShowWindow(host, SW_SHOW);
                SetForegroundWindow(host);
                SetFocus(edit);
            }
            Editor { host, edit, font, brush }
        }
    }

    /// 글을 적는 칸의 창.
    pub fn edit(&self) -> HWND {
        self.edit
    }

    /// 글을 적는 칸을 담은 창. 이 창에서 일어난 일인지 가리는 데 쓴다.
    pub fn host(&self) -> HWND {
        self.host
    }

    /// 적힌 글을 바꾼다. 글이 바뀌었다는 알림이 따라온다.
    pub fn set_text(&self, text: &str) {
        unsafe { SetWindowTextW(self.edit, wide(text).as_ptr()) };
    }

    /// 적힌 글을 모두 고른다. 여러 줄 입력 칸은 Ctrl+A를 스스로 처리하지 않아서 따로 해 준다.
    pub fn select_all(&self) {
        unsafe { SendMessageW(self.edit, EM_SETSEL, 0, -1) };
    }

    pub fn text(&self) -> String {
        let length = unsafe { GetWindowTextLengthW(self.edit) }.max(0) as usize;
        let mut buffer = vec![0u16; length + 1];
        let length = unsafe { GetWindowTextW(self.edit, buffer.as_mut_ptr(), buffer.len() as i32) }.max(0) as usize;
        String::from_utf16_lossy(&buffer[..length]).replace("\r\n", "\n")
    }
}

impl Drop for Editor {
    fn drop(&mut self) {
        EDIT_PAINT.set((null_mut(), 0, 0));
        unsafe {
            DestroyWindow(self.host);
            DeleteObject(self.brush);
            DeleteObject(self.font);
        }
    }
}

unsafe extern "system" fn note_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let at = || ((lparam & 0xFFFF) as i16 as i32, ((lparam >> 16) & 0xFFFF) as i16 as i32);
    match message {
        // 포스트잇과 판은 눌러도 쓰던 창의 포커스를 빼앗지 않는다. 메모 목록은 보통 창처럼 앞으로 나온다.
        WM_MOUSEACTIVATE if unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32 & WS_EX_NOACTIVATE != 0 => {
            MA_NOACTIVATE as LRESULT
        }
        WM_KEYDOWN => {
            push(Event::Key { hwnd, key: wparam as u32 });
            0
        }
        // Alt+F4처럼 닫으라는 말은 Esc를 누른 것으로 친다. 창을 없애는 것은 앱이 한다.
        WM_CLOSE => {
            push(Event::Key { hwnd, key: KEY_ESCAPE });
            0
        }
        // 누른 동안은 커서가 창 밖으로 나가도 움직임을 계속 받는다. 끌어서 옮기려면 그래야 한다.
        WM_LBUTTONDOWN => {
            unsafe { SetCapture(hwnd) };
            let (x, y) = at();
            push(Event::Press { hwnd, x, y });
            0
        }
        WM_LBUTTONDBLCLK => {
            let (x, y) = at();
            push(Event::Double { hwnd, x, y });
            0
        }
        WM_MOUSEMOVE => {
            let mut track = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            unsafe { TrackMouseEvent(&mut track) };
            let (x, y) = at();
            push(Event::Move { hwnd, x, y });
            0
        }
        WM_LBUTTONUP => {
            if unsafe { GetCapture() } == hwnd {
                unsafe { ReleaseCapture() };
            }
            0
        }
        WM_CAPTURECHANGED => {
            push(Event::Lift { hwnd });
            0
        }
        WM_MOUSELEAVE => {
            push(Event::Leave { hwnd });
            0
        }
        WM_MOUSEWHEEL => {
            // 휠의 좌표는 화면 기준이다. 창 안의 좌표로 바꾼다.
            let mut point = POINT { x: (lparam & 0xFFFF) as i16 as i32, y: ((lparam >> 16) & 0xFFFF) as i16 as i32 };
            unsafe { ScreenToClient(hwnd, &mut point) };
            push(Event::Wheel { hwnd, x: point.x, y: point.y, up: ((wparam >> 16) & 0xFFFF) as i16 > 0 });
            0
        }
        WM_RBUTTONUP => {
            push(Event::Menu { hwnd });
            0
        }
        WM_SETCURSOR => {
            let shape = match CURSOR.get() {
                Cursor::Arrow => IDC_ARROW,
                Cursor::Hand => IDC_HAND,
                Cursor::Move => IDC_SIZEALL,
                Cursor::Resize => IDC_SIZENWSE,
            };
            unsafe { SetCursor(LoadCursorW(null_mut(), shape)) };
            1
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

/// 글을 적는 칸을 담은 창.
unsafe extern "system" fn host_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        // 입력 칸의 바탕을 종이 색으로 칠한다.
        WM_CTLCOLOREDIT => {
            let (brush, paper, ink) = EDIT_PAINT.get();
            unsafe {
                SetBkColor(wparam as HDC, paper);
                SetTextColor(wparam as HDC, ink);
            }
            brush as LRESULT
        }
        // 다른 창으로 갔다(low word가 0이면 비활성). 적기를 마친다.
        WM_ACTIVATE if wparam & 0xFFFF == 0 => {
            push(Event::EditDone { host: hwnd });
            0
        }
        // 여러 줄 입력 칸은 Esc를 누르면 주인 창에 닫으라고 알린다.
        WM_CLOSE => {
            push(Event::EditDone { host: hwnd });
            0
        }
        // 입력 칸이 글이 바뀌었다고 알려 왔다(high word가 알림의 종류).
        WM_COMMAND if (wparam >> 16) & 0xFFFF == EN_CHANGE => {
            push(Event::EditChanged { host: hwnd });
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}
