//! 방송 촬영용 창.
//!
//! 단색 바탕의 보통 창 안에 꽃핀 한 명이 산다. 방송 프로그램(OBS 등)의 창 캡처로 이 창을 잡고 크로마 키로
//! 바탕색을 빼면, 방송 화면에 꽃핀만 올라간다. 창은 다른 창에 가려져도 되지만, 최소화하면 잡히지 않는다.
//!
//! 창 안의 꽃핀은 바탕화면의 꽃핀들과 따로다. 창의 안쪽이 그 꽃핀의 모니터이고, 아래 끝(조금 위)이 바닥,
//! 양옆이 벽이다. 꽃핀은 화면에 내보내지 않는 그림 버퍼에 그려지고([`Look::offscreen`]), 이 창이 그것을
//! 바탕색 위에 옮겨 그린다. 창 안에서 클릭하고, 머리를 문지르고, 잡아서 옮길 수 있다. 창을 오른쪽 클릭하면
//! 바탕색을 고르는 메뉴가 뜬다.
//!
//! 창에서 일어난 일은 창 메시지를 받는 자리에서 처리하지 않고 [`StudioEvent`]로 적어 두기만 한다.

use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::Once;
use std::time::{Duration, Instant};

use dfp_pack::PackError;
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
    DestroyWindow, GetClientRect, GetWindowRect, HICON, IDC_ARROW, IsIconic, LoadCursorW, MF_CHECKED, MF_POPUP,
    MF_SEPARATOR, MF_STRING, MINMAXINFO, PostMessageW, RegisterClassExW, SW_SHOWNOACTIVATE, SW_SHOWNORMAL,
    SendMessageW, SetForegroundWindow, ShowWindow, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx,
    WM_CAPTURECHANGED, WM_CLOSE, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOVE,
    WM_NULL, WM_PAINT, WM_RBUTTONUP, WM_SETICON, WM_SIZE, WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
};

use crate::actor::{Habits, Mode};
use crate::character::{Character, Look};
use crate::desktop::Desktop;
use crate::frames::Frames;
use crate::geom::Rect;
use crate::motions::Motions;
use crate::stage::{Monitor, Stage};
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
/// 누른 채 이만큼(1배 기준 px) 움직이거나 이만큼 누르고 있으면 잡은 것으로 친다. 바탕화면 꽃핀과 같다.
const CLICK_MOVE: f64 = 6.0;
const CLICK_HOLD: Duration = Duration::from_millis(250);
/// 머리를 문지르는 움직임이 이어진 것으로 칠 사이와 한 번에 움직인 거리의 한계(1배 기준 px).
const RUB_GAP: Duration = Duration::from_millis(250);
const RUB_JUMP: f64 = 120.0;

/// 메뉴 항목의 번호.
const MENU_COLOR: u32 = 1;
const MENU_CUSTOM: u32 = 9;
const MENU_CLOSE: u32 = 10;

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
    /// [`COLORS`]의 몇 번째 색.
    Color(usize),
    /// 색을 직접 고른다.
    Custom,
    Close,
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

/// 누르고 끄는 상태.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Grip {
    Idle,
    Pressing { at: (i32, i32), since: Instant },
    Dragging,
}

pub struct Studio {
    hwnd: HWND,
    character: Character,
    /// 창 안쪽을 모니터 하나로 친 무대.
    stage: Stage,
    /// 창 안쪽의 왼쪽 위(화면 좌표).
    origin: (i32, i32),
    /// 다른 창이 없는 바탕화면. 창 안에는 올라설 창이 없다.
    desktop: Desktop,
    color: u32,
    canvas: Canvas,
    grip: Grip,
    hover: Option<((i32, i32), Instant)>,
    /// 지난번에 꽃핀을 그린 범위(창 안쪽 좌표). 다음에는 여기와 새 범위만 다시 칠한다.
    drawn: Rect,
    /// 다음에 창 안쪽을 통째로 다시 그린다.
    full: bool,
    icons: (HICON, HICON),
}

impl Studio {
    /// 창을 연다. `place`는 기억해 둔 창의 자리와 크기(창 바깥 기준)이고, 없으면 `center`를 가운데로 연다.
    /// `look`은 바탕화면 꽃핀의 것이다. 창 안의 꽃핀은 화면에 내보내지 않는 버퍼에 그린다.
    /// `activate`가 아니면 창을 띄우되 쓰던 창의 포커스를 빼앗지 않는다(앱을 켤 때 다시 열 때).
    pub fn open(
        (frames, motions): (&mut Frames, &Motions),
        look: Look,
        (place, center): (Option<Rect>, (i32, i32)),
        (color, activate): (u32, bool),
        icons: (HICON, HICON),
        (now, beat, seed): (Instant, Instant, u64),
    ) -> Result<Studio, PackError> {
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
        let work = stage.primary().work;
        let look = Look { topmost: false, offscreen: true, ..look };
        // 창 안의 위쪽 공중에 나타나 바닥으로 떨어진다.
        let spot = ((work.x + work.w / 2) as f64, (work.y + work.h / 3) as f64);
        let character = Character::appear(frames, motions, &stage, look, spot, (now, beat), seed)?;
        let size = client_size(hwnd);
        Ok(Studio {
            hwnd,
            character,
            stage,
            origin,
            desktop: Desktop::default(),
            color,
            canvas: Canvas::new(size.0, size.1),
            grip: Grip::Idle,
            hover: None,
            drawn: Rect::EMPTY,
            full: true,
            icons,
        })
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    pub fn color(&self) -> u32 {
        self.color
    }

    /// 바탕색을 바꾸고 다시 그린다.
    pub fn set_color(&mut self, color: u32) {
        self.color = color & 0xFF_FFFF;
        self.full = true;
        self.compose();
    }

    /// 창의 자리와 크기(창 바깥 기준). 다시 열 때 쓰려고 기억해 둔다. 최소화되어 있으면 없다.
    pub fn place(&self) -> Option<Rect> {
        if unsafe { IsIconic(self.hwnd) } != 0 {
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

    /// 꽃핀의 크기 설정이 바뀌었다.
    pub fn set_look(&mut self, look: Look, now: Instant) {
        self.character.set_look(Look { topmost: false, offscreen: true, ..look }, now);
    }

    /// 꽃핀의 버릇(가끔 하는 행동의 빈도)을 설정에 맞춘다.
    pub fn set_habits(&mut self, habits: Habits) {
        self.character.actor_mut().set_habits(habits);
    }

    /// 다음에 깨어나야 할 시각.
    pub fn due(&self) -> Instant {
        match self.grip {
            Grip::Pressing { since, .. } => self.character.due().min(since + CLICK_HOLD),
            _ => self.character.due(),
        }
    }

    /// 창에서 일어난 일을 처리한다. 앱이 해야 할 일이 있으면 돌려준다.
    pub fn handle(&mut self, event: StudioEvent, motions: &Motions, now: Instant) -> Option<StudioAsk> {
        let scale = self.character.scale().factor();
        match event {
            StudioEvent::Changed => {
                if unsafe { IsIconic(self.hwnd) } != 0 {
                    return None;
                }
                (self.stage, self.origin) = read_stage(self.hwnd);
                // 바닥이 옮겨졌으면 꽃핀이 떨어지거나 올라선다.
                self.character.wake(now);
                self.full = true;
                self.compose();
                return Some(StudioAsk::Moved);
            }
            StudioEvent::Paint => {
                self.full = true;
                self.compose();
            }
            StudioEvent::Close => return Some(StudioAsk::Close),
            StudioEvent::Menu(x, y) => return Some(StudioAsk::Menu(x, y)),
            StudioEvent::Press(x, y) => {
                if self.touches(x, y) && self.character.actor().can_grab() {
                    self.grip = Grip::Pressing { at: (x, y), since: now };
                } else {
                    unsafe { ReleaseCapture() };
                }
            }
            StudioEvent::Move(x, y) => match self.grip {
                Grip::Pressing { at, .. } => {
                    if ((x - at.0) as f64).hypot((y - at.1) as f64) > CLICK_MOVE * scale {
                        self.grab((x, y), motions, now);
                    }
                }
                Grip::Dragging => {
                    let target = self.character.drag_target((x, y), &self.stage);
                    self.character.actor_mut().drag_to(target);
                    self.character.wake(now);
                }
                Grip::Idle => {}
            },
            StudioEvent::Lift => {
                match self.grip {
                    Grip::Dragging => self.character.actor_mut().release(motions),
                    // 짧게 눌렀다 뗐다. 클릭이다.
                    Grip::Pressing { .. } => self.character.actor_mut().poke(motions),
                    Grip::Idle => {}
                }
                self.grip = Grip::Idle;
                self.character.wake(now);
            }
            StudioEvent::Hover(x, y) => {
                if !self.touches(x, y) {
                    self.hover = None;
                    return None;
                }
                if let Some((at, when)) = self.hover {
                    // 가로와 세로로 움직인 거리를 더한다(바탕화면 꽃핀과 같은 셈법).
                    let distance = ((x - at.0).abs() + (y - at.1).abs()) as f64;
                    if now.duration_since(when) <= RUB_GAP && distance <= RUB_JUMP * scale && distance > 0.0 {
                        self.character.rub(motions, (x, y), distance, now);
                    }
                }
                self.hover = Some(((x, y), now));
            }
        }
        None
    }

    /// 누른 채 오래 있었으면 잡는다. 때가 된 꽃핀의 시간을 흘리고, 달라졌으면 창에 그린다.
    pub fn tick(&mut self, frames: &mut Frames, motions: &Motions, horizon: Instant) -> Result<(), PackError> {
        if let Grip::Pressing { at, since } = self.grip
            && horizon >= since + CLICK_HOLD
        {
            self.grab(at, motions, horizon);
        }
        if unsafe { IsIconic(self.hwnd) } != 0 {
            return Ok(());
        }
        let shown = self.character.tick(frames, motions, &self.stage, (&self.desktop, &[]), horizon)?;
        if shown || self.full {
            self.compose();
        }
        Ok(())
    }

    fn grab(&mut self, cursor: (i32, i32), motions: &Motions, now: Instant) {
        let target = self.character.drag_target(cursor, &self.stage);
        self.grip = if self.character.actor_mut().grab(motions, target) { Grip::Dragging } else { Grip::Idle };
        self.character.wake(now);
    }

    /// 화면의 `(x, y)`가 꽃핀의 몸(투명하지 않은 곳) 위인가.
    fn touches(&self, x: i32, y: i32) -> bool {
        let window = self.character.window();
        let rect = window.screen_rect();
        let (cx, cy) = (x - rect.x, y - rect.y);
        if cx < 0 || cy < 0 || cx >= rect.w || cy >= rect.h {
            return false;
        }
        window.shown()[(cy * rect.w + cx) as usize] >> 24 != 0
    }

    /// 바탕색 위에 꽃핀을 얹어 창에 그린다. 지난번과 이번에 꽃핀이 있던 곳만 다시 칠한다.
    fn compose(&mut self) {
        let size = client_size(self.hwnd);
        if (self.canvas.width, self.canvas.height) != size {
            self.canvas = Canvas::new(size.0, size.1);
            self.full = true;
        }
        let (width, height) = (self.canvas.width, self.canvas.height);
        let screen = self.character.window().screen_rect();
        let now_rect = Rect::new(screen.x - self.origin.0, screen.y - self.origin.1, screen.w, screen.h);
        let region = if self.full { Rect::new(0, 0, width, height) } else { self.drawn.union(now_rect) };
        let region = region.clip(width, height);
        if region.is_empty() {
            return;
        }
        let back = 0xFF00_0000 | self.color;
        let sprite = self.character.window().shown();
        let canvas = self.canvas.pixels();
        for y in region.y..region.bottom() {
            let row = (y * width) as usize;
            canvas[row + region.x as usize..row + region.right() as usize].fill(back);
            let sy = y - now_rect.y;
            if sy < 0 || sy >= now_rect.h {
                continue;
            }
            let from = region.x.max(now_rect.x);
            let to = region.right().min(now_rect.right());
            for x in from..to {
                let pixel = sprite[(sy * now_rect.w + (x - now_rect.x)) as usize];
                let alpha = pixel >> 24;
                if alpha != 0 {
                    canvas[row + x as usize] = over(pixel, back, alpha);
                }
            }
        }
        unsafe {
            let dc = GetDC(self.hwnd);
            BitBlt(dc, region.x, region.y, region.w, region.h, self.canvas.dc, region.x, region.y, SRCCOPY);
            ReleaseDC(self.hwnd, dc);
        }
        self.drawn = now_rect;
        self.full = false;
    }

    /// 창 안쪽을 그림으로 받는다(확인용). 픽셀은 0xRRGGBB이다.
    pub fn snapshot(&mut self) -> (usize, Vec<[u8; 3]>) {
        self.full = true;
        self.compose();
        let width = self.canvas.width as usize;
        let pixels = self.canvas.pixels().iter().map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8]).collect();
        (width, pixels)
    }

    /// 창 안의 꽃핀의 상태(확인용).
    pub fn describe(&self) -> String {
        let actor = self.character.actor();
        let (x, y) = actor.position();
        let mode = actor.mode();
        let work = self.stage.primary().work;
        let floor = work.bottom();
        let grabbed = if mode == Mode::Drag { ", 잡혀 있음" } else { "" };
        format!(
            "방송 촬영용 창: 안쪽 {}x{}, 바탕색 #{:06X}, 꽃핀 {mode:?} ({x:.0}, {y:.0}), 바닥 {floor}{grabbed}",
            work.w, work.h, self.color
        )
    }

    /// 확인용 스크립트가 창 안쪽의 `(x, y)`(창 안쪽 좌표)에서 일어난 일을 흉내 낸다.
    pub fn script(&mut self, event: StudioEvent, motions: &Motions, now: Instant) -> Option<StudioAsk> {
        let at = |x: i32, y: i32| (x + self.origin.0, y + self.origin.1);
        let event = match event {
            StudioEvent::Press(x, y) => StudioEvent::Press(at(x, y).0, at(x, y).1),
            StudioEvent::Move(x, y) => StudioEvent::Move(at(x, y).0, at(x, y).1),
            StudioEvent::Hover(x, y) => StudioEvent::Hover(at(x, y).0, at(x, y).1),
            other => other,
        };
        self.handle(event, motions, now)
    }

    /// 창 안의 꽃핀의 발바닥 자리(창 안쪽 좌표). 확인용 스크립트가 꽃핀을 누를 자리를 찾는 데 쓴다.
    pub fn character_spot(&self) -> (i32, i32) {
        let (x, y) = self.character.actor().position();
        (x.round() as i32 - self.origin.0, y.round() as i32 - self.origin.1)
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

/// 창의 오른쪽 클릭 메뉴를 화면의 `(x, y)`에 띄우고, 고른 것을 돌려준다. 메뉴가 닫힐 때까지 돌아오지 않는다.
pub fn pick(owner: HWND, (x, y): (i32, i32), color: u32) -> Option<StudioPick> {
    unsafe {
        let menu = CreatePopupMenu();
        let colors = CreatePopupMenu();
        if menu.is_null() || colors.is_null() {
            return None;
        }
        for (index, (name, value)) in COLORS.iter().enumerate() {
            let flags = if *value == color { MF_STRING | MF_CHECKED } else { MF_STRING };
            AppendMenuW(colors, flags, (MENU_COLOR + index as u32) as usize, wide(name).as_ptr());
        }
        let custom = COLORS.iter().all(|(_, value)| *value != color);
        let flags = if custom { MF_STRING | MF_CHECKED } else { MF_STRING };
        AppendMenuW(colors, flags, MENU_CUSTOM as usize, wide("직접 고르기...").as_ptr());
        AppendMenuW(menu, MF_POPUP, colors as usize, wide("바탕색").as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        AppendMenuW(menu, MF_STRING, MENU_CLOSE as usize, wide("창 닫기").as_ptr());
        SetForegroundWindow(owner);
        let picked = TrackPopupMenuEx(menu, TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY, x, y, owner, null());
        PostMessageW(owner, WM_NULL, 0, 0);
        DestroyMenu(menu);
        match picked as u32 {
            MENU_CUSTOM => Some(StudioPick::Custom),
            MENU_CLOSE => Some(StudioPick::Close),
            id if (MENU_COLOR..MENU_COLOR + COLORS.len() as u32).contains(&id) => {
                Some(StudioPick::Color((id - MENU_COLOR) as usize))
            }
            _ => None,
        }
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
}
