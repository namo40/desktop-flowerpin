//! 꽃핀을 오른쪽 클릭하면 머리 위에 부채꼴로 뜨는 단추들.
//!
//! 포스트잇의 단추와 같은 둥근 단추가 머리 위에 활 모양으로 늘어선다. 누르면 그 일을 하고 닫힌다. 마우스를
//! 올리면 그 단추의 이름이 부채꼴 안쪽 한가운데에 뜬다. 마우스가 벗어난 채로 조금 지나면 저절로 닫힌다.
//!
//! 단추는 두 단계다. 관련된 단추들은 묶음 단추 하나에 들어 있다. 묶음 단추를 누르면 떠 있던 단추들이 머리로
//! 들어갔다가, 묶음 안의 단추들이 맨 앞의 "되돌아가기"와 함께 튀어나온다. 되돌아가기를 누르면 같은 움직임으로
//! 처음 단추들이 돌아온다.
//!
//! 포스트잇과 같은 종류의 창이라 그리는 법과 일어난 일을 받는 법이 같다. 눌러도 쓰던 창의 포커스를 빼앗지 않는다.
//! 어떤 단추를 넣을지와, 눌렀을 때 무엇을 할지는 앱이 정한다. 여기서는 무엇을 눌렀는지([`Pick`])만 알린다.
//!
//! 바로 가기 단추의 그림(치지직, 유튜브, 네이버 카페)은 저마다의 상표다. 모양과 색을 바꾸지 않고 줄여서만 쓴다.

use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject,
    GetDIBits, GetObjectW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateIconFromResourceEx, DestroyIcon, GetIconInfo, ICONINFO, LR_DEFAULTCOLOR,
};

use super::draw::{CREAM, DEEP, HOT, PINK, ROSE, rgb};
use super::panel::Surface;
use super::text::{self, Block};
use super::window::{self, Cursor, Event};
use super::{Outcome, Postits};
use crate::dev::FanStep;
use crate::geom::Rect;

/// 단추의 반지름.
const BUTTON: f32 = 19.0;
/// 이웃한 단추 사이의 틈.
const GAP: f32 = 7.0;
/// 단추들이 늘어서는 활의 반지름.
const RADIUS: f32 = 96.0;
/// 가장 아래 단추의 아래 끝이 머리 꼭대기보다 얼마나 위에 있는지. 음수면 그만큼 아래다.
/// 꽃핀의 머리 꼭대기는 눈에 보이는 머리보다 조금 위로 잡혀 있어서, 양 끝의 단추를 그보다 조금 내린다.
const LIFT: f32 = -12.0;
/// 창 둘레에 그림자가 들어갈 여백.
const EDGE: f32 = 8.0;
/// 바로 가기 그림의 한 변.
const PICTURE: f32 = 24.0;
/// 단추 하나가 튀어나오는 데 걸리는 초와, 다음 단추가 늦게 나오는 초.
const OPEN: f32 = 0.16;
const STAGGER: f32 = 0.018;
/// 단추 하나가 머리로 들어가는 데 걸리는 초. 묶음을 열거나 되돌아갈 때다.
const RETRACT: f32 = 0.12;
/// 한 단계에 늘어설 수 있는 단추의 가장 많은 수. 이보다 많으면 반원을 넘어 양 끝 단추가 머리 옆으로 내려온다.
pub const MOST: usize = 7;
/// 마우스가 벗어난 뒤 닫히기까지의 초. 한 번도 올라오지 않았으면 더 기다린다.
const LINGER: f32 = 1.5;
const UNSEEN: f32 = 5.0;
/// 그림을 다시 그리는 가장 짧은 간격(초).
const REDRAW_EVERY: f32 = 0.03;
/// 넣어 둔 바로 가기 그림의 한 변(px).
const SOURCE: usize = 96;

/// 단추 하나. 눌렀을 때 무엇을 할지는 앱이 정한다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pick {
    /// 꽃핀을 하나 더 초대한다.
    Invite,
    /// 이 꽃핀을 보낸다.
    Send,
    /// 메모 목록 창을 연다.
    Memos,
    /// 바로 가기: 치지직 방송, 유튜브, 팬카페.
    Chzzk,
    Youtube,
    Cafe,
    /// 설정 창을 연다.
    Settings,
    /// 묶음: 바로 가기(치지직, 유튜브, 카페).
    Links,
    /// 묶음 안의 맨 앞 단추. 처음 단추들로 돌아간다.
    Back,
}

impl Pick {
    /// 모든 단추.
    pub const ALL: [Pick; 9] = [
        Pick::Invite,
        Pick::Send,
        Pick::Memos,
        Pick::Links,
        Pick::Back,
        Pick::Chzzk,
        Pick::Youtube,
        Pick::Cafe,
        Pick::Settings,
    ];

    /// 누르면 안에 든 단추들이 나오는 묶음 단추인가.
    pub fn is_group(self) -> bool {
        matches!(self, Pick::Links)
    }

    /// 마우스를 올리면 뜨는 이름.
    pub fn name(self) -> &'static str {
        match self {
            Pick::Invite => "초대하기",
            Pick::Send => "보내기",
            Pick::Memos => "메모 목록",
            Pick::Chzzk => "치지직 방송",
            Pick::Youtube => "유튜브",
            Pick::Cafe => "카페",
            Pick::Settings => "설정",
            Pick::Links => "바로 가기",
            Pick::Back => "되돌아가기",
        }
    }

    /// 확인용 스크립트에서 부르는 이름.
    pub fn word(self) -> &'static str {
        match self {
            Pick::Invite => "invite",
            Pick::Send => "send",
            Pick::Memos => "memos",
            Pick::Chzzk => "chzzk",
            Pick::Youtube => "youtube",
            Pick::Cafe => "cafe",
            Pick::Settings => "settings",
            Pick::Links => "links",
            Pick::Back => "back",
        }
    }

    /// 아이콘 글꼴(Segoe MDL2 Assets)의 글자: 사람과 더하기, 문 밖으로 나가는 화살표, 줄 목록, 톱니바퀴,
    /// 사슬 고리, 왼쪽 화살표.
    fn glyph(self) -> Option<&'static str> {
        match self {
            Pick::Invite => Some("\u{E8FA}"),
            Pick::Send => Some("\u{F3B1}"),
            Pick::Memos => Some("\u{E8FD}"),
            Pick::Settings => Some("\u{E713}"),
            Pick::Links => Some("\u{E71B}"),
            Pick::Back => Some("\u{E72B}"),
            Pick::Chzzk | Pick::Youtube | Pick::Cafe => None,
        }
    }

    /// 바로 가기 단추의 그림(PNG, 96x96).
    fn picture(self) -> Option<&'static [u8]> {
        match self {
            Pick::Chzzk => Some(include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/res/icons/chzzk.png"))),
            Pick::Youtube => Some(include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/res/icons/youtube.png"))),
            Pick::Cafe => Some(include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/res/icons/naver_cafe.png"))),
            Pick::Invite | Pick::Send | Pick::Memos | Pick::Settings | Pick::Links | Pick::Back => None,
        }
    }
}

/// 단추 `count`개가 놓일 자리. 활의 중심을 (0, 0)으로 하고, 위쪽이 음수다. 왼쪽부터 차례로다.
/// 이웃한 단추가 [`GAP`]만큼 떨어지게 벌리고, 한가운데가 가장 높다.
pub fn spread(count: usize) -> Vec<(f32, f32)> {
    let step = 2.0 * ((2.0 * BUTTON + GAP) / (2.0 * RADIUS)).asin();
    (0..count)
        .map(|at| {
            let (sin, cos) = ((at as f32 - (count as f32 - 1.0) / 2.0) * step).sin_cos();
            (RADIUS * sin, -RADIUS * cos)
        })
        .collect()
}

/// 단추의 이름이 뜨는 높이. 가장 아래 단추와 같은 높이인데, 단추가 적어 활이 얕으면 가운데 단추에 닿지 않게
/// 그 아래로 내린다. `spots`와 같은 좌표다.
fn name_height(spots: &[(f32, f32)]) -> f32 {
    let lowest = spots.iter().map(|spot| spot.1).fold(f32::MIN, f32::max);
    let highest = spots.iter().map(|spot| spot.1).fold(f32::MAX, f32::min);
    lowest.max(highest + BUTTON + 13.0)
}

/// 단추들을 띄울 창의 자리와 크기, 창 안에서 단추들이 놓일 자리.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// 창의 왼쪽 위가 놓일 화면 좌표(px).
    pub at: (i32, i32),
    /// 창의 크기(100%일 때).
    pub size: (f32, f32),
    /// 창 안에서 단추의 가운데들.
    pub spots: Vec<(f32, f32)>,
    /// 단추들이 튀어나오는 곳. 머리 꼭대기다.
    pub origin: (f32, f32),
}

/// 머리 꼭대기의 가운데(`head`, 화면 좌표) 위에 단추 `count`개를 놓는다. `bounds`를 벗어나면 안쪽으로 민다.
pub fn plan(head: (i32, i32), bounds: Rect, scale: f32, count: usize) -> Plan {
    let spots = spread(count);
    let lowest = spots.iter().map(|spot| spot.1).fold(-RADIUS, f32::max);
    let reach = spots.iter().map(|spot| spot.0.abs()).fold(0.0, f32::max) + BUTTON + EDGE;
    // 창의 가장자리. 활의 중심에서 잰 것이다.
    let (left, top) = (-reach, -RADIUS - BUTTON - EDGE);
    // 이름표(높이 22)도 창 안에 들게 한다.
    let size = (2.0 * reach, (lowest + BUTTON).max(name_height(&spots) + 11.0) + EDGE - top);
    // 가장 아래 단추의 아래 끝이 머리 꼭대기 바로 위에 온다.
    let head_below = lowest + BUTTON + LIFT;
    let px = |units: f32| (units * scale).round() as i32;
    let fit = |value: i32, least: i32, most: i32| value.clamp(least, most.max(least));
    let at = (
        fit(head.0 + px(left), bounds.x, bounds.x + bounds.w - px(size.0)),
        fit(head.1 + px(top - head_below), bounds.y, bounds.y + bounds.h - px(size.1)),
    );
    Plan {
        at,
        size,
        spots: spots.iter().map(|spot| (spot.0 - left, spot.1 - top)).collect(),
        origin: (-left, head_below - top),
    }
}

/// PNG를 풀어 알파를 곱하지 않은 BGRA 픽셀로 돌려준다. 한 변이 [`SOURCE`]인 정사각형만 받는다.
///
/// 그림을 푸는 일은 Windows에 맡긴다. PNG로 된 아이콘을 읽는 길을 빌린 것이다.
fn decode(png: &[u8]) -> Option<Vec<[u8; 4]>> {
    unsafe {
        let icon = CreateIconFromResourceEx(png.as_ptr(), png.len() as u32, 1, 0x0003_0000, 0, 0, LR_DEFAULTCOLOR);
        if icon.is_null() {
            return None;
        }
        let mut parts: ICONINFO = std::mem::zeroed();
        let mut pixels = vec![0u32; SOURCE * SOURCE];
        let mut read = false;
        if GetIconInfo(icon, &mut parts) != 0 {
            let mut shape: BITMAP = std::mem::zeroed();
            GetObjectW(parts.hbmColor, size_of::<BITMAP>() as i32, &mut shape as *mut BITMAP as *mut c_void);
            if (shape.bmWidth, shape.bmHeight) == (SOURCE as i32, SOURCE as i32) {
                let mut info: BITMAPINFO = std::mem::zeroed();
                info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
                info.bmiHeader.biWidth = SOURCE as i32;
                info.bmiHeader.biHeight = -(SOURCE as i32);
                info.bmiHeader.biPlanes = 1;
                info.bmiHeader.biBitCount = 32;
                info.bmiHeader.biCompression = BI_RGB;
                let dc = CreateCompatibleDC(null_mut());
                let rows = SOURCE as u32;
                let bits = pixels.as_mut_ptr() as *mut c_void;
                read = GetDIBits(dc, parts.hbmColor, 0, rows, bits, &mut info, DIB_RGB_COLORS) == SOURCE as i32;
                DeleteDC(dc);
            }
            DeleteObject(parts.hbmColor);
            DeleteObject(parts.hbmMask);
        }
        DestroyIcon(icon);
        read.then(|| pixels.iter().map(|pixel| pixel.to_le_bytes()).collect())
    }
}

/// 풀어 둔 바로 가기 그림. 처음 쓸 때 한 번만 푼다.
fn source(pick: Pick) -> Option<&'static [[u8; 4]]> {
    static DECODED: OnceLock<Vec<Option<Vec<[u8; 4]>>>> = OnceLock::new();
    let all = DECODED.get_or_init(|| Pick::ALL.iter().map(|pick| pick.picture().and_then(decode)).collect());
    all[Pick::ALL.iter().position(|&other| other == pick)?].as_deref()
}

/// 한 변이 `side`인 그림을 한 변이 `to`가 되게 줄인다. 겹치는 넓이만큼 고르게 섞는다.
fn shrink(pixels: &[[u8; 4]], side: usize, to: usize) -> Vec<[u8; 4]> {
    let ratio = side as f32 / to as f32;
    let mut out = Vec::with_capacity(to * to);
    for y in 0..to {
        for x in 0..to {
            // 이 픽셀이 덮는 원래 그림의 범위.
            let (left, right) = (x as f32 * ratio, (x + 1) as f32 * ratio);
            let (top, low) = (y as f32 * ratio, (y + 1) as f32 * ratio);
            let mut sum = [0.0f32; 4];
            let mut area = 0.0;
            for v in top as usize..(low.ceil() as usize).min(side) {
                let tall = (low.min(v as f32 + 1.0) - top.max(v as f32)).max(0.0);
                for u in left as usize..(right.ceil() as usize).min(side) {
                    let part = tall * (right.min(u as f32 + 1.0) - left.max(u as f32)).max(0.0);
                    let [blue, green, red, alpha] = pixels[v * side + u];
                    // 색은 알파를 곱해서 섞어야 투명한 곳의 색이 번지지 않는다.
                    let cover = alpha as f32 / 255.0 * part;
                    sum = [
                        sum[0] + blue as f32 * cover,
                        sum[1] + green as f32 * cover,
                        sum[2] + red as f32 * cover,
                        sum[3] + cover,
                    ];
                    area += part;
                }
            }
            let color = |value: f32| if sum[3] > 0.0 { (value / sum[3] + 0.5) as u8 } else { 0 };
            let alpha = if area > 0.0 { (sum[3] / area * 255.0 + 0.5) as u8 } else { 0 };
            out.push([color(sum[0]), color(sum[1]), color(sum[2]), alpha]);
        }
    }
    out
}

/// 묶음 단추와 그 안에 든 단추들(무엇인지, 누를 수 있는지).
pub type Group = (Pick, Vec<(Pick, bool)>);

pub struct Fan {
    pub hwnd: HWND,
    at: (i32, i32),
    scale: f32,
    /// 창의 크기(100%일 때).
    size: (f32, f32),
    /// 지금 보이는 단추들: 무엇인지, 누를 수 있는지, 창 안에서의 가운데.
    buttons: Vec<(Pick, bool, (f32, f32))>,
    /// 단추들이 튀어나오는 곳.
    origin: (f32, f32),
    /// 처음 단추들과, 묶음마다 안에 든 단추들.
    top: Vec<(Pick, bool)>,
    groups: Vec<Group>,
    /// 머리 꼭대기와 벗어나면 안 되는 범위. 단계가 바뀌면 단추 수에 맞춰 다시 놓는다.
    head: (i32, i32),
    bounds: Rect,
    /// 지금 보이는 묶음. 처음 단추들이면 없다.
    level: Option<Pick>,
    /// 단추들이 머리로 들어가는 중이다. 다 들어간 뒤에 보일 묶음(처음 단추들이면 없음)과 들어가기 시작한 뒤 흐른 초.
    leaving: Option<(Option<Pick>, f32)>,
    /// 마우스가 올라가 있는 단추.
    hot: Option<usize>,
    /// 마우스가 단추 위에 있다. 한 번이라도 올라왔으면 `seen`이다.
    inside: bool,
    seen: bool,
    /// 마우스가 벗어나 있던 초.
    quiet: f32,
    /// 뜬 뒤 흐른 초.
    age: f32,
    since_drawn: f32,
    /// 그림을 다시 그려야 한다.
    stale: bool,
    /// 다 펼쳐진 모습을 그렸다.
    settled: bool,
    /// 한 번 그린 글자와 줄여 둔 그림은 다시 만들지 않는다.
    labels: HashMap<(String, i32, bool, bool), Block>,
    pictures: HashMap<(Pick, usize), Vec<[u8; 4]>>,
}

impl Fan {
    /// 머리 꼭대기(`head`) 위에 처음 단추들(`top`)을 띄울 준비를 한다. 창은 아직 없다.
    fn new((head, bounds): ((i32, i32), Rect), scale: f32, top: &[(Pick, bool)], groups: &[Group]) -> Fan {
        let mut fan = Fan {
            hwnd: null_mut(),
            at: (0, 0),
            scale,
            size: (0.0, 0.0),
            buttons: Vec::new(),
            origin: (0.0, 0.0),
            top: top.iter().copied().take(MOST).collect(),
            groups: groups.to_vec(),
            head,
            bounds,
            level: None,
            leaving: None,
            hot: None,
            inside: false,
            seen: false,
            quiet: 0.0,
            age: 0.0,
            since_drawn: 0.0,
            stale: true,
            settled: false,
            labels: HashMap::new(),
            pictures: HashMap::new(),
        };
        fan.arrange(None);
        fan
    }

    /// 그 단계(처음 단추들이면 `None`, 아니면 묶음)의 단추들. 묶음 안에서는 맨 앞에 되돌아가기가 있다.
    fn level_buttons(&self, level: Option<Pick>) -> Vec<(Pick, bool)> {
        match level.and_then(|group| self.groups.iter().find(|(pick, _)| *pick == group)) {
            Some((_, inner)) => std::iter::once((Pick::Back, true)).chain(inner.iter().copied()).take(MOST).collect(),
            None => self.top.clone(),
        }
    }

    /// 그 단계의 단추들을 머리 위에 늘어놓고, 처음부터 튀어나오게 한다.
    fn arrange(&mut self, level: Option<Pick>) {
        let buttons = self.level_buttons(level);
        let plan = plan(self.head, self.bounds, self.scale, buttons.len());
        self.buttons = buttons.iter().zip(plan.spots).map(|(&(pick, enabled), spot)| (pick, enabled, spot)).collect();
        (self.at, self.size, self.origin) = (plan.at, plan.size, plan.origin);
        (self.level, self.leaving, self.hot) = (level, None, None);
        (self.age, self.settled, self.stale) = (0.0, false, true);
    }

    /// 단추들이 다 들어가는 데 걸리는 초.
    fn retract_time(&self) -> f32 {
        RETRACT + STAGGER * self.buttons.len().saturating_sub(1) as f32
    }

    /// 단추 하나가 머리에서 얼마나 나와 있는지(0~1). `at`번째 단추다.
    fn out(&self, at: usize) -> f32 {
        if let Some((_, gone)) = self.leaving {
            // 오른쪽 끝의 단추부터 머리로 들어간다.
            let late = (self.buttons.len() - 1 - at) as f32 * STAGGER;
            let part = ((gone - late) / RETRACT).clamp(0.0, 1.0);
            return 1.0 - part.powi(3);
        }
        // 머리 꼭대기에서 제자리로 튀어나온다. 뒤의 단추일수록 조금 늦게 나온다.
        let part = ((self.age - at as f32 * STAGGER) / OPEN).clamp(0.0, 1.0);
        1.0 - (1.0 - part).powi(3)
    }

    /// 단추를 눌렀다. 묶음이나 되돌아가기면 단추들을 머리로 들이고 `None`을, 아니면 그 단추를 돌려준다.
    /// 단추들이 들어가는 동안에는 누른 것을 받지 않는다.
    fn press(&mut self, at: usize) -> Option<Pick> {
        let (pick, enabled, _) = *self.buttons.get(at)?;
        if !enabled || self.leaving.is_some() {
            return None;
        }
        if pick == Pick::Back || pick.is_group() {
            let target = (pick != Pick::Back).then_some(pick);
            (self.leaving, self.hot, self.stale) = (Some((target, 0.0)), None, true);
            return None;
        }
        Some(pick)
    }

    fn size_px(&self) -> (i32, i32) {
        ((self.size.0 * self.scale).round() as i32, (self.size.1 * self.scale).round() as i32)
    }

    /// 창 안의 한 점에 있는 단추가 몇 번째인지.
    fn hit(&self, x: f32, y: f32) -> Option<usize> {
        self.buttons.iter().position(|&(_, _, spot)| (x - spot.0).hypot(y - spot.1) <= BUTTON)
    }

    /// 단추의 이름이 뜨는 자리. 가로로는 한가운데다.
    fn name_spot(&self) -> (f32, f32) {
        let spots: Vec<(f32, f32)> = self.buttons.iter().map(|button| button.2).collect();
        (self.size.0 / 2.0, name_height(&spots))
    }

    /// 아직 펼쳐지거나 들어가는 중인가.
    fn is_opening(&self) -> bool {
        self.leaving.is_some() || self.age < OPEN + STAGGER * self.buttons.len() as f32
    }

    /// 한 줄짜리 글이나 아이콘 글자의 그림.
    fn label(&mut self, text: &str, size: f32, bold: bool, scale: f32) -> &Block {
        let icon = text.chars().next().is_some_and(|c| ('\u{E000}'..='\u{F8FF}').contains(&c));
        let pixels = (size * scale).round() as i32;
        self.labels
            .entry((text.to_string(), pixels, bold, icon))
            .or_insert_with(|| text::label(text, pixels, bold, icon))
    }

    /// 단추들을 그린다.
    fn paint(&mut self, surface: &mut Surface) {
        surface.clip_to(None);
        surface.clear();
        let scale = surface.scale;
        let shadow = rgb(0x141E46);
        for at in 0..self.buttons.len() {
            let (pick, enabled, spot) = self.buttons[at];
            let out = self.out(at);
            if out <= 0.0 {
                continue;
            }
            let center =
                (self.origin.0 + (spot.0 - self.origin.0) * out, self.origin.1 + (spot.1 - self.origin.1) * out);
            // 아래로 조금 번진 그림자.
            let under = (center.0, center.1 + 2.0);
            surface.each((under.0 - 27.0, under.1 - 27.0, under.0 + 27.0, under.1 + 27.0), |u, v, pixel| {
                let soft = ((BUTTON + 6.0 - (u - under.0).hypot(v - under.1)) / 8.0).clamp(0.0, 1.0);
                pixel.over(shadow, 0.26 * soft * soft * out);
            });
            let hot = enabled && self.hot == Some(at);
            surface.disc(center, BUTTON, PINK, out);
            surface.disc(center, BUTTON - 1.5, if hot { HOT } else { CREAM }, out);
            // 지금 할 수 없는 것은 흐리게 보인다.
            let ink = if enabled { out } else { 0.32 * out };
            if let Some(glyph) = pick.glyph() {
                let block = self.label(glyph, 16.0, false, scale);
                surface.stamp(block, center, ROSE, ink);
            } else if let Some(pixels) = source(pick) {
                let side = ((PICTURE * scale).round() as usize).clamp(1, SOURCE);
                let small = self.pictures.entry((pick, side)).or_insert_with(|| shrink(pixels, SOURCE, side));
                surface.picture(small, side as i32, center, ink);
            }
            // 묶음 단추에는 안에 단추가 더 있다는 점 셋을 아래에 찍는다.
            if pick.is_group() {
                for step in [-4.0, 0.0, 4.0] {
                    surface.disc((center.0 + step, center.1 + 11.5), 1.3, ROSE, ink);
                }
            }
        }
        // 마우스를 올린 단추의 이름. 부채꼴 안쪽의 한가운데, 양 끝 단추와 같은 높이에 뜬다.
        if let Some((pick, _, _)) = self.hot.map(|at| self.buttons[at]) {
            let middle = self.name_spot();
            let wide = self.label(pick.name(), 12.0, true, scale).w as f32 / scale + 18.0;
            let rect = (middle.0 - wide / 2.0, middle.1 - 11.0, middle.0 + wide / 2.0, middle.1 + 11.0);
            surface.frame(rect, 11.0, PINK, CREAM, 1.5);
            let block = self.label(pick.name(), 12.0, true, scale);
            surface.stamp(block, middle, DEEP, 1.0);
        }
    }
}

impl Postits {
    /// 머리 꼭대기의 가운데(`head`, 화면 좌표) 위에 단추들을 부채꼴로 띄운다. 떠 있던 것은 닫는다.
    /// `bounds`는 벗어나면 안 되는 범위, `dpi`는 그 모니터의 화면 배율이다. 단추마다 누를 수 있는지를 함께 준다.
    /// `groups`는 묶음 단추마다 안에 든 단추들이다. 한 단계에 [`MOST`]개까지 늘어선다.
    pub fn open_fan(&mut self, head: (i32, i32), bounds: Rect, dpi: u32, buttons: &[(Pick, bool)], groups: &[Group]) {
        self.close_fan();
        window::start();
        let scale = dpi.max(96) as f32 / 96.0;
        let mut fan = Fan::new((head, bounds), scale, buttons, groups);
        // 꽃핀이 늘 위에 떠 있을 수 있으므로, 단추는 그보다도 위에 띄운다.
        fan.hwnd = window::create(fan.at, fan.size_px(), true);
        let hwnd = fan.hwnd;
        self.fan = Some(fan);
        self.paint_fan();
        window::show(hwnd);
        self.wake();
    }

    /// 단추들을 거둔다.
    pub fn close_fan(&mut self) {
        if let Some(fan) = self.fan.take() {
            window::destroy(fan.hwnd);
        }
    }

    /// 단추들이 떠 있는가.
    pub fn has_fan(&self) -> bool {
        self.fan.is_some()
    }

    /// 단추들을 공용 버퍼에 그린다. 그린 그림의 크기(px)를 돌려준다.
    fn draw_fan(&mut self) -> Option<(i32, i32)> {
        self.fan.as_ref()?;
        self.ready();
        let (fan, tools) = (self.fan.as_mut()?, self.tools.as_mut()?);
        let size = fan.size_px();
        tools.canvas.fit(size.0, size.1);
        let stride = tools.canvas.width as usize;
        let mut surface = Surface::new(tools.canvas.pixels(), stride, size, fan.scale);
        fan.paint(&mut surface);
        (fan.stale, fan.since_drawn) = (false, 0.0);
        Some(size)
    }

    fn paint_fan(&mut self) {
        let Some(size) = self.draw_fan() else { return };
        if let (Some(fan), Some(tools)) = (&self.fan, &self.tools) {
            tools.canvas.present(fan.hwnd, fan.at, size);
        }
    }

    /// 단추들에서 일어난 일이면 처리하고 `true`를 돌려준다.
    pub(super) fn fan_event(&mut self, event: Event) -> bool {
        let Some(fan) = &mut self.fan else { return false };
        let (hwnd, scale) = (fan.hwnd, fan.scale);
        match event {
            Event::Press { hwnd: on, x, y } | Event::Double { hwnd: on, x, y } if on == hwnd => {
                window::stop_pressing();
                let pressed = fan.hit(x as f32 / scale, y as f32 / scale).and_then(|at| fan.press(at));
                if let Some(pick) = pressed {
                    self.close_fan();
                    self.outcomes.push(Outcome::Fan(pick));
                }
            }
            Event::Move { hwnd: on, x, y } if on == hwnd => {
                let hot = fan.hit(x as f32 / scale, y as f32 / scale).filter(|_| fan.leaving.is_none());
                fan.stale |= hot != fan.hot;
                (fan.hot, fan.inside, fan.seen, fan.quiet) = (hot, true, true, 0.0);
                let can_press = hot.is_some_and(|at| fan.buttons[at].1);
                window::set_cursor(if can_press { Cursor::Hand } else { Cursor::Arrow });
            }
            Event::Leave { hwnd: on } if on == hwnd => {
                fan.stale |= fan.hot.is_some();
                (fan.hot, fan.inside) = (None, false);
            }
            // 단추를 오른쪽 클릭하면 아무것도 하지 않고 닫는다.
            Event::Menu { hwnd: on } if on == hwnd => {
                self.close_fan();
                self.outcomes.push(Outcome::FanClosed);
            }
            Event::Lift { hwnd: on } | Event::Wheel { hwnd: on, .. } if on == hwnd => {}
            _ => return false,
        }
        true
    }

    /// 단추들의 시간을 흘린다. 펼쳐지는 동안과 모습이 바뀌었을 때 다시 그리고, 마우스가 벗어난 채로 오래 있으면
    /// 닫는다. 단추들이 떠 있으면 `true`를 돌려준다.
    pub(super) fn tick_fan(&mut self, dt: f32) -> bool {
        let Some(fan) = &mut self.fan else { return false };
        fan.age += dt;
        fan.since_drawn += dt;
        // 단추들이 다 들어갔으면 다음 단계의 단추들이 튀어나온다.
        if let Some((target, gone)) = fan.leaving {
            fan.leaving = Some((target, gone + dt));
            if gone + dt >= fan.retract_time() {
                fan.arrange(target);
            }
        }
        if !fan.inside {
            fan.quiet += dt;
        }
        if fan.quiet > if fan.seen { LINGER } else { UNSEEN } {
            self.close_fan();
            self.outcomes.push(Outcome::FanClosed);
            return false;
        }
        // 다 펼쳐진 모습은 꼭 한 번 그린다.
        if !fan.is_opening() && !fan.settled {
            (fan.settled, fan.stale) = (true, true);
        }
        if (fan.is_opening() || fan.stale) && fan.since_drawn >= REDRAW_EVERY {
            self.paint_fan();
        }
        true
    }

    /// 확인용: 지금의 단추들을 그린 그림과 그 자리. 꽃핀들의 그림 위에 얹어 보는 데 쓴다.
    pub fn fan_shot(&mut self) -> Option<(Rect, Vec<u32>)> {
        let size = self.draw_fan()?;
        let (fan, tools) = (self.fan.as_ref()?, self.tools.as_mut()?);
        let stride = tools.canvas.width as usize;
        let pixels = tools.canvas.pixels();
        let rows = (0..size.1 as usize).flat_map(|y| pixels[y * stride..y * stride + size.0 as usize].iter().copied());
        Some((Rect::new(fan.at.0, fan.at.1, size.0, size.1), rows.collect()))
    }

    /// 확인용: 사람이 단추들에서 하는 일을 흉내 낸다. 띄우는 것은 앱이 한다.
    pub fn fan_script(&mut self, step: &FanStep) {
        let Some(fan) = &mut self.fan else { return };
        let (hwnd, scale) = (fan.hwnd, fan.scale);
        let spot = |pick: Pick| fan.buttons.iter().find(|button| button.0 == pick).map(|button| button.2);
        let px = |units: f32| (units * scale).round() as i32;
        match step {
            FanStep::Open(_) => {}
            FanStep::Close => {
                self.fan_event(Event::Menu { hwnd });
            }
            FanStep::Press(pick) => {
                if let Some((x, y)) = spot(*pick) {
                    self.fan_event(Event::Press { hwnd, x: px(x), y: px(y) });
                }
            }
            FanStep::Hover(pick) => {
                if let Some((x, y)) = spot(*pick) {
                    self.fan_event(Event::Move { hwnd, x: px(x), y: px(y) });
                }
            }
            FanStep::Leave => {
                self.fan_event(Event::Leave { hwnd });
            }
            // 다 펼쳐진 모습으로 넘어간다. 그림을 뽑을 때 펼쳐지거나 들어가는 중간이 찍히지 않게 한다.
            FanStep::Settle => {
                if let Some((target, _)) = fan.leaving {
                    fan.arrange(target);
                }
                (fan.age, fan.stale) = (fan.age.max(1.0), true);
            }
        }
        self.wake();
    }

    /// 확인용: 단추들이 지금 어떤지를 적은 한 줄. 떠 있지 않으면 `None`이다.
    pub fn describe_fan(&self) -> Option<String> {
        let fan = self.fan.as_ref()?;
        let names: Vec<String> = fan
            .buttons
            .iter()
            .map(|&(pick, enabled, _)| if enabled { pick.name().to_string() } else { format!("({})", pick.name()) })
            .collect();
        let size = fan.size_px();
        Some(format!(
            "부채꼴 단추: {}, 자리 {:?}, 크기 {}x{}(배율 {:.0}%), 단추 [{}], 마우스 {}",
            fan.level.map_or("처음", Pick::name),
            fan.at,
            size.0,
            size.1,
            fan.scale * 100.0,
            names.join(", "),
            fan.hot.map_or("없음", |at| fan.buttons[at].0.name()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect::new(0, 0, 1920, 1040);

    #[test]
    fn buttons_fan_out_with_the_middle_highest() {
        for count in 1..=MOST {
            let spots = spread(count);
            assert_eq!(spots.len(), count);
            // 단추를 모두 띄워도 반원을 넘지 않는다. 양 끝의 단추가 머리 옆으로 내려오지 않는다.
            assert!(spots.iter().all(|spot| spot.1 < -BUTTON / 2.0), "{count}개: {spots:?}");
            // 이웃한 단추는 틈만큼 떨어져 있고, 모두 활의 중심에서 같은 거리에 있다.
            for pair in spots.windows(2) {
                let apart = (pair[1].0 - pair[0].0).hypot(pair[1].1 - pair[0].1);
                assert!((apart - (2.0 * BUTTON + GAP)).abs() < 0.01, "{count}개: {apart}");
                assert!(pair[1].0 > pair[0].0);
            }
            for spot in &spots {
                assert!((spot.0.hypot(spot.1) - RADIUS).abs() < 0.01);
            }
            // 왼쪽과 오른쪽이 맞서 있고, 가운데로 갈수록 높다.
            for (left, right) in spots.iter().zip(spots.iter().rev()) {
                assert!((left.0 + right.0).abs() < 0.01 && (left.1 - right.1).abs() < 0.01);
            }
            let highest = spots.iter().map(|spot| spot.1).fold(0.0, f32::min);
            assert!((spots[count / 2].1 - highest).abs() < 0.01);
        }
    }

    #[test]
    fn the_fan_sits_just_above_the_head() {
        let made = plan((1000, 600), SCREEN, 1.0, 6);
        // 가로로는 머리 가운데에 맞는다.
        assert_eq!(made.at.0, 1000 - (made.size.0 / 2.0).round() as i32);
        // 가장 아래 단추의 아래 끝이 머리 꼭대기 바로 위에 온다.
        let lowest = made.spots.iter().map(|spot| spot.1).fold(0.0, f32::max);
        assert!((made.at.1 as f32 + lowest + BUTTON + LIFT - 600.0).abs() <= 0.5);
        // 단추들은 머리 꼭대기에서 튀어나온다.
        assert!((made.at.0 as f32 + made.origin.0 - 1000.0).abs() <= 0.5);
        assert!((made.at.1 as f32 + made.origin.1 - 600.0).abs() <= 0.5);
        // 모든 단추가 창 안에 있다.
        for spot in &made.spots {
            assert!(spot.0 - BUTTON >= EDGE - 0.01 && spot.0 + BUTTON <= made.size.0 - EDGE + 0.01);
            assert!(spot.1 - BUTTON >= EDGE - 0.01 && spot.1 + BUTTON <= made.size.1 - EDGE + 0.01);
        }
        // 배율이 커지면 창도 그만큼 커지고, 머리에서 그만큼 더 떨어진다.
        let big = plan((1000, 600), SCREEN, 1.5, 6);
        assert_eq!(big.size, made.size);
        assert_eq!(big.at.1, 600 - (made.origin.1 * 1.5).round() as i32);
    }

    #[test]
    fn the_fan_stays_on_the_screen() {
        let size = plan((1000, 600), SCREEN, 1.0, 6).size;
        let (wide, tall) = (size.0.round() as i32, size.1.round() as i32);
        // 화면 위쪽 끝과 왼쪽 끝에서는 안쪽으로 밀린다.
        assert_eq!(plan((1000, 20), SCREEN, 1.0, 6).at.1, 0);
        assert_eq!(plan((10, 600), SCREEN, 1.0, 6).at.0, 0);
        assert_eq!(plan((1915, 600), SCREEN, 1.0, 6).at.0, 1920 - wide);
        // 아래로는 밀릴 일이 없다. 머리 위에 뜨기 때문이다.
        assert!(plan((1000, 1040), SCREEN, 1.0, 6).at.1 + tall <= 1040);
    }

    #[test]
    fn the_link_pictures_are_read_with_their_colors() {
        let picture = |pick: Pick| source(pick).unwrap_or_else(|| panic!("{} 그림을 풀지 못함", pick.name()));
        let at = |pixels: &[[u8; 4]], x: usize, y: usize| pixels[y * SOURCE + x];
        for pick in [Pick::Chzzk, Pick::Youtube, Pick::Cafe] {
            assert_eq!(picture(pick).len(), SOURCE * SOURCE);
        }
        // 치지직: 검은 바탕이 가장자리까지 차 있고, 가운데에 초록 글자가 있다.
        let chzzk = picture(Pick::Chzzk);
        assert_eq!(at(chzzk, 48, 4), [0, 0, 0, 255]);
        let [blue, green, red, alpha] = at(chzzk, 48, 48);
        assert!(green > 200 && red < 80 && blue > 100 && alpha == 255, "{:?}", at(chzzk, 48, 48));
        // 유튜브: 빨간 바탕 한가운데에 흰 세모가 있고, 위아래는 비어 있다.
        let youtube = picture(Pick::Youtube);
        assert_eq!(at(youtube, 50, 48), [255, 255, 255, 255]);
        let [blue, green, red, alpha] = at(youtube, 16, 48);
        assert!(red > 240 && green < 40 && blue < 80 && alpha == 255, "{:?}", at(youtube, 16, 48));
        assert_eq!(at(youtube, 48, 2)[3], 0);
        // 가장자리의 반쯤 투명한 픽셀도 색은 빨강 그대로다. 알파가 색에 곱해져 있지 않다.
        let edges: Vec<&[u8; 4]> = youtube.iter().filter(|pixel| (40..216).contains(&pixel[3])).collect();
        assert!(!edges.is_empty() && edges.iter().all(|pixel| pixel[2] > 200), "{:?}", &edges[..edges.len().min(4)]);
        // 네이버 카페: 초록 찻잔. 모서리는 비어 있다.
        let cafe = picture(Pick::Cafe);
        let [blue, green, red, alpha] = at(cafe, 40, 60);
        assert!(green > 150 && red < 60 && blue < 140 && alpha == 255, "{:?}", at(cafe, 40, 60));
        assert_eq!(at(cafe, 95, 95)[3], 0);
        // 글자로 그리는 단추에는 그림이 없다. 단추마다 글자나 그림 가운데 하나가 있다.
        assert!(source(Pick::Invite).is_none() && Pick::Invite.glyph().is_some());
        assert!(Pick::ALL.iter().all(|pick| pick.glyph().is_some() != pick.picture().is_some()));
    }

    #[test]
    fn a_shrunk_picture_keeps_its_colors() {
        // 왼쪽 절반은 불투명한 빨강, 오른쪽 절반은 투명한 초록.
        let side = 8;
        let pixels: Vec<[u8; 4]> =
            (0..side * side).map(|at| if at % side < 4 { [0, 0, 255, 255] } else { [0, 255, 0, 0] }).collect();
        let small = shrink(&pixels, side, 2);
        assert_eq!(small, [[0, 0, 255, 255], [0, 0, 0, 0], [0, 0, 255, 255], [0, 0, 0, 0]]);
        // 반씩 걸치면 색은 불투명한 쪽의 것이고 알파만 절반이 된다. 투명한 곳의 색은 섞이지 않는다.
        assert_eq!(shrink(&pixels, side, 1), [[0, 0, 255, 128]]);
        // 나누어떨어지지 않게 줄여도 넓이만큼 섞인다.
        let third = shrink(&pixels, side, 3);
        assert_eq!((third[0], third[2]), ([0, 0, 255, 255], [0, 0, 0, 0]));
        assert!(matches!(third[1], [0, 0, 255, 127..=128]), "{:?}", third[1]);
    }

    #[test]
    fn drawn_buttons_can_be_pressed() {
        text::load_fonts();
        let buttons = [(Pick::Invite, false), (Pick::Send, true), (Pick::Memos, true), (Pick::Settings, true)];
        let made = plan((1000, 600), SCREEN, 1.0, buttons.len());
        let mut fan = Fan::new(((1000, 600), SCREEN), 1.0, &buttons, &[]);
        let (wide, tall) = fan.size_px();
        let mut pixels = vec![0u32; (wide * tall) as usize];
        let alpha = |pixels: &[u32], spot: (f32, f32)| pixels[spot.1 as usize * wide as usize + spot.0 as usize] >> 24;
        // 막 떴을 때는 아직 아무것도 나오지 않았다.
        fan.paint(&mut Surface::new(&mut pixels, wide as usize, (wide, tall), 1.0));
        assert!(made.spots.iter().all(|&spot| alpha(&pixels, spot) == 0));
        // 다 펼쳐지면 단추가 제자리에 있고, 그 자리를 누를 수 있다.
        fan.age = 1.0;
        fan.hot = Some(1);
        fan.paint(&mut Surface::new(&mut pixels, wide as usize, (wide, tall), 1.0));
        for (at, &spot) in made.spots.iter().enumerate() {
            assert_eq!(alpha(&pixels, spot), 255, "{at}번 단추");
            assert_eq!(fan.hit(spot.0, spot.1), Some(at));
            assert_eq!(fan.hit(spot.0, spot.1 - BUTTON - 2.0), None);
        }
        // 마우스를 올린 단추의 이름이 부채꼴 안쪽 한가운데에 떠 있다. 올린 단추가 없으면 그 자리는 비어 있다.
        let name = fan.name_spot();
        assert!(name.1 > made.spots[1].1 && (name.0 - made.size.0 / 2.0).abs() < 0.01);
        assert_eq!(alpha(&pixels, name), 255);
        fan.hot = None;
        fan.paint(&mut Surface::new(&mut pixels, wide as usize, (wide, tall), 1.0));
        assert_eq!(alpha(&pixels, name), 0);
        // 단추 사이의 빈 곳은 비어 있어서, 클릭이 뒤로 넘어간다.
        let between = ((made.spots[1].0 + made.spots[2].0) / 2.0, made.spots[1].1 + BUTTON);
        assert_eq!(alpha(&pixels, between), 0);
    }

    #[test]
    fn a_group_opens_in_place_and_goes_back() {
        let links = [(Pick::Chzzk, true), (Pick::Youtube, true), (Pick::Cafe, false)];
        let top = [(Pick::Invite, true), (Pick::Memos, true), (Pick::Links, true), (Pick::Settings, true)];
        let mut fan = Fan::new(((1000, 600), SCREEN), 1.0, &top, &[(Pick::Links, links.to_vec())]);
        let picks = |fan: &Fan| fan.buttons.iter().map(|button| button.0).collect::<Vec<_>>();
        fan.age = 1.0;
        // 보통 단추는 그대로 돌려준다.
        assert_eq!(fan.press(1), Some(Pick::Memos));
        // 묶음 단추를 누르면 단추들이 오른쪽 끝부터 머리로 들어간다. 그동안은 누른 것을 받지 않는다.
        assert_eq!(fan.press(2), None);
        assert_eq!(fan.level, None);
        fan.leaving = Some((Some(Pick::Links), RETRACT * 0.5));
        assert!(fan.out(3) < fan.out(0) && fan.out(0) > 0.99);
        assert_eq!(fan.press(1), None);
        // 다 들어가면 묶음 안의 단추들이 맨 앞의 되돌아가기와 함께, 단추 수에 맞춰 다시 놓인다.
        fan.arrange(Some(Pick::Links));
        assert_eq!(picks(&fan), [Pick::Back, Pick::Chzzk, Pick::Youtube, Pick::Cafe]);
        assert_eq!(fan.level, Some(Pick::Links));
        assert!(fan.age == 0.0 && fan.out(0) == 0.0);
        assert_eq!(fan.size, plan((1000, 600), SCREEN, 1.0, 4).size);
        fan.age = 1.0;
        // 흐린 단추는 눌러도 아무 일이 없다.
        assert_eq!(fan.press(3), None);
        assert!(fan.leaving.is_none());
        assert_eq!(fan.press(1), Some(Pick::Chzzk));
        // 되돌아가기를 누르면 같은 움직임으로 처음 단추들이 돌아온다.
        assert_eq!(fan.press(0), None);
        assert_eq!(fan.leaving.map(|leaving| leaving.0), Some(None));
        fan.arrange(None);
        assert_eq!(picks(&fan), [Pick::Invite, Pick::Memos, Pick::Links, Pick::Settings]);
        // 한 단계에는 많아야 MOST개만 늘어선다.
        let many: Vec<(Pick, bool)> = Pick::ALL.iter().map(|&pick| (pick, true)).collect();
        assert_eq!(Fan::new(((1000, 600), SCREEN), 1.0, &many, &[]).buttons.len(), MOST);
    }
}
