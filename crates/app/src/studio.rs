//! 방송 촬영용 창.
//!
//! 단색 바탕의 보통 창 안에 꽃핀들이 산다. 방송 프로그램(OBS 등)의 창 캡처로 이 창을 잡고 크로마 키로
//! 바탕색을 빼면, 방송 화면에 꽃핀만 올라간다. 창은 다른 창에 가려져도 되지만, 최소화하면 잡히지 않는다.
//!
//! 창이 열려 있는 동안은 꽃핀들의 무대가 창 안으로 옮겨 온다. 바탕화면의 꽃핀들은 그 자리에 멈춰 숨고, 앱이
//! 다루는 꽃핀들(초대, 보내기, 다 같이, 머리 위 단추 등)이 창 안에서 지낸다. 창의 안쪽이 그 꽃핀들의 모니터이고,
//! 아래 끝(조금 위)이 바닥, 양옆이 벽이다. 꽃핀은 화면에 내보내지 않는 그림 버퍼에 그려지고([`Look::offscreen`]),
//! 이 창이 그것들을 앞뒤 차례대로 바탕색 위에 옮겨 그린다.
//!
//! 창에서 일어난 일은 창 메시지를 받는 자리에서 처리하지 않고 [`StudioEvent`]로 적어 두기만 한다. 누르기, 끌기,
//! 문지르기, 꽃핀 위의 오른쪽 클릭은 앱이 그 자리의 꽃핀을 찾아 바탕화면 꽃핀과 똑같이 다룬다. 빈 곳을 오른쪽
//! 클릭하면 꽃핀을 초대하고 돌려보내고 다 같이 시키고 바탕색을 고르는 메뉴가 뜬다.
//!
//! [`Look::offscreen`]: crate::character::Look::offscreen

use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::Once;
use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, BitBlt, ClientToScreen, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, EndPaint, GetDC, HBITMAP, HDC, HGDIOBJ, PAINTSTRUCT, ReleaseDC, SRCCOPY,
    SelectObject,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::LibraryLoader::GetProcAddress;
use windows_sys::Win32::UI::Controls::Dialogs::{CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetCapture, ReleaseCapture, SetCapture};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu,
    DestroyWindow, GA_ROOT, GetAncestor, GetClientRect, GetWindowRect, HICON, HMENU, IDC_ARROW, IsIconic, LoadCursorW,
    MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, MINMAXINFO, PostMessageW, RegisterClassExW,
    SW_SHOWNOACTIVATE, SW_SHOWNORMAL, SendMessageW, SetForegroundWindow, ShowWindow, TPM_NONOTIFY, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, TrackPopupMenuEx, WM_CAPTURECHANGED, WM_CLOSE, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOVE, WM_NULL, WM_PAINT, WM_RBUTTONUP, WM_SETICON, WM_SIZE, WNDCLASSEXW,
    WS_OVERLAPPEDWINDOW, WindowFromPoint,
};

use crate::character_window::CharacterWindow;
use crate::geom::Rect;
use crate::stage::{Monitor, Stage};
use crate::tray::{CALLS, TOGETHER};
use crate::win::{fatal, system_library, wide};

/// 창의 제목. 방송 프로그램에서 창을 고를 때 이 이름으로 보인다.
pub const TITLE: &str = "데스크탑 꽃핀 - 방송 촬영용";
const CLASS_NAME: &str = "DesktopFlowerpinStudio";

/// 고를 수 있는 바탕색(이름, 0xRRGGBB). 크로마 키로 빼기 좋은 색들이다. 처음은 초록이다.
pub const COLORS: [(&str, u32); 3] = [("초록", 0x00FF00), ("파랑", 0x0000FF), ("자홍", 0xFF00FF)];
/// 처음 열 때 창 안쪽의 크기와, 줄일 수 있는 가장 작은 크기(배율 100% 기준 px).
const FIRST_SIZE: (i32, i32) = (640, 360);
const MIN_SIZE: (i32, i32) = (240, 240);
/// 바닥을 창 아래 끝에서 이만큼(배율 100% 기준 px) 올린다. 발밑 그림자가 잘리지 않게 한다.
const FLOOR_GAP: i32 = 14;

/// 메뉴 항목의 번호.
const MENU_COLOR: u32 = 1;
const MENU_CUSTOM: u32 = 9;
const MENU_CLOSE: u32 = 10;
const MENU_CALL: u32 = 11;
const MENU_SEND: u32 = 12;
const MENU_MANY: u32 = 20;
const MENU_FILL: u32 = 29;
const MENU_TOGETHER: u32 = 30;

/// 창에서 일어난 일. 좌표는 화면 기준이다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StudioEvent {
    Press(i32, i32),
    Move(i32, i32),
    Hover(i32, i32),
    Lift,
    Menu(i32, i32),
    /// 창을 옮겼거나 크기를 바꿨다.
    Changed,
    /// 창을 다시 그려야 한다.
    Paint,
    Close,
}

/// 앱에 부탁하는 일.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StudioAsk {
    /// 화면의 이 자리에 메뉴를 띄운다.
    Menu(i32, i32),
    /// 창을 닫는다.
    Close,
    /// 창의 자리나 크기가 바뀌었다. 기억해 둔다.
    Moved,
}

/// 메뉴에서 고른 것.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StudioPick {
    /// 꽃핀을 한 명 초대한다.
    Call,
    /// [`CALLS`]의 몇 번째 수만큼 초대한다.
    CallMany(usize),
    /// 남은 자리만큼 초대한다.
    CallFill,
    /// 처음 꽃핀 한 명만 남기고 돌려보낸다.
    SendAll,
    /// 나와 있는 꽃핀들이 다 같이 한다. [`TOGETHER`]의 몇 번째인지다.
    Together(usize),
    /// [`COLORS`]의 몇 번째 색.
    Color(usize),
    /// 색을 직접 고른다.
    Custom,
    Close,
}

/// 빈 곳을 오른쪽 클릭했을 때의 메뉴가 보여 줄 것.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuState {
    /// 지금의 바탕색.
    pub color: u32,
    /// 꽃핀을 더 부를 수 있는가와, 먼저 온 꽃핀을 돌려보내지 않고 더 부를 수 있는 수.
    pub can_call: bool,
    pub room: usize,
    /// 돌려보낼 꽃핀이 있는가.
    pub can_send: bool,
    /// 꽃핀을 숨겨 둔 중이다. "다 같이"가 흐리다.
    pub hidden: bool,
}

thread_local! {
    static EVENTS: RefCell<Vec<StudioEvent>> = const { RefCell::new(Vec::new()) };
}

fn push(event: StudioEvent) {
    EVENTS.with(|events| {
        let mut events = events.borrow_mut();
        // 옮기는 동안 잇달아 오는 것은 한 번만 적는다.
        if !(matches!(event, StudioEvent::Changed | StudioEvent::Paint) && events.last() == Some(&event)) {
            events.push(event);
        }
    });
}

/// 확인용: 창에서 그 일이 일어난 것으로 친다. 좌표는 화면 기준이다.
pub fn inject(event: StudioEvent) {
    push(event);
}

/// 지난번에 꺼낸 뒤로 쌓인 일을 모두 꺼낸다.
pub fn take_events() -> Vec<StudioEvent> {
    EVENTS.with(|events| std::mem::take(&mut *events.borrow_mut()))
}

/// 창 안쪽과 같은 크기의 그림 버퍼. 바탕색을 칠하고 꽃핀을 얹은 뒤 창에 옮긴다.
struct Canvas {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u32,
    width: i32,
    height: i32,
}

impl Canvas {
    fn new(width: i32, height: i32) -> Canvas {
        let (width, height) = (width.max(1), height.max(1));
        let mut info: BITMAPINFO = unsafe { std::mem::zeroed() };
        info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = width;
        info.bmiHeader.biHeight = -height;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        let mut bits: *mut c_void = null_mut();
        let dc = unsafe { CreateCompatibleDC(null_mut()) };
        let bitmap = unsafe { CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0) };
        if dc.is_null() || bitmap.is_null() || bits.is_null() {
            fatal("방송 촬영용 창의 그림 버퍼를 만들지 못했습니다.");
        }
        let old = unsafe { SelectObject(dc, bitmap) };
        Canvas { dc, bitmap, old, bits: bits as *mut u32, width, height }
    }

    fn pixels(&mut self) -> &mut [u32] {
        unsafe { std::slice::from_raw_parts_mut(self.bits, (self.width * self.height) as usize) }
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

pub struct Studio {
    hwnd: HWND,
    /// 창 안쪽을 모니터 하나로 친 무대.
    stage: Stage,
    /// 창 안쪽의 왼쪽 위(화면 좌표).
    origin: (i32, i32),
    color: u32,
    canvas: Canvas,
    /// 지난번에 꽃핀들을 그린 범위(창 안쪽 좌표). 다음에는 여기와 새 범위만 다시 칠한다.
    drawn: Rect,
    /// 다음에 창 안쪽을 통째로 다시 그린다.
    full: bool,
    icons: (HICON, HICON),
}

impl Studio {
    /// 창을 연다. `place`는 기억해 둔 창의 자리와 크기(창 바깥 기준)이고, 없으면 `center`를 가운데로 연다.
    /// `activate`가 아니면 창을 띄우되 쓰던 창의 포커스를 빼앗지 않는다(앱을 켤 때 다시 열 때).
    pub fn open(
        (place, center): (Option<Rect>, (i32, i32)),
        (color, activate): (u32, bool),
        icons: (HICON, HICON),
    ) -> Studio {
        register_class();
        let class = wide(CLASS_NAME);
        let title = wide(TITLE);
        let outer = place.unwrap_or_else(|| {
            let mut rect = RECT { left: 0, top: 0, right: FIRST_SIZE.0, bottom: FIRST_SIZE.1 };
            unsafe { AdjustWindowRectEx(&mut rect, WS_OVERLAPPEDWINDOW, 0, 0) };
            let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
            Rect::new(center.0 - w / 2, center.1 - h / 2, w, h)
        });
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPEDWINDOW,
                outer.x,
                outer.y,
                outer.w,
                outer.h,
                null_mut(),
                null_mut(),
                GetModuleHandleW(null()),
                null(),
            )
        };
        if hwnd.is_null() {
            fatal("방송 촬영용 창을 만들지 못했습니다.");
        }
        unsafe {
            // 창의 아이콘. 달지 않으면 작업표시줄에 Windows의 기본 그림이 나온다(1이 큰 것, 0이 작은 것).
            SendMessageW(hwnd, WM_SETICON, 1, icons.0 as LPARAM);
            SendMessageW(hwnd, WM_SETICON, 0, icons.1 as LPARAM);
            ShowWindow(hwnd, if activate { SW_SHOWNORMAL } else { SW_SHOWNOACTIVATE });
        }
        let (stage, origin) = read_stage(hwnd);
        let size = client_size(hwnd);
        Studio {
            hwnd,
            stage,
            origin,
            color,
            canvas: Canvas::new(size.0, size.1),
            drawn: Rect::EMPTY,
            full: true,
            icons,
        }
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    pub fn color(&self) -> u32 {
        self.color
    }

    /// 바탕색을 바꾼다. 다음에 그릴 때 통째로 다시 그린다.
    pub fn set_color(&mut self, color: u32) {
        self.color = color & 0xFF_FFFF;
        self.full = true;
    }

    /// 다음에 그릴 때 통째로 다시 그린다.
    pub fn refresh(&mut self) {
        self.full = true;
    }

    /// 창 안쪽을 모니터 하나로 친 무대. 창이 열려 있는 동안 꽃핀들은 여기서 지낸다.
    pub fn stage(&self) -> &Stage {
        &self.stage
    }

    /// 최소화되어 있는가. 그동안은 그리지 않고, 꽃핀들의 시간도 흐르지 않는다.
    pub fn is_minimized(&self) -> bool {
        unsafe { IsIconic(self.hwnd) != 0 }
    }

    /// 화면의 그 점이 창 안쪽인가. `exposed`면 그 점에서 다른 창에 가려져 있지 않은지도 본다.
    pub fn covers(&self, (x, y): (i32, i32), exposed: bool) -> bool {
        let (width, height) = client_size(self.hwnd);
        let (u, v) = (x - self.origin.0, y - self.origin.1);
        if u < 0 || v < 0 || u >= width || v >= height {
            return false;
        }
        !exposed || unsafe { GetAncestor(WindowFromPoint(POINT { x, y }), GA_ROOT) } == self.hwnd
    }

    /// 창의 자리와 크기(창 바깥 기준). 다시 열 때 쓰려고 기억해 둔다. 최소화되어 있으면 없다.
    pub fn place(&self) -> Option<Rect> {
        if self.is_minimized() {
            return None;
        }
        let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        unsafe { GetWindowRect(self.hwnd, &mut rect) };
        Some(Rect::new(rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top))
    }

    /// 창을 맨 앞으로 가져온다.
    pub fn bring_to_front(&self) {
        unsafe {
            ShowWindow(self.hwnd, SW_SHOWNORMAL);
            SetForegroundWindow(self.hwnd);
        }
    }

    /// 창에서 일어난 일 가운데 창이 스스로 할 일을 하고, 앱이 해야 할 일이면 돌려준다.
    /// 누르기, 끌기, 문지르기는 앱이 다룬다.
    pub fn handle(&mut self, event: StudioEvent) -> Option<StudioAsk> {
        match event {
            StudioEvent::Changed => {
                if self.is_minimized() {
                    return None;
                }
                (self.stage, self.origin) = read_stage(self.hwnd);
                self.full = true;
                return Some(StudioAsk::Moved);
            }
            StudioEvent::Paint => self.full = true,
            StudioEvent::Close => return Some(StudioAsk::Close),
            StudioEvent::Menu(x, y) => return Some(StudioAsk::Menu(x, y)),
            StudioEvent::Press(..) | StudioEvent::Move(..) | StudioEvent::Hover(..) | StudioEvent::Lift => {}
        }
        None
    }

    /// 바탕색 위에 꽃핀들을 얹어 창에 그린다. `windows`는 뒤에 서는 꽃핀부터다. `changed`가 아니면(바뀐 꽃핀이
    /// 없으면) 통째로 다시 그려야 할 때만 그린다. 지난번과 이번에 꽃핀들이 있던 곳만 다시 칠한다.
    pub fn compose(&mut self, windows: &[&CharacterWindow], changed: bool) {
        if self.is_minimized() || !(changed || self.full) {
            return;
        }
        let size = client_size(self.hwnd);
        if (self.canvas.width, self.canvas.height) != size {
            self.canvas = Canvas::new(size.0, size.1);
            self.full = true;
        }
        let (width, height) = (self.canvas.width, self.canvas.height);
        let rects: Vec<Rect> = windows
            .iter()
            .map(|window| {
                let screen = window.screen_rect();
                Rect::new(screen.x - self.origin.0, screen.y - self.origin.1, screen.w, screen.h)
            })
            .collect();
        let now_rect = rects.iter().copied().fold(Rect::EMPTY, Rect::union);
        let region = if self.full { Rect::new(0, 0, width, height) } else { self.drawn.union(now_rect) };
        let region = region.clip(width, height);
        (self.drawn, self.full) = (now_rect, false);
        if region.is_empty() {
            return;
        }
        let back = 0xFF00_0000 | self.color;
        let canvas = self.canvas.pixels();
        for y in region.y..region.bottom() {
            let row = (y * width) as usize;
            canvas[row + region.x as usize..row + region.right() as usize].fill(back);
        }
        for (window, rect) in windows.iter().zip(&rects) {
            let sprite = window.shown();
            let (from, to) = (region.x.max(rect.x), region.right().min(rect.right()));
            for y in region.y.max(rect.y)..region.bottom().min(rect.bottom()) {
                let (row, line) = ((y * width) as usize, ((y - rect.y) * rect.w) as usize);
                for x in from..to {
                    let pixel = sprite[line + (x - rect.x) as usize];
                    let alpha = pixel >> 24;
                    if alpha != 0 {
                        let under = &mut canvas[row + x as usize];
                        *under = over(pixel, *under, alpha);
                    }
                }
            }
        }
        unsafe {
            let dc = GetDC(self.hwnd);
            BitBlt(dc, region.x, region.y, region.w, region.h, self.canvas.dc, region.x, region.y, SRCCOPY);
            ReleaseDC(self.hwnd, dc);
        }
    }

    /// 창 안쪽을 그림으로 받는다(확인용). 픽셀은 0xRRGGBB이다.
    pub fn snapshot(&mut self, windows: &[&CharacterWindow]) -> (usize, Vec<[u8; 3]>) {
        self.full = true;
        self.compose(windows, true);
        let width = self.canvas.width as usize;
        let pixels = self.canvas.pixels().iter().map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8]).collect();
        (width, pixels)
    }

    /// 창의 상태(확인용).
    pub fn describe(&self) -> String {
        let work = self.stage.primary().work;
        format!("방송 촬영용 창: 안쪽 {}x{}, 바탕색 #{:06X}, 바닥 {}", work.w, work.h, self.color, work.bottom())
    }
}

impl Drop for Studio {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.hwnd);
            DestroyIcon(self.icons.0);
            DestroyIcon(self.icons.1);
        }
    }
}

/// 화면의 `(x, y)`가 그 꽃핀의 몸(투명하지 않은 곳) 위인가. 창 안에서 누른 꽃핀을 찾는 데 쓴다.
pub fn touches(window: &CharacterWindow, (x, y): (i32, i32)) -> bool {
    let rect = window.screen_rect();
    let (cx, cy) = (x - rect.x, y - rect.y);
    if cx < 0 || cy < 0 || cx >= rect.w || cy >= rect.h {
        return false;
    }
    window.shown()[(cy * rect.w + cx) as usize] >> 24 != 0
}

/// 미리 곱한 알파의 `pixel`을 불투명한 `back` 위에 얹는다.
fn over(pixel: u32, back: u32, alpha: u32) -> u32 {
    let mix = |shift: u32| {
        let (top, bottom) = ((pixel >> shift) & 0xFF, (back >> shift) & 0xFF);
        (top + bottom * (255 - alpha) / 255).min(255) << shift
    };
    0xFF00_0000 | mix(16) | mix(8) | mix(0)
}

/// 창 안쪽의 크기.
fn client_size(hwnd: HWND) -> (i32, i32) {
    let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe { GetClientRect(hwnd, &mut rect) };
    (rect.right - rect.left, rect.bottom - rect.top)
}

/// 창 안쪽을 모니터 하나로 친 무대와, 창 안쪽의 왼쪽 위(화면 좌표).
fn read_stage(hwnd: HWND) -> (Stage, (i32, i32)) {
    let (width, height) = client_size(hwnd);
    let mut origin = POINT { x: 0, y: 0 };
    unsafe { ClientToScreen(hwnd, &mut origin) };
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    let gap = FLOOR_GAP * dpi as i32 / 96;
    let area = Rect::new(origin.x, origin.y, width, (height - gap).max(1));
    (Stage::new(vec![Monitor { area, work: area, dpi, primary: true }]), (origin.x, origin.y))
}

/// 빈 곳을 오른쪽 클릭했을 때의 메뉴를 만든다. 다 쓰면 [`DestroyMenu`]로 거둔다.
fn build(state: &MenuState) -> Option<HMENU> {
    unsafe {
        let [menu, calls, together, colors] = [(); 4].map(|_| CreatePopupMenu());
        if [menu, calls, together, colors].iter().any(|menu| menu.is_null()) {
            return None;
        }
        let add = |menu: HMENU, possible: bool, id: u32, label: &str| {
            let flags = if possible { MF_STRING } else { MF_STRING | MF_GRAYED };
            AppendMenuW(menu, flags, id as usize, wide(label).as_ptr());
        };
        let popup = |menu: HMENU, possible: bool, sub: HMENU, label: &str| {
            let flags = if possible { MF_POPUP } else { MF_POPUP | MF_GRAYED };
            AppendMenuW(menu, flags, sub as usize, wide(label).as_ptr());
        };
        // 초대하기와 돌려보내기. 트레이 메뉴와 같은 수를 고른다.
        add(menu, state.can_call, MENU_CALL, "꽃핀 초대하기");
        for (index, count) in CALLS.iter().enumerate() {
            add(calls, state.room > 0, MENU_MANY + index as u32, &format!("{count}명"));
        }
        let fill = if state.room > 0 {
            format!("가득 채우기 ({}명)", state.room)
        } else {
            "가득 채우기".to_string()
        };
        add(calls, state.room > 0, MENU_FILL, &fill);
        popup(menu, state.can_call, calls, "여러 명 초대하기");
        add(menu, state.can_send, MENU_SEND, "1명만 남기고 보내기");
        for (index, name) in TOGETHER.iter().enumerate() {
            add(together, true, MENU_TOGETHER + index as u32, name);
        }
        popup(menu, !state.hidden, together, "다 같이");
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        // 바탕색.
        for (index, (name, value)) in COLORS.iter().enumerate() {
            let flags = if *value == state.color { MF_STRING | MF_CHECKED } else { MF_STRING };
            AppendMenuW(colors, flags, (MENU_COLOR + index as u32) as usize, wide(name).as_ptr());
        }
        let custom = COLORS.iter().all(|(_, value)| *value != state.color);
        let flags = if custom { MF_STRING | MF_CHECKED } else { MF_STRING };
        AppendMenuW(colors, flags, MENU_CUSTOM as usize, wide("직접 고르기...").as_ptr());
        AppendMenuW(menu, MF_POPUP, colors as usize, wide("바탕색").as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        AppendMenuW(menu, MF_STRING, MENU_CLOSE as usize, wide("창 닫기").as_ptr());
        Some(menu)
    }
}

/// 메뉴 항목의 번호를 고른 것으로 바꾼다.
fn picked(id: u32) -> Option<StudioPick> {
    let within = |start: u32, count: usize| (start..start + count as u32).contains(&id).then(|| (id - start) as usize);
    match id {
        MENU_CALL => Some(StudioPick::Call),
        MENU_SEND => Some(StudioPick::SendAll),
        MENU_FILL => Some(StudioPick::CallFill),
        MENU_CUSTOM => Some(StudioPick::Custom),
        MENU_CLOSE => Some(StudioPick::Close),
        _ => within(MENU_MANY, CALLS.len())
            .map(StudioPick::CallMany)
            .or_else(|| within(MENU_TOGETHER, TOGETHER.len()).map(StudioPick::Together))
            .or_else(|| within(MENU_COLOR, COLORS.len()).map(StudioPick::Color)),
    }
}

/// 창의 빈 곳을 오른쪽 클릭했을 때의 메뉴를 화면의 `(x, y)`에 띄우고, 고른 것을 돌려준다. 메뉴가 닫힐 때까지
/// 돌아오지 않는다.
pub fn pick(owner: HWND, (x, y): (i32, i32), state: &MenuState) -> Option<StudioPick> {
    let menu = build(state)?;
    unsafe {
        SetForegroundWindow(owner);
        let id = TrackPopupMenuEx(menu, TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY, x, y, owner, null());
        PostMessageW(owner, WM_NULL, 0, 0);
        DestroyMenu(menu);
        picked(id as u32)
    }
}

/// Windows의 색 고르기 창을 띄운다. 고르면 그 색(0xRRGGBB)이고, 그만두면 없다. 닫힐 때까지 돌아오지 않는다.
///
/// 색 고르기 창이 든 `comdlg32.dll`은 실행 파일이 켜질 때 불러오지 않고, 쓸 때 시스템 폴더에서 따로 불러온다.
/// 거의 쓰지 않는 것을 켤 때부터 불러 둘 까닭이 없고, 그 DLL이 끌어오는 다른 DLL도 시스템 폴더에서만 찾게 한다.
pub fn choose_color(owner: HWND, color: u32) -> Option<u32> {
    type Choose = unsafe extern "system" fn(*mut CHOOSECOLORW) -> i32;
    let library = system_library("comdlg32.dll");
    if library.is_null() {
        return None;
    }
    let found = unsafe { GetProcAddress(library, c"ChooseColorW".as_ptr().cast()) }?;
    // 받아 온 것은 인자를 알 수 없는 함수 주소다. Windows 문서에 적힌 모양으로 바꿔 쓴다.
    let choose = unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, Choose>(found) };
    // COLORREF는 0x00BBGGRR이다.
    let swap = |c: u32| ((c & 0xFF) << 16) | (c & 0xFF00) | ((c >> 16) & 0xFF);
    let mut custom: [COLORREF; 16] = [0xFF_FFFF; 16];
    for (slot, (_, value)) in custom.iter_mut().zip(COLORS) {
        *slot = swap(value);
    }
    let mut chooser: CHOOSECOLORW = unsafe { std::mem::zeroed() };
    chooser.lStructSize = size_of::<CHOOSECOLORW>() as u32;
    chooser.hwndOwner = owner;
    chooser.rgbResult = swap(color);
    chooser.lpCustColors = custom.as_mut_ptr();
    chooser.Flags = CC_RGBINIT | CC_FULLOPEN;
    (unsafe { choose(&mut chooser) } != 0).then(|| swap(chooser.rgbResult))
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
            hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class.as_ptr(),
            hIconSm: null_mut(),
        };
        if unsafe { RegisterClassExW(&wc) } == 0 {
            fatal("방송 촬영용 창의 종류를 등록하지 못했습니다.");
        }
    });
}

/// 클라이언트 좌표가 담긴 `lparam`을 화면 좌표로 바꾼다.
fn screen_point(hwnd: HWND, lparam: LPARAM) -> (i32, i32) {
    let mut point = POINT { x: (lparam & 0xFFFF) as i16 as i32, y: ((lparam >> 16) & 0xFFFF) as i16 as i32 };
    unsafe { ClientToScreen(hwnd, &mut point) };
    (point.x, point.y)
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_LBUTTONDOWN => {
            unsafe { SetCapture(hwnd) };
            let (x, y) = screen_point(hwnd, lparam);
            push(StudioEvent::Press(x, y));
            0
        }
        WM_MOUSEMOVE => {
            let (x, y) = screen_point(hwnd, lparam);
            push(if unsafe { GetCapture() } == hwnd { StudioEvent::Move(x, y) } else { StudioEvent::Hover(x, y) });
            0
        }
        WM_LBUTTONUP => {
            unsafe { ReleaseCapture() };
            0
        }
        // 뗐다는 것은 여기 한 곳에서만 알린다. 누르고 있던 것이 다른 까닭으로 끊겼을 때도 온다.
        WM_CAPTURECHANGED => {
            push(StudioEvent::Lift);
            0
        }
        WM_RBUTTONUP => {
            let (x, y) = screen_point(hwnd, lparam);
            push(StudioEvent::Menu(x, y));
            0
        }
        WM_MOVE | WM_SIZE => {
            push(StudioEvent::Changed);
            0
        }
        // 바탕은 그림 버퍼로 칠하므로 지우지 않는다. 지우면 깜빡인다.
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            let mut paint: PAINTSTRUCT = unsafe { std::mem::zeroed() };
            unsafe {
                BeginPaint(hwnd, &mut paint);
                EndPaint(hwnd, &paint);
            }
            push(StudioEvent::Paint);
            0
        }
        WM_GETMINMAXINFO => {
            let info = unsafe { &mut *(lparam as *mut MINMAXINFO) };
            let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as i32;
            let mut rect = RECT { left: 0, top: 0, right: MIN_SIZE.0 * dpi / 96, bottom: MIN_SIZE.1 * dpi / 96 };
            unsafe { AdjustWindowRectEx(&mut rect, WS_OVERLAPPEDWINDOW, 0, 0) };
            info.ptMinTrackSize = POINT { x: rect.right - rect.left, y: rect.bottom - rect.top };
            0
        }
        WM_CLOSE => {
            push(StudioEvent::Close);
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiplied_pixels_sit_on_the_backdrop() {
        let green = 0xFF00_FF00;
        // 다 투명하지 않은 빨강은 빨강 그대로, 반투명한 빨강(미리 곱함)은 초록과 섞인다.
        assert_eq!(over(0xFFFF_0000, green, 0xFF), 0xFFFF_0000);
        assert_eq!(over(0x8080_0000, green, 0x80), 0xFF80_7F00);
        assert_eq!(COLORS[0].1, 0x00FF00);
    }

    #[test]
    fn the_colour_picker_is_found_in_the_system_folder() {
        let library = system_library("comdlg32.dll");
        assert!(!library.is_null());
        assert!(unsafe { GetProcAddress(library, c"ChooseColorW".as_ptr().cast()) }.is_some());
    }

    #[test]
    fn the_menu_on_an_empty_spot_calls_sends_and_plays_together() {
        use crate::tray::tests::{lines, marked};
        let state = MenuState { color: 0x0000FF, can_call: true, room: 4, can_send: true, hidden: false };
        let names = |state: &MenuState| {
            let menu = build(state).expect("메뉴");
            let read = marked(lines(menu));
            unsafe { DestroyMenu(menu) };
            read
        };
        assert_eq!(
            names(&state),
            ["꽃핀 초대하기", "여러 명 초대하기", "1명만 남기고 보내기", "다 같이", "-", "바탕색", "-", "창 닫기"]
        );
        // 혼자면 보낼 꽃핀이 없고, 더 부를 수 없으면 초대하는 두 줄이 흐리다. 숨겨 둔 동안에는 "다 같이"가 흐리다.
        let stuck = MenuState { can_call: false, room: 0, can_send: false, hidden: true, ..state };
        assert_eq!(names(&stuck)[..4], ["(꽃핀 초대하기)", "(여러 명 초대하기)", "(1명만 남기고 보내기)", "(다 같이)"]);
        // 번호는 고른 것으로 돌아온다.
        let all = [
            StudioPick::Call,
            StudioPick::SendAll,
            StudioPick::CallFill,
            StudioPick::Custom,
            StudioPick::Close,
            StudioPick::CallMany(2),
            StudioPick::Together(1),
            StudioPick::Color(2),
        ];
        let ids = [
            MENU_CALL,
            MENU_SEND,
            MENU_FILL,
            MENU_CUSTOM,
            MENU_CLOSE,
            MENU_MANY + 2,
            MENU_TOGETHER + 1,
            MENU_COLOR + 2,
        ];
        for (id, pick) in ids.into_iter().zip(all) {
            assert_eq!(picked(id), Some(pick));
        }
        for id in [0, 4, 13, 23, 33] {
            assert_eq!(picked(id), None, "{id}");
        }
    }
}
