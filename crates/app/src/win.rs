//! Win32를 부를 때 여러 곳에서 쓰는 작은 도구.

use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, IDOK, MB_ICONERROR, MB_ICONQUESTION, MB_OK, MB_OKCANCEL, MessageBoxW, SW_SHOWNORMAL,
};

/// Win32에 넘길 수 있게, 끝에 0을 붙인 UTF-16 문자열로 바꾼다.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// 모니터마다 화면 배율이 달라도 흐려지지 않게, 배율을 앱이 직접 다루겠다고 알린다.
pub fn enable_per_monitor_dpi() {
    // 실패해도(오래된 Windows 10) 앱은 돌아가므로 결과는 보지 않는다.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}

/// 사용자가 마우스나 키보드를 마지막으로 쓴 뒤 흐른 시간(초). 알 수 없으면 0이다.
pub fn idle_seconds() -> f64 {
    let mut info = LASTINPUTINFO { cbSize: size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    if unsafe { GetLastInputInfo(&mut info) } == 0 {
        return 0.0;
    }
    // 둘 다 Windows를 켠 뒤 흐른 밀리초다. 49일마다 0으로 돌아가므로 넘침을 감안해 뺀다.
    unsafe { GetTickCount() }.wrapping_sub(info.dwTime) as f64 / 1000.0
}

/// 마우스 커서의 자리(화면 좌표).
pub fn cursor_position() -> (i32, i32) {
    let mut point = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut point) };
    (point.x, point.y)
}

/// 주소는 기본 브라우저로, 폴더는 탐색기로 연다.
pub fn open(target: &str) {
    let (verb, target) = (wide("open"), wide(target));
    unsafe { ShellExecuteW(null_mut(), verb.as_ptr(), target.as_ptr(), null(), null(), SW_SHOWNORMAL) };
}

/// `owner` 창 위에 예·아니요를 묻는 창을 띄운다. 답할 때까지 돌아오지 않는다.
pub fn confirm(owner: HWND, title: &str, question: &str) -> bool {
    let (title, question) = (wide(title), wide(question));
    unsafe { MessageBoxW(owner, question.as_ptr(), title.as_ptr(), MB_OKCANCEL | MB_ICONQUESTION) == IDOK }
}

/// 더 진행할 수 없는 오류를 알리고 끝낸다. 콘솔이 없는 프로그램이라 메시지 상자로 보여 준다.
pub fn fatal(message: &str) -> ! {
    let text = wide(message);
    let title = wide("DesktopFlowerpin");
    unsafe { MessageBoxW(null_mut(), text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR) };
    std::process::exit(1);
}
