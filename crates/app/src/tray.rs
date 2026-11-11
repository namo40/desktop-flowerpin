//! 트레이 아이콘과 메뉴, 그리고 앱이 하나만 실행되게 하는 일.
//!
//! 꽃핀 창은 눌러도 포커스를 받지 않게 만들어져 있어서 메뉴의 주인이 될 수 없다. 그래서 화면에 보이지
//! 않는 창을 하나 따로 두고, 트레이 아이콘의 알림과 메뉴를 이 창이 맡는다.
//!
//! 아이콘 그림은 파일로 두지 않는다. 실행할 때 그림 팩의 한 장에서 만든다. 그래서 공식 빌드는
//! 꽃핀 얼굴이, 이 저장소만 받아 빌드한 것은 대체 그림이 아이콘이 된다.

use std::cell::{Cell, RefCell};
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
use windows_sys::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_LARGE_ICON, NIIF_RESPECT_QUIET_TIME, NIIF_USER, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NIN_BALLOONUSERCLICK, NOTIFYICONDATAW, NOTIFYICONIDENTIFIER, Shell_NotifyIconGetRect,
    Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu,
    DestroyWindow, FindWindowW, GetSystemMetrics, HICON, HMENU, ICONINFO, KillTimer, MF_DISABLED, MF_GRAYED, MF_POPUP,
    MF_SEPARATOR, MF_STRING, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND, PBT_APMSUSPEND, PostMessageW,
    RegisterClassExW, RegisterWindowMessageW, SM_CXICON, SM_CXSMICON, SetForegroundWindow, SetTimer, TPM_NONOTIFY,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx, WM_APP, WM_CONTEXTMENU, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_NULL,
    WM_POWERBROADCAST, WM_RBUTTONUP, WM_TIMER, WM_WTSSESSION_CHANGE, WNDCLASSEXW, WS_POPUP, WTS_SESSION_LOCK,
    WTS_SESSION_UNLOCK,
};

use crate::actor::Show;
use crate::geom::Rect;
use crate::links::LINKS;
use crate::win::{cursor_position, fatal, wide};

const CLASS_NAME: &str = "DesktopFlowerpinMain";
/// 앱이 이미 실행 중인지 알아보는 데 쓰는 이름. 로그인한 사용자마다 따로다.
const INSTANCE_NAME: &str = "Local\\DesktopFlowerpin.Running";
/// 트레이 아이콘에서 일이 있을 때 Windows가 보내 주는 메시지와, 아이콘의 번호.
const WM_TRAY: u32 = WM_APP + 1;
const ICON_ID: u32 = 1;
const TIP: &str = crate::win::APP_NAME;
/// 메뉴나 물음 창이 떠 있는 동안 꽃핀을 움직이게 하는 타이머의 번호와 간격(ms).
const TICK_TIMER: usize = 1;
const TICK_EVERY_MS: u32 = 15;

/// "잠깐 숨기기"에서 고를 수 있는 것. 이름과, 숨겨 둘 시간(초)이다. 시간이 없으면 다시 보이라고 할 때까지다.
pub const HIDES: [(&str, Option<u64>); 3] = [("10분", Some(600)), ("1시간", Some(3600)), ("다시 켤 때까지", None)];

/// 메뉴에서 고를 수 있는 것.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuItem {
    Quit,
    Settings,
    /// 방송 보러 가기. 방송 중일 때만 메뉴에 있다.
    Watch,
    /// 새 메모를 포스트잇으로 붙인다.
    MemoNew,
    /// 포스트잇을 모두 다른 창들 앞으로 가져온다.
    MemoShow,
    /// 포스트잇을 다시 줄 세운다.
    MemoTidy,
    /// 메모를 한데 모아 보는 창을 연다.
    MemoList,
    /// 꽃핀을 하나 더 초대한다.
    CallOne,
    /// 더 부른 꽃핀을 모두 돌려보낸다.
    SendAll,
    /// 꽃핀 하나를 돌려보낸다. 메뉴에는 없고, 확인용 스크립트가 번호로 고른다.
    SendThis,
    /// 꽃핀을 잠깐 숨긴다. [`HIDES`]의 몇 번째인지다.
    Hide(usize),
    /// 숨겨 둔 꽃핀을 다시 보인다. 숨겨 둔 동안에만 메뉴에 있다.
    Unhide,
    /// 바로 가기. [`LINKS`]의 몇 번째인지다.
    Link(usize),
    /// 꽃핀에게 시켜서 볼 행동. 메뉴에는 없고, 확인용 스크립트가 번호로 고른다.
    Show(Show),
}

impl MenuItem {
    /// 메뉴 항목의 번호. 0은 "아무것도 고르지 않음"이라 쓰지 않는다.
    pub fn id(self) -> u32 {
        match self {
            MenuItem::Quit => 1,
            MenuItem::Settings => 2,
            MenuItem::Watch => 3,
            MenuItem::MemoNew => 4,
            MenuItem::MemoShow => 5,
            MenuItem::MemoTidy => 6,
            MenuItem::CallOne => 7,
            MenuItem::SendAll => 8,
            MenuItem::SendThis => 9,
            MenuItem::Hide(index) => 10 + index as u32,
            MenuItem::Unhide => 13,
            MenuItem::MemoList => 14,
            MenuItem::Link(index) => 100 + index as u32,
            MenuItem::Show(show) => 200 + Show::ALL.iter().position(|&other| other == show).unwrap_or(0) as u32,
        }
    }

    pub fn from_id(id: u32) -> Option<MenuItem> {
        match id {
            1 => Some(MenuItem::Quit),
            2 => Some(MenuItem::Settings),
            3 => Some(MenuItem::Watch),
            4 => Some(MenuItem::MemoNew),
            5 => Some(MenuItem::MemoShow),
            6 => Some(MenuItem::MemoTidy),
            7 => Some(MenuItem::CallOne),
            8 => Some(MenuItem::SendAll),
            9 => Some(MenuItem::SendThis),
            10..=12 => Some(MenuItem::Hide((id - 10) as usize)),
            13 => Some(MenuItem::Unhide),
            14 => Some(MenuItem::MemoList),
            100.. if ((id - 100) as usize) < LINKS.len() => Some(MenuItem::Link((id - 100) as usize)),
            200.. if ((id - 200) as usize) < Show::ALL.len() => Some(MenuItem::Show(Show::ALL[(id - 200) as usize])),
            _ => None,
        }
    }
}

/// 트레이 쪽에서 일어난 일.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayEvent {
    /// 트레이 아이콘을 오른쪽 클릭했다. 좌표는 메뉴를 띄울 화면 위치다.
    Menu { x: i32, y: i32 },
    /// 트레이 아이콘을 왼쪽 버튼으로 클릭했다. 두 번 클릭할 때도 그 앞에 한 번 온다.
    Click,
    /// 트레이 아이콘을 두 번 클릭했다. 설정 창을 연다.
    Open,
    /// 작업표시줄이 새로 만들어졌다(탐색기가 다시 시작됨). 아이콘을 다시 등록해야 한다.
    TaskbarCreated,
    /// 앱을 한 번 더 켰다. 새로 켠 쪽은 이것만 알리고 끝난다.
    Hello,
    /// Windows 알림을 클릭했다.
    ToastClicked,
    /// 화면을 잠갔거나(`true`) 잠금을 풀었다.
    Locked(bool),
    /// PC가 절전에 들어가거나(`true`) 절전에서 돌아왔다.
    Asleep(bool),
}

thread_local! {
    static EVENTS: RefCell<Vec<TrayEvent>> = const { RefCell::new(Vec::new()) };
    /// 메뉴나 물음 창이 떠 있는 동안 타이머가 불러 줄 것.
    static TICK: Cell<Option<fn()>> = const { Cell::new(None) };
}

/// `show`가 돌아올 때까지 `tick`을 자주 불러 준다.
///
/// 메뉴와 물음 창은 닫힐 때까지 돌아오지 않고 그 안에서 창 메시지를 돌린다. 그동안 메인 루프가 멈추므로
/// 꽃핀도 멈춘다. 그래서 `owner` 창에 타이머를 걸어, 그 안에서도 꽃핀을 계속 움직이게 한다.
pub fn while_modal<T>(owner: HWND, tick: fn(), show: impl FnOnce() -> T) -> T {
    TICK.set(Some(tick));
    unsafe { SetTimer(owner, TICK_TIMER, TICK_EVERY_MS, None) };
    let result = show();
    unsafe { KillTimer(owner, TICK_TIMER) };
    TICK.set(None);
    result
}

/// 메뉴를 띄울 때의 상황. 넣을 항목과 흐리게 할 항목이 여기에 달려 있다.
pub struct MenuState<'a> {
    /// 방송 중일 때의 방송 제목과 방송 정보 한 줄. 있으면 "● 방송 중"을 넣는다.
    pub live: Option<&'a (String, String)>,
    /// 꽃핀을 더 부를 수 있는가.
    pub can_call: bool,
    /// 돌려보낼 꽃핀이 있는가.
    pub can_send: bool,
    /// 꽃핀을 잠깐 숨겨 둔 중이다. "잠깐 숨기기" 자리에 "다시 보이기"를 넣는다.
    pub hidden: bool,
}

/// 화면의 `(x, y)`에 메뉴를 띄우고, 고른 것을 돌려준다. 메뉴가 닫힐 때까지 돌아오지 않는다.
/// `owner`는 메뉴의 주인이 될 창([`Tray::hwnd`])이다.
pub fn pick(owner: HWND, x: i32, y: i32, state: &MenuState) -> Option<MenuItem> {
    let menu = build(state)?;
    unsafe {
        // 메뉴를 띄우기 전에 주인 창을 앞으로 가져오고, 닫힌 뒤에 빈 메시지를 보낸다.
        // 이렇게 하지 않으면 메뉴 밖을 눌러도 메뉴가 닫히지 않는다.
        SetForegroundWindow(owner);
        let flags = TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY;
        let picked = TrackPopupMenuEx(menu, flags, x, y, owner, null());
        PostMessageW(owner, WM_NULL, 0, 0);
        // 딸린 메뉴는 큰 메뉴와 함께 없어진다.
        DestroyMenu(menu);
        MenuItem::from_id(picked as u32)
    }
}

/// 메뉴를 만든다. 다 쓰면 `DestroyMenu`로 없애야 한다.
fn build(state: &MenuState) -> Option<HMENU> {
    unsafe {
        let menu = CreatePopupMenu();
        if let Some((title, details)) = state.live {
            let info = CreatePopupMenu();
            if !menu.is_null() && !info.is_null() {
                // 위 두 줄은 읽기만 하는 줄이다. 메뉴에서 &는 밑줄 표시라서, 글자 그대로 보이게 겹쳐 적는다.
                for line in [title, details] {
                    let text = wide(&line.replace('&', "&&"));
                    AppendMenuW(info, MF_STRING | MF_DISABLED, 0, text.as_ptr());
                }
                AppendMenuW(info, MF_SEPARATOR, 0, null());
                AppendMenuW(info, MF_STRING, MenuItem::Watch.id() as usize, wide("보러 가기").as_ptr());
                AppendMenuW(menu, MF_POPUP, info as usize, wide("● 방송 중").as_ptr());
                AppendMenuW(menu, MF_SEPARATOR, 0, null());
            }
        }
        let links = CreatePopupMenu();
        let memos = CreatePopupMenu();
        if menu.is_null() || links.is_null() || memos.is_null() {
            return None;
        }
        let add = |menu, item: MenuItem, label: &str| {
            AppendMenuW(menu, MF_STRING, item.id() as usize, wide(label).as_ptr());
        };
        // 지금 할 수 없는 것은 흐리게 넣는다.
        let add_if = |menu, possible: bool, item: MenuItem, label: &str| {
            let flags = if possible { MF_STRING } else { MF_STRING | MF_GRAYED };
            AppendMenuW(menu, flags, item.id() as usize, wide(label).as_ptr());
        };
        for (index, link) in LINKS.iter().enumerate() {
            add(links, MenuItem::Link(index), link.name);
        }
        add(memos, MenuItem::MemoNew, "새 메모");
        add(memos, MenuItem::MemoList, "메모 목록...");
        add(memos, MenuItem::MemoShow, "모두 앞으로 가져오기");
        add(memos, MenuItem::MemoTidy, "다시 줄 세우기");
        add_if(menu, state.can_call, MenuItem::CallOne, "꽃핀 초대하기");
        add_if(menu, state.can_send, MenuItem::SendAll, "모두 보내기");
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        AppendMenuW(menu, MF_POPUP, memos as usize, wide("메모").as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        if state.hidden {
            add(menu, MenuItem::Unhide, "다시 보이기");
        } else {
            let hides = CreatePopupMenu();
            for (index, (name, _)) in HIDES.iter().enumerate() {
                add(hides, MenuItem::Hide(index), name);
            }
            AppendMenuW(menu, MF_POPUP, hides as usize, wide("잠깐 숨기기").as_ptr());
        }
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        AppendMenuW(menu, MF_POPUP, links as usize, wide("바로 가기").as_ptr());
        // 이름 끝의 "..."은 창이 열린다는 뜻이다.
        add(menu, MenuItem::Settings, "설정...");
        add(menu, MenuItem::Quit, "끝내기");
        Some(menu)
    }
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

/// Windows 알림에 넣는 그림의 한 변 길이(px). 100% 배율에서 48이다.
pub fn toast_icon_size() -> i32 {
    unsafe { GetSystemMetrics(SM_CXICON) }.max(32) * 3 / 2
}

pub struct Tray {
    hwnd: HWND,
    icon: HICON,
    /// Windows 알림에 넣는 큰 그림. 없으면 트레이 아이콘이 대신 쓰인다.
    toast_icon: HICON,
    /// 아이콘에 마우스를 올리면 보이는 글.
    tip: String,
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
        // 화면을 잠그고 푸는 것을 알려 달라고 한다. 받아 주지 않아도 앱은 그대로 돈다.
        unsafe { WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) };
        let tray = Tray { hwnd, icon: make_icon(pixels, size), toast_icon: null_mut(), tip: TIP.to_string() };
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
        put(&mut data.szTip, &self.tip);
        data
    }

    /// 아이콘 그림과, 마우스를 올리면 보이는 글을 바꾼다. 글을 비우면 앱 이름이 보인다.
    pub fn set(&mut self, pixels: &[u32], size: i32, tip: &str) {
        let old = std::mem::replace(&mut self.icon, make_icon(pixels, size));
        self.tip = if tip.is_empty() { TIP.to_string() } else { tip.to_string() };
        unsafe {
            Shell_NotifyIconW(NIM_MODIFY, &self.data());
            DestroyIcon(old);
        }
    }

    /// Windows 알림에 넣을 큰 그림을 정한다. 트레이 아이콘을 늘려 쓰면 흐려 보여서 따로 둔다.
    pub fn set_toast_icon(&mut self, pixels: &[u32], size: i32) {
        let old = std::mem::replace(&mut self.toast_icon, make_icon(pixels, size));
        if !old.is_null() {
            unsafe { DestroyIcon(old) };
        }
    }

    /// Windows 알림을 띄운다. 트레이 아이콘에 딸린 알림이라 앱을 따로 등록하지 않아도 된다.
    /// 클릭하면 [`TrayEvent::ToastClicked`]가 온다. Windows가 받아 주었으면 `true`를 돌려준다.
    pub fn notify(&self, title: &str, body: &str) -> bool {
        let mut data = self.data();
        data.uFlags |= NIF_INFO;
        data.dwInfoFlags = NIIF_USER | NIIF_RESPECT_QUIET_TIME;
        if !self.toast_icon.is_null() {
            data.dwInfoFlags |= NIIF_LARGE_ICON;
            data.hBalloonIcon = self.toast_icon;
        }
        put(&mut data.szInfoTitle, title);
        put(&mut data.szInfo, body);
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) != 0 }
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

    /// 메뉴와 물음 창의 주인이 되는, 화면에 보이지 않는 창.
    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &self.data());
            WTSUnRegisterSessionNotification(self.hwnd);
            DestroyIcon(self.icon);
            if !self.toast_icon.is_null() {
                DestroyIcon(self.toast_icon);
            }
            DestroyWindow(self.hwnd);
        }
    }
}

/// 정해진 길이의 칸에 글을 넣는다. 끝에 0이 들어갈 자리를 남기고, 넘치는 글은 자른다.
fn put(slot: &mut [u16], text: &str) {
    let room = slot.len() - 1;
    let mut units: Vec<u16> = text.encode_utf16().take(room).collect();
    // 두 칸짜리 글자의 앞 칸만 남았으면 그것도 뺀다.
    if units.last().is_some_and(|unit| (0xD800..0xDC00).contains(unit)) {
        units.pop();
    }
    slot[..units.len()].copy_from_slice(&units);
    slot[units.len()] = 0;
}

/// 아이콘 그림의 오른쪽 위에 빨간 점을 찍은 그림을 돌려준다. 방송 중이라는 표시다.
///
/// `pixels`는 한 변이 `size`인 그림(미리 곱한 알파의 BGRA)이다. 점에는 흰 테를 둘러 어떤 그림 위에서도 보이게 한다.
pub fn with_dot(pixels: &[u32], size: i32) -> Vec<u32> {
    let mut marked = pixels.to_vec();
    let radius = size as f64 * 0.24;
    let center = (size as f64 - radius - 0.5, radius + 0.5);
    for y in 0..size {
        for x in 0..size {
            let distance = (x as f64 + 0.5 - center.0).hypot(y as f64 + 0.5 - center.1);
            // 테의 바깥 가장자리와, 점과 테의 경계를 반 픽셀씩 부드럽게 한다.
            let cover = (radius + 0.5 - distance).clamp(0.0, 1.0);
            if cover <= 0.0 {
                continue;
            }
            let red = (radius * 0.72 + 0.5 - distance).clamp(0.0, 1.0);
            let color = (255.0, 255.0 - 207.0 * red, 255.0 - 191.0 * red);
            let slot = &mut marked[(y * size + x) as usize];
            let under = |shift: u32| ((*slot >> shift) & 0xFF) as f64 * (1.0 - cover);
            let part = |value: f64, shift: u32| ((value * cover + under(shift)).round() as u32).min(255) << shift;
            *slot = part(255.0, 24) | part(color.0, 16) | part(color.1, 8) | part(color.2, 0);
        }
    }
    marked
}

/// 미리 곱한 알파의 BGRA 그림으로 아이콘을 만든다. 다 쓰면 `DestroyIcon`으로 없애야 한다.
pub fn make_icon(pixels: &[u32], size: i32) -> HICON {
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
            match lparam as u32 {
                WM_RBUTTONUP | WM_CONTEXTMENU => {
                    let (x, y) = cursor_position();
                    push(TrayEvent::Menu { x, y });
                }
                WM_LBUTTONUP => push(TrayEvent::Click),
                WM_LBUTTONDBLCLK => push(TrayEvent::Open),
                NIN_BALLOONUSERCLICK => push(TrayEvent::ToastClicked),
                _ => {}
            }
            0
        }
        WM_TIMER if wparam == TICK_TIMER => {
            if let Some(tick) = TICK.get() {
                tick();
            }
            0
        }
        WM_WTSSESSION_CHANGE => {
            match wparam as u32 {
                WTS_SESSION_LOCK => push(TrayEvent::Locked(true)),
                WTS_SESSION_UNLOCK => push(TrayEvent::Locked(false)),
                _ => {}
            }
            0
        }
        WM_POWERBROADCAST => {
            match wparam as u32 {
                PBT_APMSUSPEND => push(TrayEvent::Asleep(true)),
                PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND => push(TrayEvent::Asleep(false)),
                _ => {}
            }
            // 절전에 들어가도 된다고 답한다.
            1
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_live_dot_sits_in_the_top_right_corner() {
        let size = 16;
        let plain = vec![0xFF20_4060u32; (size * size) as usize];
        let marked = with_dot(&plain, size);
        let at = |x: i32, y: i32| marked[(y * size + x) as usize];
        // 점의 가운데는 빨갛고, 그 둘레는 희다. 왼쪽 아래는 그대로다.
        let (red, green) = ((at(12, 3) >> 16) & 0xFF, (at(12, 3) >> 8) & 0xFF);
        assert!(red > 240 && green < 80, "{:08X}", at(12, 3));
        let (red, green, blue) = ((at(8, 4) >> 16) & 0xFF, (at(8, 4) >> 8) & 0xFF, at(8, 4) & 0xFF);
        assert!(red > 240 && green > 220 && blue > 220, "{:08X}", at(8, 4));
        assert_eq!((at(2, 13), at(0, 0), at(15, 15)), (0xFF20_4060, 0xFF20_4060, 0xFF20_4060));
        // 투명한 그림 위에서도 점은 불투명하다.
        let clear = with_dot(&vec![0u32; (size * size) as usize], size);
        assert_eq!(clear[(3 * size + 12) as usize] >> 24, 255);
    }

    #[test]
    fn text_is_cut_to_fit_its_slot() {
        let mut slot = [0xFFFFu16; 8];
        put(&mut slot, "방송 중");
        assert_eq!(String::from_utf16_lossy(&slot[..4]), "방송 중");
        assert_eq!(slot[4], 0);
        // 넘치면 자르고, 끝에는 늘 0이 온다.
        put(&mut slot, "아주 긴 제목입니다");
        assert_eq!((String::from_utf16_lossy(&slot[..7]), slot[7]), ("아주 긴 제목".to_string(), 0));
        // 두 칸짜리 글자가 반만 남으면 통째로 뺀다.
        put(&mut slot, "abcdef😀");
        assert_eq!((String::from_utf16_lossy(&slot[..6]), slot[6]), ("abcdef".to_string(), 0));
    }

    /// 메뉴의 줄을 위에서부터 읽는다. 줄마다 이름과, 흐린지다. 나누는 줄은 이름이 "-"다.
    fn lines(menu: HMENU) -> Vec<(String, bool)> {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetMenuItemCount, GetMenuState, GetMenuStringW, MF_BYPOSITION,
        };
        let count = unsafe { GetMenuItemCount(menu) }.max(0) as u32;
        (0..count)
            .map(|at| {
                let mut text = [0u16; 64];
                let length = unsafe { GetMenuStringW(menu, at, text.as_mut_ptr(), text.len() as i32, MF_BYPOSITION) };
                let state = unsafe { GetMenuState(menu, at, MF_BYPOSITION) };
                let name = String::from_utf16_lossy(&text[..length.max(0) as usize]);
                // 나누는 줄은 늘 흐린 것으로 읽힌다. 흐린 줄로 치지 않는다.
                if state & MF_SEPARATOR != 0 { ("-".to_string(), false) } else { (name, state & MF_GRAYED != 0) }
            })
            .collect()
    }

    #[test]
    fn the_menu_lists_what_can_be_done_now() {
        let state = |can_call, can_send| MenuState { live: None, can_call, can_send, hidden: false };
        let names = |state: &MenuState| {
            let menu = build(state).expect("메뉴");
            let read = lines(menu);
            unsafe { DestroyMenu(menu) };
            read.into_iter().map(|(name, grayed)| if grayed { format!("({name})") } else { name }).collect::<Vec<_>>()
        };
        // 보낼 꽃핀이 없으면 "모두 보내기"가 흐리다.
        assert_eq!(
            names(&state(true, false)),
            ["꽃핀 초대하기", "(모두 보내기)", "-", "메모", "-", "잠깐 숨기기", "-", "바로 가기", "설정...", "끝내기"]
        );
        // 숨겨 둔 동안에는 "잠깐 숨기기" 자리에 "다시 보이기"가 온다.
        let hidden = MenuState { hidden: true, ..state(false, false) };
        assert_eq!(names(&hidden)[4..], ["-", "다시 보이기", "-", "바로 가기", "설정...", "끝내기"]);
        // 더 초대할 수 없으면 "꽃핀 초대하기"가 흐리다.
        assert_eq!(names(&state(false, true))[..2], ["(꽃핀 초대하기)", "모두 보내기"]);
        // 방송 중이면 방송 정보가 맨 위에 들어간다.
        let live = ("제목 & 제목".to_string(), "게임 · 시청자 10명".to_string());
        let on_air = MenuState { live: Some(&live), ..state(true, true) };
        assert_eq!(names(&on_air)[..3], ["● 방송 중", "-", "꽃핀 초대하기"]);
    }

    #[test]
    fn menu_numbers_are_unique_and_read_back() {
        let memos = [MenuItem::MemoNew, MenuItem::MemoShow, MenuItem::MemoTidy, MenuItem::MemoList];
        let mut items = vec![MenuItem::Quit, MenuItem::Settings, MenuItem::Watch];
        items.extend(memos);
        items.extend([MenuItem::CallOne, MenuItem::SendAll, MenuItem::SendThis, MenuItem::Unhide]);
        items.extend((0..HIDES.len()).map(MenuItem::Hide));
        items.extend((0..LINKS.len()).map(MenuItem::Link));
        items.extend(Show::ALL.map(MenuItem::Show));
        for (n, item) in items.iter().enumerate() {
            // 0은 "아무것도 고르지 않음"이다.
            assert_ne!(item.id(), 0);
            assert_eq!(MenuItem::from_id(item.id()), Some(*item));
            assert!(items[..n].iter().all(|other| other.id() != item.id()), "{item:?}");
        }
        for id in [0, 15, 99, 100 + LINKS.len() as u32, 199, 200 + Show::ALL.len() as u32] {
            assert_eq!(MenuItem::from_id(id), None, "{id}");
        }
    }
}
