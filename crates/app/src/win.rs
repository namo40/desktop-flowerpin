//! Win32를 부를 때 여러 곳에서 쓰는 작은 도구.

use std::ptr::{null, null_mut};

use crate::when::days_from_civil;

use windows_sys::Win32::Foundation::{HMODULE, HWND, POINT};
use windows_sys::Win32::System::LibraryLoader::{
    LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW, SetDefaultDllDirectories,
};
use windows_sys::Win32::System::SystemInformation::{GetLocalTime, GetTickCount};
use windows_sys::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows_sys::Win32::UI::Shell::{
    QUNS_BUSY, QUNS_NOT_PRESENT, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState,
    ShellExecuteW,
};
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

/// 이 프로그램이 켜진 뒤에 불러오는 DLL을 Windows의 시스템 폴더에서만 찾게 한다. 가장 먼저 부른다.
///
/// Windows는 이름만으로 DLL을 찾을 때 실행 파일이 있는 폴더를 먼저 본다. 그 폴더에 같은 이름의 파일을 둔 사람이
/// 제 코드를 이 프로그램 안에서 돌릴 수 있게 되므로 막는다. 이 프로그램이 직접 부르는 것뿐 아니라, Windows의
/// 부품들이 일하다가 이름만으로 불러오는 DLL에도 적용된다.
///
/// 실행 파일이 켜질 때 바로 불러오는 DLL은 이 함수가 불리기 전에 이미 올라와 있다. 그쪽은 링크할 때 넣는
/// 설정이 맡는다(`build.rs`).
pub fn search_system_folder_only() {
    // 실패해도(아주 오래된 Windows) 앱은 돌아가므로 결과는 보지 않는다.
    unsafe { SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32) };
}

/// Windows의 시스템 폴더에 있는 DLL을 불러온다. 그 DLL이 끌어오는 다른 DLL도 시스템 폴더에서만 찾는다.
/// 불러오지 못했으면 빈 값이다. 프로그램이 끝날 때까지 쓰므로 내려놓지 않는다.
pub fn system_library(name: &str) -> HMODULE {
    unsafe { LoadLibraryExW(wide(name).as_ptr(), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) }
}

/// 지금 화면을 누가 쓰고 있는지.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    /// 평소대로다.
    Free,
    /// 전체 화면 프로그램(게임, 발표, 전체 화면 영상)이 앞에 떠 있다.
    Busy,
    /// 화면이 잠겨 있거나 화면 보호기가 떠 있다.
    Absent,
}

/// 지금 화면을 누가 쓰고 있는지를 Windows에 묻는다. Windows가 알림을 띄워도 되는지 정할 때 쓰는 것과 같은 답이다.
pub fn screen() -> Screen {
    let mut state = 0;
    if unsafe { SHQueryUserNotificationState(&mut state) } < 0 {
        return Screen::Free;
    }
    match state {
        QUNS_BUSY | QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE => Screen::Busy,
        QUNS_NOT_PRESENT => Screen::Absent,
        _ => Screen::Free,
    }
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

/// 지금 이 PC의 벽시계 시각. 1970-01-01 00:00부터 흐른 초로 센다(시간대를 따지지 않는다).
pub fn local_seconds() -> i64 {
    let mut time = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut time) };
    let days = days_from_civil(time.wYear as i64, time.wMonth as i64, time.wDay as i64);
    days * 86_400 + time.wHour as i64 * 3_600 + time.wMinute as i64 * 60 + time.wSecond as i64
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

/// 화면에 보이는 이 프로그램의 이름. 창의 제목, 트레이 아이콘, 오류를 알리는 창에 쓴다.
/// 파일과 폴더의 이름은 따로다(`DesktopFlowerpin`).
pub const APP_NAME: &str = "데스크탑 꽃핀";

/// 더 진행할 수 없는 오류를 알리고 끝낸다. 콘솔이 없는 프로그램이라 메시지 상자로 보여 준다.
pub fn fatal(message: &str) -> ! {
    let text = wide(message);
    let title = wide(APP_NAME);
    unsafe { MessageBoxW(null_mut(), text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR) };
    std::process::exit(1);
}
