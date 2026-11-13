//! 꽃핀 한 명이 쓰는 창.
//!
//! 화면에 보이는 것은 32비트(미리 곱한 알파) 그림 한 장이다. 테두리도 제목 줄도 없고,
//! 완전히 투명한 픽셀을 누르면 클릭이 뒤에 있는 창으로 넘어간다. 그림은 창 버퍼에 그린 뒤
//! [`CharacterWindow::present`]로 화면에 넘긴다.
//!
//! 창에서 일어난 일(누르기, 끌기, 모니터가 바뀜)은 창 메시지를 받는 자리에서 처리하지 않고
//! [`WindowEvent`]로 적어 두기만 한다. 메인 루프가 [`take_events`]로 꺼내 처리한다.

use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::Once;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CreateCompatibleDC,
    CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, HBITMAP, HDC, HGDIOBJ, SelectObject,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetCapture, ReleaseCapture, SetCapture};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GW_HWNDPREV, GetWindow, HTCLIENT, HWND_NOTOPMOST, HWND_TOP,
    HWND_TOPMOST, IDC_HAND, IsWindowVisible, LoadCursorW, MA_NOACTIVATE, RegisterClassExW, SW_HIDE, SW_SHOWNOACTIVATE,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetCursor, SetWindowPos, ShowWindow, ULW_ALPHA, UPDATELAYEREDWINDOWINFO,
    UpdateLayeredWindowIndirect, WM_CAPTURECHANGED, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_RBUTTONUP, WM_SETCURSOR, WM_SETTINGCHANGE, WNDCLASSEXW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::geom::Rect;
use crate::win::{cursor_position, fatal, wide};

pub const CLASS_NAME: &str = "DesktopFlowerpinCharacter";

/// 꽃핀 창에서 일어난 일. 좌표는 화면 기준이다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowEvent {
    /// 꽃핀을 왼쪽 버튼으로 눌렀다.
    Press { hwnd: HWND, x: i32, y: i32 },
    /// 누른 채로 움직였다.
    Move { hwnd: HWND, x: i32, y: i32 },
    /// 누르지 않은 채로 꽃핀 위에서 움직였다. 투명한 곳 위에서는 오지 않는다.
    Hover { hwnd: HWND, x: i32, y: i32 },
    /// 버튼을 뗐다. 누르고 있던 것이 다른 까닭으로 끊겼을 때도 온다.
    Lift { hwnd: HWND },
    /// 오른쪽 버튼으로 눌렀다 뗐다.
    Menu { hwnd: HWND, x: i32, y: i32 },
    /// 모니터 구성, 작업 영역, 화면 배율이 바뀌었다.
    StageChanged,
}

thread_local! {
    static EVENTS: RefCell<Vec<WindowEvent>> = const { RefCell::new(Vec::new()) };
}

fn push(event: WindowEvent) {
    EVENTS.with(|events| events.borrow_mut().push(event));
}

/// 지난번에 꺼낸 뒤로 쌓인 일을 모두 꺼낸다.
pub fn take_events() -> Vec<WindowEvent> {
    EVENTS.with(|events| std::mem::take(&mut *events.borrow_mut()))
}

/// 누르고 있던 것을 그만둔다. 잡을 수 없는 꽃핀을 눌렀을 때 쓴다.
pub fn stop_pressing() {
    unsafe { ReleaseCapture() };
}

pub struct CharacterWindow {
    hwnd: HWND,
    mem_dc: HDC,
    bitmap: HBITMAP,
    old_bitmap: HGDIOBJ,
    bits: *mut u32,
    width: i32,
    height: i32,
    /// 창 왼쪽 위가 놓일 화면 좌표. 다음 `present` 때 반영된다.
    pos: (i32, i32),
    /// 마지막으로 화면에 넘긴 위치. 아직 한 번도 넘기지 않았으면 `None`.
    presented_pos: Option<(i32, i32)>,
}

impl CharacterWindow {
    /// 크기가 `width`×`height`이고 왼쪽 위가 화면 `(x, y)`에 오는 창을 만든다. 처음에는 보이지 않는다.
    /// `topmost`면 다른 창들보다 늘 위에 있다.
    pub fn new(width: i32, height: i32, x: i32, y: i32, topmost: bool) -> CharacterWindow {
        register_class();
        let class = wide(CLASS_NAME);
        // 작업표시줄과 Alt+Tab에 나오지 않고, 눌러도 다른 창의 포커스를 빼앗지 않는다.
        let ex_style = WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | if topmost { WS_EX_TOPMOST } else { 0 };
        let hwnd = unsafe {
            CreateWindowExW(
                ex_style,
                class.as_ptr(),
                class.as_ptr(),
                WS_POPUP,
                x,
                y,
                width,
                height,
                null_mut(),
                null_mut(),
                GetModuleHandleW(null()),
                null(),
            )
        };
        if hwnd.is_null() {
            fatal("꽃핀 창을 만들지 못했습니다.");
        }

        // 위에서 아래로 놓이는 32비트 창 버퍼. 픽셀은 미리 곱한 알파의 BGRA다.
        let mut info: BITMAPINFO = unsafe { std::mem::zeroed() };
        info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = width;
        info.bmiHeader.biHeight = -height;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        let mut bits: *mut c_void = null_mut();
        let mem_dc = unsafe { CreateCompatibleDC(null_mut()) };
        let bitmap = unsafe { CreateDIBSection(mem_dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0) };
        if mem_dc.is_null() || bitmap.is_null() || bits.is_null() {
            fatal("꽃핀 창의 그림 버퍼를 만들지 못했습니다.");
        }
        let old_bitmap = unsafe { SelectObject(mem_dc, bitmap) };

        CharacterWindow {
            hwnd,
            mem_dc,
            bitmap,
            old_bitmap,
            bits: bits as *mut u32,
            width,
            height,
            pos: (x, y),
            presented_pos: None,
        }
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    pub fn width(&self) -> i32 {
        self.width
    }

    pub fn height(&self) -> i32 {
        self.height
    }

    /// 창이 화면에서 차지하는 범위.
    pub fn screen_rect(&self) -> Rect {
        Rect::new(self.pos.0, self.pos.1, self.width, self.height)
    }

    /// 다른 창들보다 늘 위에 있게 하거나, 보통 창처럼 다른 창 뒤로 갈 수 있게 한다.
    pub fn set_topmost(&self, topmost: bool) {
        let after = if topmost { HWND_TOPMOST } else { HWND_NOTOPMOST };
        unsafe { SetWindowPos(self.hwnd, after, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
    }

    /// 보통 창들 가운데 맨 앞으로 올린다. 포커스는 빼앗지 않는다.
    pub fn raise(&self) {
        unsafe { SetWindowPos(self.hwnd, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
    }

    /// 창을 숨기거나 다시 보인다. 다시 보일 때도 다른 창의 포커스를 빼앗지 않는다.
    pub fn set_visible(&self, visible: bool) {
        unsafe { ShowWindow(self.hwnd, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE }) };
    }

    pub fn is_visible(&self) -> bool {
        unsafe { IsWindowVisible(self.hwnd) != 0 }
    }

    /// 이 창을 `front` 창의 바로 뒤에 둔다. 다른 창들 사이에서의 높이는 `front`를 따라간다.
    pub fn place_behind(&self, front: HWND) {
        unsafe { SetWindowPos(self.hwnd, front, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
    }

    /// 화면에서 이 창 바로 앞에 놓인 창. 맨 앞이면 빈 값이다.
    pub fn just_ahead(&self) -> HWND {
        unsafe { GetWindow(self.hwnd, GW_HWNDPREV) }
    }

    /// 창의 왼쪽 위를 화면의 `(x, y)`로 옮긴다. 다음 `present` 때 화면에 반영된다.
    pub fn move_to(&mut self, x: i32, y: i32) {
        self.pos = (x, y);
    }

    /// 창 버퍼. 한 줄이 `width`개이고 위에서 아래로 놓인다. 값은 미리 곱한 알파의 BGRA(0xAARRGGBB)다.
    pub fn pixels(&mut self) -> &mut [u32] {
        // 버퍼는 창과 함께 만들어져 창이 없어질 때까지 살아 있고, 크기는 width × height로 고정이다.
        unsafe { std::slice::from_raw_parts_mut(self.bits, (self.width * self.height) as usize) }
    }

    /// 창 버퍼를 읽기만 할 때 쓴다.
    pub fn shown(&self) -> &[u32] {
        unsafe { std::slice::from_raw_parts(self.bits, (self.width * self.height) as usize) }
    }

    /// 창 버퍼에서 `dirty` 범위를 화면에 넘기고, 자리가 바뀌었으면 함께 옮긴다.
    ///
    /// 화면에 넘기는 호출은 넘기는 픽셀 수보다 부를 때마다 드는 비용이 크다. 그래서 그림과 위치를
    /// 한 번에 넘기고, 바뀐 것이 없으면 부르지 않는다.
    ///
    /// 그림을 넘길 때는 창 크기(`psize`)를 꼭 함께 넘긴다. 크기가 그대로라고 `psize`를 비우면
    /// 호출은 성공했다고 돌려주면서 화면의 그림은 바뀌지 않는다(Windows 11에서 확인).
    /// 실제로 바뀌는지는 `--check-screen`으로 본다.
    pub fn present(&mut self, dirty: Rect) {
        let first = self.presented_pos.is_none();
        let moved = self.presented_pos != Some(self.pos);
        let dirty = if first { Rect::new(0, 0, self.width, self.height) } else { dirty.clip(self.width, self.height) };
        if !moved && dirty.is_empty() {
            return;
        }

        let pos = POINT { x: self.pos.0, y: self.pos.1 };
        let size = SIZE { cx: self.width, cy: self.height };
        let src = POINT { x: 0, y: 0 };
        let dirty_rect = RECT { left: dirty.x, top: dirty.y, right: dirty.right(), bottom: dirty.bottom() };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let draw = !dirty.is_empty();
        let info = UPDATELAYEREDWINDOWINFO {
            cbSize: size_of::<UPDATELAYEREDWINDOWINFO>() as u32,
            hdcDst: null_mut(),
            pptDst: if moved { &pos } else { null() },
            // 그림이 그대로면 원본과 크기를 넘기지 않는다. 그러면 창의 모양은 두고 위치만 바뀐다.
            psize: if draw { &size } else { null() },
            hdcSrc: if draw { self.mem_dc } else { null_mut() },
            pptSrc: if draw { &src } else { null() },
            crKey: 0,
            pblend: &blend,
            dwFlags: ULW_ALPHA,
            prcDirty: if draw && !first { &dirty_rect } else { null() },
        };
        unsafe { UpdateLayeredWindowIndirect(self.hwnd, &info) };

        if first {
            unsafe { ShowWindow(self.hwnd, SW_SHOWNOACTIVATE) };
        }
        self.presented_pos = Some(self.pos);
    }
}

impl Drop for CharacterWindow {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.hwnd);
            SelectObject(self.mem_dc, self.old_bitmap);
            DeleteObject(self.bitmap);
            DeleteDC(self.mem_dc);
        }
    }
}

fn register_class() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let class = wide(CLASS_NAME);
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(wnd_proc),
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
        if unsafe { RegisterClassExW(&wc) } == 0 {
            fatal("꽃핀 창의 종류를 등록하지 못했습니다.");
        }
    });
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        // 꽃핀을 눌러도 쓰던 창이 포커스를 잃지 않게 한다.
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        // 누른 동안은 커서가 창 밖으로 나가도 움직임을 계속 받는다. 끌어서 옮기려면 그래야 한다.
        WM_LBUTTONDOWN => {
            unsafe { SetCapture(hwnd) };
            let (x, y) = cursor_position();
            push(WindowEvent::Press { hwnd, x, y });
            0
        }
        WM_MOUSEMOVE => {
            let (x, y) = cursor_position();
            if unsafe { GetCapture() } == hwnd {
                push(WindowEvent::Move { hwnd, x, y });
            } else {
                push(WindowEvent::Hover { hwnd, x, y });
            }
            0
        }
        // 뗐다는 것은 아래의 WM_CAPTURECHANGED 한 곳에서만 알린다.
        WM_LBUTTONUP => {
            unsafe { ReleaseCapture() };
            0
        }
        WM_CAPTURECHANGED => {
            push(WindowEvent::Lift { hwnd });
            0
        }
        WM_RBUTTONUP => {
            let (x, y) = cursor_position();
            push(WindowEvent::Menu { hwnd, x, y });
            0
        }
        // 꽃핀 위에서는 손 모양 커서를 보여 준다.
        WM_SETCURSOR if (lparam & 0xFFFF) as u32 == HTCLIENT => {
            unsafe { SetCursor(LoadCursorW(null_mut(), IDC_HAND)) };
            1
        }
        WM_DISPLAYCHANGE | WM_SETTINGCHANGE | WM_DPICHANGED => {
            push(WindowEvent::StageChanged);
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
