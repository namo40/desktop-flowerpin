//! Win32를 부를 때 여러 곳에서 쓰는 작은 도구.

use std::ffi::c_void;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MB_ICONERROR, MB_OK, MSG, MessageBoxW, PM_REMOVE, PeekMessageW, SPI_GETWORKAREA,
    SystemParametersInfoW, TranslateMessage, WM_QUIT,
};

use crate::geom::Rect;

/// Win32에 넘길 수 있게, 끝에 0을 붙인 UTF-16 문자열로 바꾼다.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// 모니터마다 화면 배율이 달라도 흐려지지 않게, 배율을 앱이 직접 다루겠다고 알린다.
pub fn enable_per_monitor_dpi() {
    // 실패해도(오래된 Windows 10) 앱은 돌아가므로 결과는 보지 않는다.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}

/// 주 모니터에서 작업표시줄을 뺀 범위.
pub fn primary_work_area() -> Rect {
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    let ok = unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut r as *mut RECT as *mut c_void, 0) };
    if ok == 0 {
        fatal("작업 영역을 읽지 못했습니다.");
    }
    Rect::new(r.left, r.top, r.right - r.left, r.bottom - r.top)
}

/// 쌓여 있는 창 메시지를 모두 처리한다. 끝내라는 메시지(`WM_QUIT`)를 받으면 `false`를 돌려준다.
pub fn pump_messages() -> bool {
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    while unsafe { PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) } != 0 {
        if msg.message == WM_QUIT {
            return false;
        }
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    true
}

/// 더 진행할 수 없는 오류를 알리고 끝낸다. 콘솔이 없는 프로그램이라 메시지 상자로 보여 준다.
pub fn fatal(message: &str) -> ! {
    let text = wide(message);
    let title = wide("DesktopFlowerpin");
    unsafe { MessageBoxW(null_mut(), text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR) };
    std::process::exit(1);
}
