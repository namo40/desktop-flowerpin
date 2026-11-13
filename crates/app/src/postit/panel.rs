//! 포스트잇에 딸린 작은 판: 색 고르기와 알림 정하기. 꽃핀 옆에 뜨는 쉬는 시간 알림 판도 같은 방식으로 그린다.
//!
//! Windows의 기본 컨트롤을 쓰지 않는다. 단추, 고르는 칸, 미는 막대를 모두 직접 그리고, 마우스가 닿은 곳도
//! 직접 가린다. 판은 포스트잇 옆에 뜨는 창 하나이고, 포스트잇처럼 픽셀마다 투명도가 있다.
//!
//! 판은 포스트잇을 직접 고치지 않는다. 무엇을 눌렀는지([`Action`])를 알려 주기만 한다.

use std::collections::HashMap;

use windows_sys::Win32::Foundation::HWND;

use super::alarm::Alarm;
use super::draw::{COLORS, CREAM, DEEP, HOT, INK, PINK, Pixel, ROSE, Rgb, WHITE, from_hsv, mix, rgb, round_box};
use super::text::{self, Block};
use crate::focus::{FOCUS_PICKS, REST_PICKS};
use crate::when::{civil_from_days, clock_label as clock, weekday};

/// 판 둘레에 그림자가 들어갈 여백.
const MARGIN: f32 = 14.0;
/// 아이콘 글꼴의 글자: 닫기, 위, 아래, 왼쪽, 오른쪽.
const CLOSE: &str = "\u{E711}";
const UP: &str = "\u{E70E}";
const DOWN: &str = "\u{E70D}";
const LEFT: &str = "\u{E76B}";
const RIGHT: &str = "\u{E76C}";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Color,
    Alarm,
    /// 쉬는 시간 알림. 포스트잇에 딸리지 않는다.
    Focus,
}

/// 판에서 누른 것.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    Close,
    /// 기본 색 가운데 하나.
    Swatch(usize),
    /// 미는 막대: 0 색상, 1 진하기, 2 밝기.
    Slide(usize),
    /// 알림 종류: 0 없음, 1 한 번, 2 날마다.
    Mode(u8),
    /// 빨리 고르기: 0 10분 뒤, 1 1시간 뒤, 2 내일 아침.
    Quick(u8),
    /// 오전(false)과 오후(true).
    Half(bool),
    Hour(i32),
    Minute(i32),
    Day(i64),
    /// 날짜 줄을 한 주 앞뒤로 넘긴다.
    Week(i32),
    /// 쉬는 시간 알림: 집중 시간(0)이나 쉬는 시간(1)을 그 분으로 고른다.
    FocusPick(u8, u32),
    /// 쉬는 시간 알림: 집중 시간(0)이나 쉬는 시간(1)을 화살표로 몇 칸 바꾼다.
    FocusStep(u8, i32),
    /// 쉬는 시간 알림: 저절로 이어 가기를 켜고 끈다.
    FocusAuto,
    /// 쉬는 시간 알림의 아래쪽 단추: 0 시작, 1 잠깐 멈추기, 2 다시, 3 그만하기, 4 지금 쉬기·바로 집중.
    FocusGo(u8),
}

/// 쉬는 시간 알림 판이 보여 줄 것.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FocusModel {
    /// 지금 하는 것. 돌고 있지 않으면 없고, 집중하는 중이면 `true`다.
    pub phase: Option<bool>,
    pub paused: bool,
    /// 자리를 비워 집중 시간이 멈춰 있다.
    pub away: bool,
    /// 남은 초와, 지금 하는 것의 전체 초.
    pub left: u64,
    pub total: u64,
    /// 판에서 고른 집중 시간과 쉬는 시간(분), 저절로 이어 가기.
    pub focus: u32,
    pub rest: u32,
    pub auto: bool,
}

/// 판이 보여 줄 것.
pub struct Model {
    pub color: Rgb,
    /// 색을 직접 고르는 막대의 값(색상, 진하기, 밝기).
    pub hsv: (f32, f32, f32),
    pub mode: u8,
    pub day: i64,
    pub hour: u32,
    pub minute: u32,
    pub today: i64,
    /// 날짜 줄의 첫 날.
    pub week: i64,
    /// 고른 때가 이미 지났다.
    pub past: bool,
}

/// 그릴 곳. 좌표는 화면 배율 100%일 때의 픽셀이다.
pub struct Surface<'a> {
    pub pixels: &'a mut [u32],
    pub stride: usize,
    pub width: i32,
    pub height: i32,
    pub scale: f32,
    /// 이 안에만 그린다(px: 왼쪽, 위, 오른쪽, 아래). 넘치는 목록을 잘라 그릴 때 좁힌다.
    pub clip: (i32, i32, i32, i32),
}

impl<'a> Surface<'a> {
    pub fn new(pixels: &'a mut [u32], stride: usize, (width, height): (i32, i32), scale: f32) -> Surface<'a> {
        Surface { pixels, stride, width, height, scale, clip: (0, 0, width, height) }
    }

    /// `rect`(100%일 때의 좌표) 안에만 그리게 한다. `None`이면 다시 전체에 그린다.
    pub fn clip_to(&mut self, rect: Option<(f32, f32, f32, f32)>) {
        let (scale, width, height) = (self.scale, self.width, self.height);
        let px = |value: f32, limit: i32| ((value * scale).round() as i32).clamp(0, limit);
        self.clip = match rect {
            Some(rect) => (px(rect.0, width), px(rect.1, height), px(rect.2, width), px(rect.3, height)),
            None => (0, 0, width, height),
        };
    }

    pub fn clear(&mut self) {
        for row in 0..self.height as usize {
            self.pixels[row * self.stride..row * self.stride + self.width as usize].fill(0);
        }
    }

    /// `rect`(왼쪽, 위, 오른쪽, 아래)에 걸친 픽셀마다 `paint`를 부른다.
    pub fn each(&mut self, rect: (f32, f32, f32, f32), mut paint: impl FnMut(f32, f32, &mut Pixel)) {
        let clip = self.clip;
        let edge = |value: f32, least: i32, most: i32| ((value * self.scale) as i32).clamp(least, most.max(least));
        let (left, top) = (edge(rect.0 - 1.0, clip.0, clip.2), edge(rect.1 - 1.0, clip.1, clip.3));
        let (right, bottom) = (edge(rect.2 + 2.0, clip.0, clip.2), edge(rect.3 + 2.0, clip.1, clip.3));
        for py in top..bottom {
            for px in left..right {
                let at = py as usize * self.stride + px as usize;
                let mut pixel = Pixel::unpack(self.pixels[at]);
                paint((px as f32 + 0.5) / self.scale, (py as f32 + 0.5) / self.scale, &mut pixel);
                self.pixels[at] = pixel.pack(1.0);
            }
        }
    }

    /// 모서리가 둥근 네모를 칠한다.
    pub fn pill(&mut self, rect: (f32, f32, f32, f32), radius: f32, color: Rgb, alpha: f32) {
        let scale = self.scale;
        self.each(rect, |u, v, pixel| {
            pixel.over(color, (0.5 - round_box(u, v, rect, radius) * scale).clamp(0.0, 1.0) * alpha)
        });
    }

    /// 테두리가 있는 둥근 네모.
    pub fn frame(&mut self, rect: (f32, f32, f32, f32), radius: f32, line: Rgb, fill: Rgb, thick: f32) {
        let scale = self.scale;
        self.each(rect, |u, v, pixel| {
            let distance = round_box(u, v, rect, radius);
            pixel.over(line, (0.5 - distance * scale).clamp(0.0, 1.0));
            pixel.over(fill, (0.5 - (distance + thick) * scale).clamp(0.0, 1.0));
        });
    }

    /// 판 아래에 깔리는 부드러운 그림자.
    pub fn shadow(&mut self, rect: (f32, f32, f32, f32), radius: f32) {
        let wide = (rect.0 - 12.0, rect.1 - 8.0, rect.2 + 12.0, rect.3 + 16.0);
        self.each(wide, |u, v, pixel| {
            let soft = (0.5 - round_box(u, v - 4.0, rect, radius) / 20.0).clamp(0.0, 1.0);
            pixel.over(rgb(0x141E46), 0.28 * soft * soft * (3.0 - 2.0 * soft));
        });
    }

    pub fn disc(&mut self, center: (f32, f32), radius: f32, color: Rgb, alpha: f32) {
        let scale = self.scale;
        let rect = (center.0 - radius, center.1 - radius, center.0 + radius, center.1 + radius);
        self.each(rect, |u, v, pixel| {
            let distance = ((u - center.0).powi(2) + (v - center.1).powi(2)).sqrt() - radius;
            pixel.over(color, (0.5 - distance * scale).clamp(0.0, 1.0) * alpha);
        });
    }

    /// 왼쪽에서 오른쪽으로 색이 바뀌는 막대. `color`는 0~1을 받아 그 자리의 색을 돌려준다.
    pub fn bar(&mut self, rect: (f32, f32, f32, f32), color: impl Fn(f32) -> Rgb) {
        let scale = self.scale;
        let radius = (rect.3 - rect.1) / 2.0;
        self.each(rect, |u, v, pixel| {
            let distance = round_box(u, v, rect, radius);
            pixel.over(PINK, (0.5 - distance * scale).clamp(0.0, 1.0));
            let t = ((u - rect.0 - radius) / (rect.2 - rect.0 - 2.0 * radius)).clamp(0.0, 1.0);
            pixel.over(color(t), (0.5 - (distance + 1.0) * scale).clamp(0.0, 1.0));
        });
    }

    /// 그림을 찍는다. `pixels`는 한 변이 `side`px인 정사각형이고, 알파를 곱하지 않은 BGRA다.
    /// `center`가 그림의 한가운데다.
    pub fn picture(&mut self, pixels: &[[u8; 4]], side: i32, center: (f32, f32), alpha: f32) {
        let left = (center.0 * self.scale - side as f32 / 2.0).round() as i32;
        let top = (center.1 * self.scale - side as f32 / 2.0).round() as i32;
        let clip = self.clip;
        for y in 0..side {
            for x in 0..side {
                let (px, py) = (left + x, top + y);
                let [blue, green, red, cover] = pixels[(y * side + x) as usize];
                if cover == 0 || px < clip.0 || py < clip.1 || px >= clip.2 || py >= clip.3 {
                    continue;
                }
                let at = py as usize * self.stride + px as usize;
                let mut pixel = Pixel::unpack(self.pixels[at]);
                let color = (red as f32 / 255.0, green as f32 / 255.0, blue as f32 / 255.0);
                pixel.over(color, cover as f32 / 255.0 * alpha);
                self.pixels[at] = pixel.pack(1.0);
            }
        }
    }

    /// 미리 그려 둔 글자를 그 색으로 찍는다. `center`가 글자의 한가운데다.
    pub fn stamp(&mut self, block: &Block, center: (f32, f32), color: Rgb, alpha: f32) {
        let left = (center.0 * self.scale - block.w as f32 / 2.0).round() as i32;
        let top = (center.1 * self.scale - block.h as f32 / 2.0).round() as i32;
        for y in 0..block.h {
            for x in 0..block.w {
                let (px, py) = (left + x, top + y);
                let cover = block.cover[(y * block.w + x) as usize];
                let clip = self.clip;
                if cover == [0; 3] || px < clip.0 || py < clip.1 || px >= clip.2 || py >= clip.3 {
                    continue;
                }
                let at = py as usize * self.stride + px as usize;
                let mut pixel = Pixel::unpack(self.pixels[at]);
                pixel.tint(color, cover.map(|value| value as f32 / 255.0 * alpha));
                self.pixels[at] = pixel.pack(1.0);
            }
        }
    }
}

pub struct Panel {
    pub hwnd: HWND,
    pub kind: Kind,
    /// 이 판이 딸린 포스트잇의 창.
    pub note: HWND,
    /// 마우스가 올라가 있는 것.
    pub hot: Option<Action>,
    /// 밀고 있는 막대.
    pub sliding: Option<usize>,
    /// 마지막으로 그렸을 때, 누를 수 있는 곳들.
    spots: Vec<((f32, f32, f32, f32), Action)>,
    /// 한 번 그린 글자는 다시 그리지 않는다.
    labels: HashMap<(String, i32, bool, bool), Block>,
}

impl Panel {
    pub fn new(hwnd: HWND, kind: Kind, note: HWND) -> Panel {
        Panel { hwnd, kind, note, hot: None, sliding: None, spots: Vec::new(), labels: HashMap::new() }
    }

    /// 판의 크기. 둘레의 그림자 여백을 더한 것이다.
    pub fn size(kind: Kind) -> (f32, f32) {
        let inner = match kind {
            Kind::Color => (236.0, 226.0),
            Kind::Alarm => (264.0, 254.0),
            Kind::Focus => (264.0, 282.0),
        };
        (inner.0 + 2.0 * MARGIN, inner.1 + 2.0 * MARGIN + 6.0)
    }

    /// 마지막으로 그렸을 때 그것을 누를 수 있던 곳. 확인용 스크립트가 누를 자리를 찾는 데 쓴다.
    pub fn spot(&self, action: Action) -> Option<(f32, f32, f32, f32)> {
        self.spots.iter().find(|(_, other)| *other == action).map(|&(rect, _)| rect)
    }

    /// 판 안의 한 점에 있는, 누를 수 있는 것.
    pub fn hit(&self, x: f32, y: f32) -> Option<Action> {
        let found =
            self.spots.iter().find(|(rect, _)| (rect.0..=rect.2).contains(&x) && (rect.1..=rect.3).contains(&y));
        found.map(|&(_, action)| action)
    }

    /// 시나 분의 칸 위에서 휠을 굴렸으면, 그 값을 `step`만큼 바꾸는 일을 돌려준다.
    pub fn wheel(&self, x: f32, y: f32, step: i32) -> Option<Action> {
        for (rect, action) in &self.spots {
            // 위쪽 화살표가 놓인 자리로 칸 전체의 범위를 안다.
            let whole = (rect.0 - 46.0, rect.1 - 2.0, rect.0 + 22.0, rect.1 + 42.0);
            if !((whole.0..=whole.2).contains(&x) && (whole.1..=whole.3).contains(&y)) {
                continue;
            }
            match action {
                Action::Hour(1) => return Some(Action::Hour(step)),
                Action::Minute(1) => return Some(Action::Minute(step)),
                Action::FocusStep(which, 1) => return Some(Action::FocusStep(*which, step)),
                _ => {}
            }
        }
        None
    }

    /// 미는 막대 위의 가로 자리를 0~1의 값으로 바꾼다.
    pub fn slider_value(&self, which: usize, x: f32) -> f32 {
        let rect = self.spots.iter().find(|(_, action)| *action == Action::Slide(which)).map(|&(rect, _)| rect);
        rect.map_or(0.0, |rect| ((x - rect.0 - 7.0) / (rect.2 - rect.0 - 14.0)).clamp(0.0, 1.0))
    }

    fn label(&mut self, text: &str, size: f32, bold: bool, icon: bool, scale: f32) -> &Block {
        let pixels = (size * scale).round() as i32;
        self.labels
            .entry((text.to_string(), pixels, bold, icon))
            .or_insert_with(|| text::label(text, pixels, bold, icon))
    }

    /// 글을 `center`에 찍는다.
    #[allow(clippy::too_many_arguments)]
    fn write(
        &mut self,
        surface: &mut Surface,
        text: &str,
        size: f32,
        bold: bool,
        center: (f32, f32),
        color: Rgb,
        alpha: f32,
    ) {
        let scale = surface.scale;
        let icon = text.chars().next().is_some_and(|c| ('\u{E000}'..='\u{F8FF}').contains(&c));
        let block = self.label(text, size, bold, icon, scale);
        // 빌린 것을 놓고 찍으려고 필요한 것만 꺼낸다.
        let block = Block { w: block.w, h: block.h, cover: block.cover.clone() };
        surface.stamp(&block, center, color, alpha);
    }

    /// 글을 `left`에서 시작하게 찍는다.
    #[allow(clippy::too_many_arguments)]
    fn write_left(
        &mut self,
        surface: &mut Surface,
        text: &str,
        size: f32,
        bold: bool,
        left: f32,
        middle: f32,
        color: Rgb,
        alpha: f32,
    ) {
        let scale = surface.scale;
        let width = self.label(text, size, bold, false, scale).w as f32 / scale;
        self.write(surface, text, size, bold, (left + width / 2.0, middle), color, alpha);
    }

    /// 누를 수 있는 둥근 단추 하나를 그리고, 누를 곳으로 적어 둔다.
    #[allow(clippy::too_many_arguments)]
    fn button(
        &mut self,
        surface: &mut Surface,
        rect: (f32, f32, f32, f32),
        text: &str,
        size: f32,
        action: Action,
        chosen: bool,
    ) {
        let hot = self.hot == Some(action);
        let radius = ((rect.3 - rect.1) / 2.0).min(14.0);
        if chosen {
            surface.pill(rect, radius, ROSE, 1.0);
        } else {
            surface.frame(rect, radius, PINK, if hot { HOT } else { CREAM }, 1.5);
        }
        let center = ((rect.0 + rect.2) / 2.0, (rect.1 + rect.3) / 2.0);
        self.write(surface, text, size, chosen, center, if chosen { WHITE } else { DEEP }, 1.0);
        self.spots.push((rect, action));
    }

    /// 판을 그린다. 누를 수 있는 곳도 이때 다시 적는다.
    pub fn paint(&mut self, surface: &mut Surface, model: &Model) {
        let title = if self.kind == Kind::Color { "색" } else { "알림" };
        let (x, y) = self.chrome(surface, title);
        match self.kind {
            Kind::Color => self.paint_color(surface, model, (x, y)),
            Kind::Alarm => self.paint_alarm(surface, model, (x, y)),
            Kind::Focus => {}
        }
    }

    /// 판의 바탕과 제목, 닫기 단추를 그린다. 판 안쪽의 왼쪽 위를 돌려준다.
    fn chrome(&mut self, surface: &mut Surface, title: &str) -> (f32, f32) {
        self.spots.clear();
        surface.clear();
        let (width, height) = Panel::size(self.kind);
        let body = (MARGIN, MARGIN, width - MARGIN, height - MARGIN - 6.0);
        surface.shadow(body, 16.0);
        surface.frame(body, 16.0, PINK, CREAM, 2.0);
        let (x, y) = (body.0, body.1);
        self.write_left(surface, title, 14.0, true, x + 16.0, y + 22.0, ROSE, 1.0);
        // 닫기.
        let close = (body.2 - 36.0, y + 10.0, body.2 - 12.0, y + 34.0);
        if self.hot == Some(Action::Close) {
            surface.disc(((close.0 + close.2) / 2.0, (close.1 + close.3) / 2.0), 12.0, HOT, 1.0);
        }
        self.write(surface, CLOSE, 11.0, false, ((close.0 + close.2) / 2.0, (close.1 + close.3) / 2.0), ROSE, 1.0);
        self.spots.push((close, Action::Close));
        (x, y)
    }

    /// 쉬는 시간 알림 판을 그린다. 지금 상태와 남은 시간, 고른 시간, 저절로 이어 가기, 아래쪽 단추들이다.
    pub fn paint_focus(&mut self, surface: &mut Surface, model: &FocusModel) {
        let (x, y) = self.chrome(surface, "쉬는 시간 알림");
        let (left, right) = (x + 14.0, x + 250.0);
        let scale = surface.scale;

        // 지금 상태와 남은 시간.
        let state = match model.phase {
            None => "쉬고 있지 않아요",
            Some(_) if model.paused => "잠깐 멈춤",
            Some(true) if model.away => "자리 비움으로 멈춤",
            Some(true) => "집중하는 중",
            Some(false) => "쉬는 중",
        };
        let lively = model.phase.is_some() && !model.paused && !(model.away && model.phase == Some(true));
        self.write_left(surface, state, 13.0, true, left + 2.0, y + 58.0, if lively { ROSE } else { INK }, 1.0);
        let bar = (left, y + 76.0, right, y + 84.0);
        surface.pill(bar, 4.0, PINK, 0.35);
        if model.phase.is_some() {
            let clock = format!("{}:{:02}", model.left / 60, model.left % 60);
            let wide = self.label(&clock, 22.0, true, false, scale).w as f32 / scale;
            self.write(surface, &clock, 22.0, true, (right - 2.0 - wide / 2.0, y + 57.0), INK, 1.0);
            // 지난 만큼 채운다.
            let done = 1.0 - model.left as f32 / model.total.max(1) as f32;
            let reach = bar.0 + (bar.2 - bar.0) * done.clamp(0.0, 1.0);
            if reach > bar.0 + 8.0 {
                surface.pill((bar.0, bar.1, reach, bar.3), 4.0, ROSE, if lively { 1.0 } else { 0.5 });
            }
        } else {
            let plan = format!("{}분 집중 + {}분 쉬기", model.focus, model.rest);
            let wide = self.label(&plan, 12.5, false, false, scale).w as f32 / scale;
            self.write(surface, &plan, 12.5, false, (right - 2.0 - wide / 2.0, y + 58.0), INK, 0.75);
        }

        // 집중 시간과 쉬는 시간: 빨리 고르는 단추 둘과, 화살표나 휠로 바꾸는 칸.
        let rows = [("집중", model.focus, FOCUS_PICKS), ("쉬기", model.rest, REST_PICKS)];
        for (row, (name, value, picks)) in rows.into_iter().enumerate() {
            let top = y + 98.0 + 52.0 * row as f32;
            self.write_left(surface, name, 12.5, true, left + 2.0, top + 22.0, DEEP, 1.0);
            for (index, minutes) in picks.into_iter().enumerate() {
                let from = left + 38.0 + 60.0 * index as f32;
                let rect = (from, top + 8.0, from + 54.0, top + 36.0);
                let label = format!("{minutes}분");
                self.button(surface, rect, &label, 12.5, Action::FocusPick(row as u8, minutes), minutes == value);
            }
            let from = right - 68.0;
            surface.frame((from, top, from + 68.0, top + 44.0), 10.0, PINK, WHITE, 1.5);
            self.write(surface, &value.to_string(), 20.0, true, (from + 23.0, top + 22.0), INK, 1.0);
            let arrows = [
                (Action::FocusStep(row as u8, 1), UP, top + 2.0),
                (Action::FocusStep(row as u8, -1), DOWN, top + 22.0),
            ];
            for (action, arrow, arrow_top) in arrows {
                let spot = (from + 46.0, arrow_top, from + 66.0, arrow_top + 20.0);
                if self.hot == Some(action) {
                    surface.pill(spot, 6.0, HOT, 1.0);
                }
                self.write(surface, arrow, 12.0, false, ((spot.0 + spot.2) / 2.0, (spot.1 + spot.3) / 2.0), ROSE, 1.0);
                self.spots.push((spot, action));
            }
        }

        // 저절로 이어 가기. 작은 스위치와 이름이다.
        let top = y + 204.0;
        let switch = (left + 2.0, top + 2.0, left + 38.0, top + 22.0);
        if model.auto {
            surface.pill(switch, 10.0, ROSE, 1.0);
        } else {
            surface.frame(switch, 10.0, PINK, if self.hot == Some(Action::FocusAuto) { HOT } else { WHITE }, 1.5);
        }
        let knob = (if model.auto { switch.2 - 10.0 } else { switch.0 + 10.0 }, top + 12.0);
        surface.disc(knob, 7.0, if model.auto { WHITE } else { PINK }, 1.0);
        self.write_left(surface, "저절로 이어 가기", 12.5, false, left + 46.0, top + 12.0, INK, 0.9);
        self.spots.push(((left, top, right, top + 24.0), Action::FocusAuto));

        // 아래쪽 단추들.
        let row = (y + 238.0, y + 266.0);
        match model.phase {
            None => self.button(surface, (left, row.0, right, row.1), "시작", 13.0, Action::FocusGo(0), true),
            Some(focusing) => {
                let third = (right - left - 12.0) / 3.0;
                let (pause, pause_action) = if model.paused { ("다시", 2) } else { ("멈추기", 1) };
                let skip = if focusing { "지금 쉬기" } else { "바로 집중" };
                for (index, (name, action)) in
                    [(pause, pause_action), (skip, 4), ("그만하기", 3)].into_iter().enumerate()
                {
                    let from = left + (third + 6.0) * index as f32;
                    self.button(
                        surface,
                        (from, row.0, from + third, row.1),
                        name,
                        12.5,
                        Action::FocusGo(action),
                        false,
                    );
                }
            }
        }
    }

    fn paint_color(&mut self, surface: &mut Surface, model: &Model, (x, y): (f32, f32)) {
        // 기본 색 열 가지. 다섯 개씩 두 줄이다.
        for (index, &color) in COLORS.iter().enumerate() {
            let center = (x + 34.0 + 42.0 * (index % 5) as f32, y + 62.0 + 40.0 * (index / 5) as f32);
            let same =
                (color.0 - model.color.0).abs() + (color.1 - model.color.1).abs() + (color.2 - model.color.2).abs()
                    < 0.01;
            let action = Action::Swatch(index);
            if same {
                surface.disc(center, 19.5, ROSE, 1.0);
                surface.disc(center, 17.0, CREAM, 1.0);
            } else if self.hot == Some(action) {
                surface.disc(center, 18.5, HOT, 1.0);
            }
            surface.disc(center, 15.0, mix(color, INK, 0.25), 1.0);
            surface.disc(center, 14.0, color, 1.0);
            self.spots.push(((center.0 - 18.0, center.1 - 18.0, center.0 + 18.0, center.1 + 18.0), action));
        }
        self.write_left(surface, "직접 고르기", 12.5, false, x + 16.0, y + 132.0, INK, 0.85);
        // 색상, 진하기, 밝기를 미는 막대.
        let (hue, saturation, value) = model.hsv;
        for which in 0..3 {
            let top = y + 148.0 + 26.0 * which as f32;
            let rect = (x + 16.0, top, x + 220.0, top + 14.0);
            let at = [hue, saturation, value][which];
            match which {
                // 색상 막대는 지금의 진하기와 밝기로 보여 주되, 너무 흐리거나 어두우면 색을 알아볼 수 없으니 조금 올린다.
                0 => surface.bar(rect, |t| from_hsv(t, saturation.max(0.35), value.max(0.6))),
                1 => surface.bar(rect, |t| from_hsv(hue, t, value)),
                _ => surface.bar(rect, |t| from_hsv(hue, saturation, t)),
            }
            let knob = (rect.0 + 7.0 + at * (rect.2 - rect.0 - 14.0), top + 7.0);
            surface.disc(knob, 10.0, ROSE, 1.0);
            surface.disc(knob, 8.5, WHITE, 1.0);
            surface.disc(knob, 6.5, model.color, 1.0);
            self.spots.push(((rect.0, top - 6.0, rect.2, top + 20.0), Action::Slide(which)));
        }
    }

    fn paint_alarm(&mut self, surface: &mut Surface, model: &Model, (x, y): (f32, f32)) {
        let (left, right) = (x + 14.0, x + 250.0);
        // 종류: 없음, 한 번, 날마다.
        let third = (right - left) / 3.0;
        surface.frame((left, y + 44.0, right, y + 72.0), 14.0, PINK, CREAM, 1.5);
        for (index, name) in ["없음", "한 번", "날마다"].into_iter().enumerate() {
            let rect = (left + third * index as f32, y + 44.0, left + third * (index + 1) as f32, y + 72.0);
            let action = Action::Mode(index as u8);
            let chosen = model.mode == index as u8;
            if chosen {
                surface.pill(rect, 14.0, ROSE, 1.0);
            } else if self.hot == Some(action) {
                surface.pill((rect.0 + 2.0, rect.1 + 2.0, rect.2 - 2.0, rect.3 - 2.0), 12.0, HOT, 1.0);
            }
            let center = ((rect.0 + rect.2) / 2.0, (rect.1 + rect.3) / 2.0);
            self.write(surface, name, 12.5, chosen, center, if chosen { WHITE } else { DEEP }, 1.0);
            self.spots.push((rect, action));
        }
        // 빨리 고르기.
        for (index, (name, from, to)) in
            [("10분 뒤", 0.0, 72.0), ("1시간 뒤", 78.0, 154.0), ("내일 아침", 160.0, 236.0)].into_iter().enumerate()
        {
            self.button(
                surface,
                (left + from, y + 82.0, left + to, y + 108.0),
                name,
                12.5,
                Action::Quick(index as u8),
                false,
            );
        }

        // 시각. 알림이 없으면 흐리게 보이지만, 누르면 알림이 켜진다.
        let faint = if model.mode == 0 { 0.45 } else { 1.0 };
        let pm = model.hour >= 12;
        let halves = (left, y + 124.0, left + 68.0, y + 156.0);
        surface.frame(halves, 16.0, PINK, CREAM, 1.5);
        for (index, name) in ["오전", "오후"].into_iter().enumerate() {
            let rect = (halves.0 + 34.0 * index as f32, halves.1, halves.0 + 34.0 * (index + 1) as f32, halves.3);
            let chosen = (index == 1) == pm;
            if chosen {
                surface.pill(rect, 16.0, ROSE, faint);
            }
            let center = ((rect.0 + rect.2) / 2.0, (rect.1 + rect.3) / 2.0);
            self.write(surface, name, 12.0, chosen, center, if chosen { WHITE } else { DEEP }, faint.max(0.6));
            self.spots.push((rect, Action::Half(index == 1)));
        }
        let shown = if model.hour.is_multiple_of(12) { 12 } else { model.hour % 12 };
        let boxes = [
            (left + 80.0, format!("{shown}"), Action::Hour(1), Action::Hour(-1)),
            (left + 162.0, format!("{:02}", model.minute), Action::Minute(1), Action::Minute(-1)),
        ];
        for (from, number, up, down) in boxes {
            let rect = (from, y + 118.0, from + 68.0, y + 162.0);
            surface.frame(rect, 10.0, PINK, WHITE, 1.5);
            self.write(surface, &number, 22.0, true, (from + 24.0, y + 140.0), INK, faint);
            for (action, arrow, top) in [(up, UP, rect.1 + 2.0), (down, DOWN, rect.1 + 22.0)] {
                let spot = (from + 46.0, top, from + 66.0, top + 20.0);
                if self.hot == Some(action) {
                    surface.pill(spot, 6.0, HOT, 1.0);
                }
                self.write(surface, arrow, 12.0, false, ((spot.0 + spot.2) / 2.0, (spot.1 + spot.3) / 2.0), ROSE, 1.0);
                self.spots.push((spot, action));
            }
        }
        self.write(surface, ":", 22.0, true, (left + 155.0, y + 138.0), INK, faint);

        // 날짜. 한 번 알릴 때만 고른다.
        let row = (y + 174.0, y + 220.0);
        match model.mode {
            1 => {
                for (action, arrow, from) in
                    [(Action::Week(-1), LEFT, left - 4.0), (Action::Week(1), RIGHT, right - 14.0)]
                {
                    let spot = (from, row.0, from + 18.0, row.1);
                    let can = action != Action::Week(-1) || model.week > model.today;
                    if can && self.hot == Some(action) {
                        surface.pill(spot, 6.0, HOT, 1.0);
                    }
                    self.write(
                        surface,
                        arrow,
                        12.0,
                        false,
                        (from + 9.0, (row.0 + row.1) / 2.0),
                        ROSE,
                        if can { 1.0 } else { 0.3 },
                    );
                    if can {
                        self.spots.push((spot, action));
                    }
                }
                for index in 0..7 {
                    let day = model.week + index;
                    let rect =
                        (left + 16.0 + 29.0 * index as f32, row.0, left + 16.0 + 29.0 * index as f32 + 27.0, row.1);
                    let action = Action::Day(day);
                    let chosen = day == model.day;
                    if chosen {
                        surface.pill(rect, 9.0, ROSE, 1.0);
                    } else if self.hot == Some(action) {
                        surface.pill(rect, 9.0, HOT, 1.0);
                    } else if day == model.today {
                        surface.frame(rect, 9.0, PINK, CREAM, 1.5);
                    }
                    let center = (rect.0 + rect.2) / 2.0;
                    let name = if day == model.today { "오늘" } else { weekday(day) };
                    let (_, _, date) = civil_from_days(day);
                    let color = if chosen { WHITE } else { DEEP };
                    self.write(surface, name, 11.5, false, (center, row.0 + 13.0), color, 1.0);
                    self.write(
                        surface,
                        &date.to_string(),
                        14.0,
                        true,
                        (center, row.0 + 31.0),
                        if chosen { WHITE } else { INK },
                        1.0,
                    );
                    self.spots.push((rect, action));
                }
            }
            2 => self.write(
                surface,
                "날마다 이 시각에 알려요",
                12.5,
                false,
                ((left + right) / 2.0, (row.0 + row.1) / 2.0),
                INK,
                0.85,
            ),
            _ => self.write(
                surface,
                "알림이 없어요",
                12.5,
                false,
                ((left + right) / 2.0, (row.0 + row.1) / 2.0),
                INK,
                0.65,
            ),
        }

        // 무엇을 고른 것인지 한 줄로.
        let time = clock(model.hour, model.minute);
        let line = match model.mode {
            1 if model.past => "지난 시각이에요. 알리지 않아요".to_string(),
            1 => {
                let (_, month, date) = civil_from_days(model.day);
                format!("{month}월 {date}일({}) {time}에 알려요", weekday(model.day))
            }
            2 => format!("날마다 {time}에 알려요"),
            _ => String::new(),
        };
        if !line.is_empty() {
            self.write(surface, &line, 12.5, true, ((left + right) / 2.0, y + 236.0), ROSE, 1.0);
        }
    }
}

/// 판이 고쳐 가는 알림. 포스트잇의 알림을 읽어 채우고, 고친 것을 다시 알림으로 돌려준다.
#[derive(Clone, Copy)]
pub struct Draft {
    pub mode: u8,
    pub day: i64,
    pub hour: u32,
    pub minute: u32,
    pub week: i64,
}

impl Draft {
    /// 포스트잇의 알림으로 시작한다. 알림이 없으면 오늘, 다음 정각을 보여 둔다.
    pub fn from(alarm: Alarm, now: (i64, i64)) -> Draft {
        let next_hour = ((now.1 / 60 + 1) % 24) as u32;
        let soon_day = if now.1 / 60 + 1 >= 24 { now.0 + 1 } else { now.0 };
        let (mode, day, hour, minute) = match alarm {
            Alarm::None => (0, soon_day, next_hour, 0),
            Alarm::Once { day, hour, minute } => (1, day.max(now.0), hour, minute),
            Alarm::Daily { hour, minute } => (2, soon_day, hour, minute),
        };
        Draft { mode, day, hour, minute, week: now.0.max(day - (day - now.0).rem_euclid(7)) }
    }

    pub fn alarm(self) -> Alarm {
        match self.mode {
            1 => Alarm::Once { day: self.day, hour: self.hour, minute: self.minute },
            2 => Alarm::Daily { hour: self.hour, minute: self.minute },
            _ => Alarm::None,
        }
    }

    /// 판에서 누른 것을 반영한다.
    pub fn apply(&mut self, action: Action, now: (i64, i64)) {
        // 시각이나 날짜를 건드리면 알림이 켜진다.
        let wake = |draft: &mut Draft| {
            if draft.mode == 0 {
                draft.mode = 1;
            }
        };
        match action {
            Action::Mode(mode) => self.mode = mode,
            Action::Quick(which) => {
                let at = match which {
                    0 => now.0 * 1_440 + now.1 + 10,
                    1 => now.0 * 1_440 + now.1 + 60,
                    _ => (now.0 + 1) * 1_440 + 9 * 60,
                };
                (self.mode, self.day) = (1, at.div_euclid(1_440));
                (self.hour, self.minute) = ((at.rem_euclid(1_440) / 60) as u32, (at.rem_euclid(60)) as u32);
                self.week = now.0.max(self.day - (self.day - now.0).rem_euclid(7));
            }
            Action::Half(pm) => {
                self.hour = self.hour % 12 + if pm { 12 } else { 0 };
                wake(self);
            }
            Action::Hour(step) => {
                // 오전과 오후는 그대로 두고 시만 돈다.
                self.hour = self.hour / 12 * 12 + (self.hour as i32 % 12 + step).rem_euclid(12) as u32;
                wake(self);
            }
            Action::Minute(step) => {
                // 5분씩 움직인다.
                self.minute = ((self.minute as i32 / 5 + step) * 5).rem_euclid(60) as u32;
                wake(self);
            }
            Action::Day(day) => (self.mode, self.day) = (1, day),
            Action::Week(step) => self.week = (self.week + 7 * step as i64).max(now.0),
            Action::Close | Action::Swatch(_) | Action::Slide(_) => {}
            Action::FocusPick(..) | Action::FocusStep(..) | Action::FocusAuto | Action::FocusGo(_) => {}
        }
    }
}
