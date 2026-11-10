//! 트레이 아이콘과 메뉴, 그리고 앱이 하나만 실행되게 하는 일.
//!
//! 꽃핀 창은 눌러도 포커스를 받지 않게 만들어져 있어서 메뉴의 주인이 될 수 없다. 그래서 화면에 보이지
//! 않는 창을 하나 따로 두고, 트레이 아이콘의 알림과 메뉴를 이 창이 맡는다.
//!
//! 아이콘 그림은 파일로 두지 않는다. 실행할 때 그림 팩의 한 장에서 만든다. 그래서 공식 빌드는
//! 꽃핀 얼굴이, 이 저장소만 받아 빌드한 것은 대체 그림이 아이콘이 된다.

use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS, DeleteObject,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, NOTIFYICONIDENTIFIER,
    Shell_NotifyIconGetRect, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu,
    DestroyWindow, FindWindowW, GetSystemMetrics, HICON, ICONINFO, MF_STRING, PostMessageW, RegisterClassExW,
    RegisterWindowMessageW, SM_CXSMICON, SetForegroundWindow, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    TrackPopupMenuEx, WM_APP, WM_CONTEXTMENU, WM_NULL, WM_RBUTTONUP, WNDCLASSEXW, WS_POPUP,
};

use crate::geom::Rect;
use crate::win::{cursor_position, fatal, wide};

const CLASS_NAME: &str = "DesktopFlowerpinMain";
/// 앱이 이미 실행 중인지 알아보는 데 쓰는 이름. 로그인한 사용자마다 따로다.
const INSTANCE_NAME: &str = "Local\\DesktopFlowerpin.Running";
/// 트레이 아이콘에서 일이 있을 때 Windows가 보내 주는 메시지와, 아이콘의 번호.
const WM_TRAY: u32 = WM_APP + 1;
const ICON_ID: u32 = 1;
const TIP: &str = "DesktopFlowerpin";

/// 메뉴에서 고를 수 있는 것.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuItem {
    Quit = 1,
}

/// 트레이 쪽에서 일어난 일.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayEvent {
    /// 트레이 아이콘을 오른쪽 클릭했다. 좌표는 메뉴를 띄울 화면 위치다.
    Menu { x: i32, y: i32 },
    /// 작업표시줄이 새로 만들어졌다(탐색기가 다시 시작됨). 아이콘을 다시 등록해야 한다.
    TaskbarCreated,
    /// 앱을 한 번 더 켰다. 새로 켠 쪽은 이것만 알리고 끝난다.
    Hello,
}

thread_local! {
    static EVENTS: RefCell<Vec<TrayEvent>> = const { RefCell::new(Vec::new()) };
}

/// 지난번에 꺼낸 뒤로 쌓인 일을 모두 꺼낸다.
pub fn take_events() -> Vec<TrayEvent> {
    EVENTS.with(|events| std::mem::take(&mut *events.borrow_mut()))
}

/// 탐색기가 작업표시줄을 새로 만들었을 때 모든 창에 돌리는 메시지.
fn taskbar_created() -> u32 {
    static MESSAGE: OnceLock<u32> = OnceLock::new();
    *MESSAGE.get_or_init(|| unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) })
}

/// 새로 켠 앱이 먼저 실행 중이던 앱에게 보내는 메시지.
fn hello() -> u32 {
    static MESSAGE: OnceLock<u32> = OnceLock::new();
    *MESSAGE.get_or_init(|| unsafe { RegisterWindowMessageW(wide("DesktopFlowerpin.Hello").as_ptr()) })
}

/// "이 앱은 내가 실행 중이다"라는 표시. 갖고 있는 동안 다른 실행은 [`claim`]에 실패한다.
pub struct Instance(HANDLE);

impl Drop for Instance {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

/// 앱이 하나만 실행되게 한다. 이미 실행 중이면 그쪽에 알리고 `None`을 돌려준다.
pub fn claim() -> Option<Instance> {
    let handle = unsafe { CreateMutexW(null(), 0, wide(INSTANCE_NAME).as_ptr()) };
    let already = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    if handle.is_null() {
        // 표시를 만들지 못했을 때는 막지 않는다. 두 개가 뜨는 편이 아예 못 켜는 것보다 낫다.
        return Some(Instance(null_mut()));
    }
    if already {
        unsafe {
            let running = FindWindowW(wide(CLASS_NAME).as_ptr(), null());
            if !running.is_null() {
                PostMessageW(running, hello(), 0, 0);
            }
            CloseHandle(handle);
        }
        return None;
    }
    Some(Instance(handle))
}

/// 트레이 아이콘의 한 변 길이(px). 화면 배율에 따라 16, 20, 24처럼 달라진다.
pub fn icon_size() -> i32 {
    unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16)
}

pub struct Tray {
    hwnd: HWND,
    icon: HICON,
}

impl Tray {
    /// 트레이 아이콘을 만든다. `pixels`는 한 변이 `size`인 그림(미리 곱한 알파의 BGRA)이다.
    pub fn new(pixels: &[u32], size: i32) -> Tray {
        let class = wide(CLASS_NAME);
        let instance = unsafe { GetModuleHandleW(null()) };
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: null_mut(),
            hCursor: null_mut(),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class.as_ptr(),
            hIconSm: null_mut(),
        };
        if unsafe { RegisterClassExW(&wc) } == 0 {
            fatal("트레이 창의 종류를 등록하지 못했습니다.");
        }
        // 화면에 보이지 않는 창. 보여 주지 않으므로 크기도 자리도 없다.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                null_mut(),
                null_mut(),
                instance,
                null(),
            )
        };
        if hwnd.is_null() {
            fatal("트레이 창을 만들지 못했습니다.");
        }
        // 등록해 두어야 탐색기가 다시 시작됐을 때 알 수 있다.
        taskbar_created();
        let tray = Tray { hwnd, icon: make_icon(pixels, size) };
        tray.add();
        tray
    }

    fn data(&self) -> NOTIFYICONDATAW {
        let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = self.hwnd;
        data.uID = ICON_ID;
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = WM_TRAY;
        data.hIcon = self.icon;
        for (slot, unit) in data.szTip.iter_mut().zip(wide(TIP)) {
            *slot = unit;
        }
        data
    }

    /// 아이콘을 트레이에 등록한다. 탐색기가 다시 시작되면 사라지므로 그때 다시 부른다.
    pub fn add(&self) {
        // 탐색기가 아직 준비되지 않았으면 실패한다. 준비되면 `TaskbarCreated`가 오므로 그때 다시 등록된다.
        unsafe { Shell_NotifyIconW(NIM_ADD, &self.data()) };
    }

    /// 아이콘이 트레이에 놓인 자리. 등록되어 있지 않으면 `None`이다.
    pub fn icon_rect(&self) -> Option<Rect> {
        let mut id: NOTIFYICONIDENTIFIER = unsafe { std::mem::zeroed() };
        id.cbSize = size_of::<NOTIFYICONIDENTIFIER>() as u32;
        id.hWnd = self.hwnd;
        id.uID = ICON_ID;
        let mut r = unsafe { std::mem::zeroed() };
        (unsafe { Shell_NotifyIconGetRect(&id, &mut r) } >= 0)
            .then(|| Rect::new(r.left, r.top, r.right - r.left, r.bottom - r.top))
    }

    /// 화면의 `(x, y)`에 메뉴를 띄우고, 고른 것을 돌려준다. 메뉴가 닫힐 때까지 돌아오지 않는다.
    pub fn pick(&self, x: i32, y: i32) -> Option<MenuItem> {
        unsafe {
            let menu = CreatePopupMenu();
            if menu.is_null() {
                return None;
            }
            AppendMenuW(menu, MF_STRING, MenuItem::Quit as usize, wide("끝내기").as_ptr());
            // 메뉴를 띄우기 전에 주인 창을 앞으로 가져오고, 닫힌 뒤에 빈 메시지를 보낸다.
            // 이렇게 하지 않으면 메뉴 밖을 눌러도 메뉴가 닫히지 않는다.
            SetForegroundWindow(self.hwnd);
            let flags = TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY;
            let picked = TrackPopupMenuEx(menu, flags, x, y, self.hwnd, null());
            PostMessageW(self.hwnd, WM_NULL, 0, 0);
            DestroyMenu(menu);
            (picked == MenuItem::Quit as i32).then_some(MenuItem::Quit)
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &self.data());
            DestroyIcon(self.icon);
            DestroyWindow(self.hwnd);
        }
    }
}

/// 미리 곱한 알파의 BGRA 그림으로 아이콘을 만든다.
fn make_icon(pixels: &[u32], size: i32) -> HICON {
    let mut info: BITMAPINFO = unsafe { std::mem::zeroed() };
    info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth = size;
    info.bmiHeader.biHeight = -size;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB;
    unsafe {
        let mut bits: *mut c_void = null_mut();
        let color = CreateDIBSection(null_mut(), &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if color.is_null() || bits.is_null() {
            return null_mut();
        }
        std::slice::from_raw_parts_mut(bits as *mut u32, (size * size) as usize).copy_from_slice(pixels);
        // 알파가 있는 그림이라 가림 그림은 쓰이지 않지만, 있어야 아이콘이 만들어진다. 한 줄은 16비트 단위다.
        let mask_bits = vec![0u8; ((size + 15) / 16 * 2 * size) as usize];
        let mask = CreateBitmap(size, size, 1, 1, mask_bits.as_ptr() as *const c_void);
        let icon_info = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&icon_info);
        DeleteObject(color);
        DeleteObject(mask);
        icon
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let push = |event: TrayEvent| EVENTS.with(|events| events.borrow_mut().push(event));
    match msg {
        WM_TRAY => {
            // 아이콘에서 일어난 마우스 일이 lparam에 들어 있다.
            if matches!(lparam as u32, WM_RBUTTONUP | WM_CONTEXTMENU) {
                let (x, y) = cursor_position();
                push(TrayEvent::Menu { x, y });
            }
            0
        }
        _ if msg == taskbar_created() => {
            push(TrayEvent::TaskbarCreated);
            0
        }
        _ if msg == hello() => {
            push(TrayEvent::Hello);
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
