//! 대화 상자로 만든 창(설정 창, 메모 창)이 함께 쓰는 도구.

use std::ffi::c_void;
use std::fmt::Write as _;
use std::path::Path;
use std::ptr::null_mut;
use std::sync::Once;

use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject,
    GetDC, MapWindowPoints, ReleaseDC, SelectObject,
};
use windows_sys::Win32::Storage::Xps::PrintWindow;
use windows_sys::Win32::UI::Controls::{
    ICC_DATE_CLASSES, ICC_STANDARD_CLASSES, ICC_TAB_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GW_CHILD, GW_HWNDNEXT, GetClassNameW, GetDlgCtrlID, GetWindow, GetWindowRect, GetWindowTextW, SWP_NOACTIVATE,
    SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
};

/// 탭 컨트롤과 날짜·시각 고르는 칸을 쓸 수 있게 공용 컨트롤을 준비시킨다. 여러 번 불러도 한 번만 한다.
pub fn prepare() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let classes = INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_TAB_CLASSES | ICC_STANDARD_CLASSES | ICC_DATE_CLASSES,
        };
        unsafe { InitCommonControlsEx(&classes) };
    });
}

/// 리소스 번호를 Win32가 받는 모양으로 바꾼다.
pub fn resource(id: u16) -> *const u16 {
    id as usize as *const u16
}

/// `child`가 `parent` 안에서 차지하는 범위.
pub fn rect_in(parent: HWND, child: HWND) -> RECT {
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe {
        GetWindowRect(child, &mut r);
        MapWindowPoints(null_mut(), parent, &mut r as *mut RECT as *mut POINT, 2);
    }
    r
}

/// 창의 가운데를 화면의 `center`에 맞춘다.
pub fn center_on(hwnd: HWND, center: (i32, i32)) {
    let mut frame = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe { GetWindowRect(hwnd, &mut frame) };
    let (x, y) = (center.0 - (frame.right - frame.left) / 2, center.1 - (frame.bottom - frame.top) / 2);
    unsafe { SetWindowPos(hwnd, null_mut(), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE) };
}

/// `parent`에 놓인 컨트롤마다 종류, 번호, 좌표(px), 글을 한 줄씩 적는다. 만드는 동안 정렬을 확인하는 데 쓴다.
pub fn describe_children(parent: HWND, text: &mut String) {
    let mut child = unsafe { GetWindow(parent, GW_CHILD) };
    while !child.is_null() {
        let r = rect_in(parent, child);
        let (mut class, mut label) = ([0u16; 32], [0u16; 96]);
        let (class_len, label_len) = unsafe {
            (
                GetClassNameW(child, class.as_mut_ptr(), 32) as usize,
                GetWindowTextW(child, label.as_mut_ptr(), 96) as usize,
            )
        };
        let _ = writeln!(
            text,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            String::from_utf16_lossy(&class[..class_len]),
            unsafe { GetDlgCtrlID(child) },
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            String::from_utf16_lossy(&label[..label_len]).replace(['\r', '\n'], " ")
        );
        child = unsafe { GetWindow(child, GW_HWNDNEXT) };
    }
}

/// 창을 그림(BMP)으로 저장한다. 다른 창에 가려 있어도 된다. 만드는 동안 눈으로 확인하는 데 쓴다.
pub fn shot(hwnd: HWND, path: &Path) {
    let mut frame = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe { GetWindowRect(hwnd, &mut frame) };
    let (width, height) = (frame.right - frame.left, frame.bottom - frame.top);
    let mut info: BITMAPINFO = unsafe { std::mem::zeroed() };
    info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth = width;
    info.bmiHeader.biHeight = -height;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB;
    unsafe {
        let screen = GetDC(null_mut());
        let mut bits: *mut c_void = null_mut();
        let bitmap = CreateDIBSection(screen, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        let memory = CreateCompatibleDC(screen);
        if !bitmap.is_null() && !bits.is_null() {
            let old = SelectObject(memory, bitmap);
            // 2는 화면에 그려진 그대로(테마 포함) 찍으라는 뜻이다(PW_RENDERFULLCONTENT).
            PrintWindow(hwnd, memory, 2);
            let pixels = std::slice::from_raw_parts(bits as *const u8, (width * height * 4) as usize);
            let header = std::slice::from_raw_parts(&info.bmiHeader as *const BITMAPINFOHEADER as *const u8, 40);
            let mut file = Vec::with_capacity(54 + pixels.len());
            file.extend_from_slice(b"BM");
            file.extend_from_slice(&(54 + pixels.len() as u32).to_le_bytes());
            file.extend_from_slice(&0u32.to_le_bytes());
            file.extend_from_slice(&54u32.to_le_bytes());
            file.extend_from_slice(header);
            file.extend_from_slice(pixels);
            let _ = std::fs::write(path, file);
            SelectObject(memory, old);
            DeleteObject(bitmap);
        }
        DeleteDC(memory);
        ReleaseDC(null_mut(), screen);
    }
}
