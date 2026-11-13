//! 메모를 포스트잇으로 화면에 붙인다.
//!
//! 메모 하나가 포스트잇 한 장이고, 포스트잇 한 장이 창 하나다. 그림 파일 없이 코드로 직접 그리고, 가만히 붙어
//! 있는 동안에는 아무 일도 하지 않는다. 보통은 다른 창들 뒤(바탕화면 바로 위)에 붙어 있고, "항상 위"를 켠
//! 포스트잇만 다른 창 위에 뜬다.
//!
//! 다루는 법
//! - 마우스를 올리면 단추 넷이 나온다: 색, 알림, 항상 위, 지우기. 꽃 핀을 눌러도 "항상 위"가 바뀐다.
//! - 색 단추를 누르면 색을 고르는 판이, 알림 단추를 누르면 알림을 정하는 판이 옆에 뜬다.
//! - 끌어서 옮긴다. 오른쪽 아래의 접힌 귀퉁이를 끌면 크기가 바뀐다.
//! - 두 번 클릭하면 반듯하게 펴지고 글을 적을 수 있다. 다른 곳을 누르거나 Esc를 누르면 끝난다.
//! - 알림이 울리면 커지고 빛나며 앞으로 나온다. 아래에 답하는 단추 둘이 나온다.
//! - 메모를 한데 모아 보는 창(메모 목록)에서 찾고, 찾아가고, 지울 수 있다.
//!
//! 메모의 내용은 [`Memos`]가 들고 있다. 여기서는 화면에 붙은 모습과 움직임을 다루고, 바뀐 것은 그때그때
//! 메모에 옮겨 적는다. 저장하거나 메뉴를 띄우는 것처럼 앱이 해야 할 일은 [`Outcome`]으로 알린다.

mod alarm;
mod draw;
mod fan;
mod focus_panel;
mod list;
mod panel;
mod text;
mod window;

use std::collections::hash_map::DefaultHasher;
use std::f32::consts::PI;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL};
use windows_sys::Win32::UI::WindowsAndMessaging::{MSG, WM_KEYDOWN};

use crate::dev::MemoStep;
use crate::geom::Rect;
use crate::memos::{Memo, Memos, Sheet, TEXT_LIMIT};
use crate::when::When;
use crate::win;
use alarm::Alarm;
use draw::{COLORS, Hit, Ink, Look, Rgb, TOOLS};
use fan::Fan;
use focus_panel::FocusPanel;
use list::{FOUND_SIZE, FOUND_TIME, List};
use panel::{Action, Draft, Kind, Model, Panel, Surface};
use text::{DT_EDITCONTROL, DT_WORDBREAK, Font, Painter};
use window::{Canvas, Cursor, Editor, Event};

pub use fan::Pick as FanPick;
pub use focus_panel::FocusAct;
pub use panel::FocusModel;
pub use window::{NOTE_CLASS, menu};

/// 움직이는 동안 시간을 흘리는 간격. 창을 옮기는 일은 이 간격으로 한다.
const TICK: Duration = Duration::from_millis(16);
/// 그림을 다시 그리는 가장 짧은 간격(초). 그리기는 창 옮기기보다 비싸서 1초에 30번쯤으로 줄인다.
const REDRAW_EVERY: f32 = 0.030;
/// 알림이 울릴 때 흔들리는 시간(초). 그 뒤에는 커진 채로 가만히 빛난다. 계속 흔들면 CPU를 계속 쓴다.
const RING_SHAKE: f32 = 6.0;
/// 알림이 울릴 때 커지는 정도.
const RING_SIZE: f32 = 1.28;
/// 마우스가 포스트잇을 벗어난 뒤 단추를 거두기까지 기다리는 초.
const LINGER: f32 = 0.35;
const PLACEHOLDER: &str = "여기에 적어요";
/// 아이콘 글꼴(Segoe MDL2 Assets)의 글자: 색, 알림, 고정 핀, 지우기.
const ICONS: [&str; TOOLS] = ["\u{E790}", "\u{EA8F}", "\u{E718}", "\u{E74D}"];
/// 새 포스트잇에 차례로 돌려 쓰는 색의 수. 기본 색 가운데 앞의 여섯이다.
const FIRST_COLORS: usize = 6;

/// 앱이 해야 할 일.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// 메모가 바뀌었다. 저장해야 한다.
    Changed,
    /// 포스트잇을 오른쪽 클릭했다. 메뉴를 띄울 화면 좌표다.
    Menu { id: u64, x: i32, y: i32 },
    /// 글이 적힌 메모의 지우기 단추를 눌렀다. 지울지 물어야 한다.
    AskDelete(u64),
    /// 울리는 알림에 답했다. `snooze`면 조금 뒤에 다시 알려 달라는 것이다.
    Answered { id: u64, snooze: bool },
    /// 메모 목록에서 "새 메모"를 눌렀다. 새 포스트잇을 붙일 화면 좌표다.
    Create { x: i32, y: i32 },
    /// 꽃핀 머리 위의 단추 가운데 하나를 눌렀다. 단추들은 이미 닫혔다.
    Fan(FanPick),
    /// 꽃핀 머리 위의 단추들이 아무것도 누르지 않은 채 닫혔다.
    FanClosed,
    /// 쉬는 시간 알림 판에서 무엇인가를 눌렀다.
    Focus(FocusAct),
}

/// 목표로 다가가는 값. 용수철처럼 조금 지나쳤다가 돌아온다.
#[derive(Clone, Copy)]
struct Spring {
    x: f32,
    v: f32,
    to: f32,
}

impl Spring {
    fn at(x: f32) -> Spring {
        Spring { x, v: 0.0, to: x }
    }

    /// 한 걸음 움직인다. 아직 움직이는 중이면 `true`다.
    fn step(&mut self, dt: f32, stiff: f32, damp: f32) -> bool {
        if (self.x - self.to).abs() < 0.0005 && self.v.abs() < 0.005 {
            (self.x, self.v) = (self.to, 0.0);
            return false;
        }
        self.v += (-stiff * (self.x - self.to) - damp * self.v) * dt;
        self.x += self.v * dt;
        true
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Idle,
    /// 막 만들어졌다. 톡 튀어나와 붙는다(0~1).
    Born(f32),
    /// 알림이 울린다. 울리기 시작한 뒤 흐른 초.
    Ringing(f32),
    /// 지워지는 중: 구겨진다(0~1).
    Crumpling(f32),
    /// 지워진 뒤: 꽃잎이 흩어진다(0~1).
    Petals(f32),
    /// 글을 적는 중.
    Editing,
}

/// 하던 움직임이 끝나고 가만히 붙으면 이어서 할 일.
#[derive(Clone, Copy, PartialEq)]
enum Then {
    Edit,
    Ring,
}

/// 글자 그림을 그리고 돌려주는 것: 그림의 크기, 글의 아래 끝, 알림 시각 칸의 너비, 글이 비어 있는지, 글이 그려진 범위.
type Written = ((i32, i32), f32, Option<f32>, bool, (f32, f32, f32, f32));

/// 글자 그림에서 `rect`(포스트잇의 자리) 안에 실제로 글이 그려진 범위를 잰다. 없으면 빈 범위다.
/// 그릴 때 빈 곳까지 읽지 않으려고 쓴다.
fn inked(
    pixels: &[u32],
    stride: usize,
    size: (i32, i32),
    origin: (f32, f32),
    density: f32,
    rect: (f32, f32, f32, f32),
) -> (f32, f32, f32, f32) {
    let px = |value: f32, from: f32, most: i32| (((value - from) * density) as i32).clamp(0, most);
    let (left, right) = (px(rect.0, origin.0, size.0), px(rect.2, origin.0, size.0));
    let (top, low) = (px(rect.1, origin.1, size.1), px(rect.3, origin.1, size.1));
    // 흰 바탕에 검은 글자다. 초록 값이 가득 차 있지 않으면 글자가 닿은 곳이다.
    let drawn = |pixel: &u32| pixel & 0xFF00 != 0xFF00;
    let mut found: Option<(i32, i32, i32, i32)> = None;
    for y in top..low {
        let row = &pixels[y as usize * stride + left as usize..y as usize * stride + right as usize];
        let Some(first) = row.iter().position(drawn) else { continue };
        let last = row.iter().rposition(drawn).unwrap_or(first);
        let (first, last) = (left + first as i32, left + last as i32);
        found = Some(match found {
            None => (first, y, last, y),
            Some((a, b, c, _)) => (a.min(first), b, c.max(last), y),
        });
    }
    let at = |pixel: i32, from: f32| from + pixel as f32 / density;
    found.map_or((0.0, 0.0, 0.0, 0.0), |(a, b, c, d)| {
        (at(a, origin.0) - 2.0, at(b, origin.1) - 2.0, at(c + 1, origin.0) + 2.0, at(d + 1, origin.1) + 2.0)
    })
}

/// 다른 자리로 미끄러져 가는 중: 떠난 곳, 갈 곳, 간 정도(0~1).
type Glide = ((f32, f32), (f32, f32), f32);

struct Note {
    /// 메모의 번호.
    id: u64,
    hwnd: HWND,
    text: String,
    color: Rgb,
    /// 종이의 크기.
    paper: (f32, f32),
    alarm: Alarm,
    /// 다음에 알림이 울릴 시각. 메모에 적힌 것을 베껴 둔 것이다.
    next: Option<When>,
    top: bool,
    /// 붙어 있을 때의 기울기(라디안).
    rest: f32,
    /// 기울기가 없을 때 종이 왼쪽 위가 놓이는 화면 좌표(px).
    at: (f32, f32),
    /// 화면 배율(100%면 1). 종이의 한가운데가 놓인 모니터의 것을 따른다.
    scale: f32,
    glide: Option<Glide>,
    phase: Phase,
    then: Option<Then>,
    tilt: Spring,
    zoom: Spring,
    /// 세로로 늘어난 정도. 붙는 순간에만 1이 아니다.
    stretch: f32,
    opacity: f32,
    tools: Spring,
    lift: Spring,
    glow: Spring,
    answers: Spring,
    hover: bool,
    hot: Option<Hit>,
    /// 마우스가 벗어난 뒤 흐른 초. 벗어나자마자 단추를 거두지 않고 잠깐 기다린다.
    away: Option<f32>,
    /// 끄는 중: 커서에서 `at`까지의 거리.
    drag: Option<(f32, f32)>,
    /// 크기를 바꾸는 중: 처음 크기와, 잡았을 때의 커서 자리.
    resize: Option<((f32, f32), (i32, i32))>,
    /// 메모 목록에서 찾아가서 반짝이는 중: 반짝이기 시작한 뒤 흐른 초.
    found: Option<f32>,
    seed: u32,
    /// 그림을 다시 그려야 한다.
    stale: bool,
    /// 마지막으로 그린 뒤 흐른 시간(초).
    since_drawn: f32,
}

impl Note {
    /// 지워지는 중인가. 구겨지거나 꽃잎으로 흩어지고 있다.
    fn is_leaving(&self) -> bool {
        matches!(self.phase, Phase::Crumpling(_) | Phase::Petals(_))
    }

    /// 포스트잇 왼쪽 아래에 적는 알림 시각. 알릴 일이 남아 있을 때만 적는다.
    fn chip(&self, today: i64) -> Option<String> {
        match (self.alarm, self.next) {
            (Alarm::None, _) | (Alarm::Once { .. }, None) => None,
            // 미뤄 둔 알림이면 미룬 시각을 보여 준다.
            (Alarm::Once { .. }, Some(next)) => {
                let of_day = next.minute_of_day() as u32;
                Alarm::Once { day: next.day(), hour: of_day / 60, minute: of_day % 60 }.chip(today)
            }
            (Alarm::Daily { .. }, _) => self.alarm.chip(today),
        }
    }

    /// 포스트잇 창의 크기(px). 종이의 크기에 따라 달라진다.
    fn canvas_px(&self) -> (i32, i32) {
        let size = draw::canvas(self.paper);
        ((size.0 * self.scale).ceil() as i32, (size.1 * self.scale).ceil() as i32)
    }

    /// 포스트잇의 창이 놓일 화면 좌표.
    fn window_at(&self) -> (i32, i32) {
        let offset = draw::paper_offset(self.paper);
        ((self.at.0 - offset.0 * self.scale).round() as i32, (self.at.1 - offset.1 * self.scale).round() as i32)
    }

    /// 종이의 한가운데쯤. 이 점이 놓인 모니터의 화면 배율을 따른다.
    fn middle(&self) -> (f32, f32) {
        (self.at.0 + self.paper.0 / 2.0 * self.scale, self.at.1 + self.paper.1 / 2.0 * self.scale)
    }

    /// 지금의 모습을 메모에 적을 모양으로 바꾼다.
    fn sheet(&self) -> Sheet {
        let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
        Sheet {
            at: Some((self.at.0.round() as i32, self.at.1.round() as i32)),
            size: (self.paper.0.round() as u32, self.paper.1.round() as u32),
            color: byte(self.color.0) << 16 | byte(self.color.1) << 8 | byte(self.color.2),
            tilt: (self.rest.to_degrees() * 10.0).round() as i32,
            top: self.top,
        }
    }
}

/// 그리는 데 쓰는 것들. 그릴 일이 생기면 만들고, 움직이는 것이 없어지면 돌려준다.
/// 가만히 붙어 있는 포스트잇은 Windows가 그림을 들고 있으므로, 그동안에는 이쪽에 버퍼를 둘 까닭이 없다.
struct Tools {
    /// 모든 포스트잇이 차례로 돌려 쓰는 그림 버퍼.
    canvas: Canvas,
    painter: Painter,
    /// 메모의 글 한 줄의 높이.
    line: f32,
}

pub struct Postits {
    /// 주 모니터의 화면 배율(100%면 1). 모니터를 알 수 없을 때 쓴다.
    scale: f32,
    /// 모니터마다의 범위와 화면 배율. 포스트잇은 자기가 놓인 모니터의 배율로 그린다.
    monitors: Vec<(Rect, f32)>,
    /// 떠 있는 판의 화면 배율. 판을 띄운 포스트잇의 배율을 따른다.
    panel_scale: f32,
    /// 확인용으로 도는 중이다. 새 포스트잇을 쓰던 창들 앞으로 가져오지 않는다.
    quiet: bool,
    tools: Option<Tools>,
    /// 글자 그림에 마지막으로 그려 둔 것의 지문과, 그때 돌려준 값. 같은 것을 또 그리라고 하면 다시 그리지 않는다.
    /// 알림이 울려 흔들리는 동안에는 글자 그림이 바뀌지 않으므로, 프레임마다 다시 만들지 않아도 된다.
    written: Option<(u64, Written)>,
    notes: Vec<Note>,
    editor: Option<(HWND, Editor)>,
    /// 떠 있는 판(색 고르기나 알림 정하기)과 그 자리, 판이 고쳐 가는 값.
    panel: Option<Panel>,
    panel_at: (i32, i32),
    draft: Draft,
    hsv: (f32, f32, f32),
    /// 메모 목록 창. 열려 있을 때만 있다.
    list: Option<List>,
    /// 메모 목록에서 찾는 말을 적는 칸. 적는 동안에만 있다.
    search: Option<Editor>,
    /// 메모 목록을 닫을 때 있던 자리. 다시 열면 그 자리에 띄운다.
    list_spot: Option<(i32, i32)>,
    /// 꽃핀 머리 위에 부채꼴로 뜬 단추들. 떠 있을 때만 있다.
    fan: Option<Fan>,
    /// 꽃핀 옆에 뜬 쉬는 시간 알림 판. 떠 있을 때만 있다.
    focus: Option<FocusPanel>,
    /// 쉬는 시간 알림 판을 마지막으로 옮겨 둔 자리. 다시 열면 그 자리에 띄운다(앱이 켜져 있는 동안).
    focus_spot: Option<(i32, i32)>,
    /// 움직이는 것이 있다. 있는 동안에는 짧은 간격으로 시간을 흘린다.
    awake: bool,
    last: Instant,
    /// 지금 이 PC의 벽시계 시각. 앱이 한 바퀴 돌 때마다 받아 둔다.
    wall: When,
    seed: u32,
    outcomes: Vec<Outcome>,
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// 끝에서 조금 지나쳤다가 돌아오는 움직임(0~1).
fn overshoot(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0) - 1.0;
    1.0 + 2.2 * t * t * t + 1.2 * t * t
}

impl Postits {
    /// `dpi`는 주 모니터의 화면 배율이다. 모니터들을 알려 주기 전까지 이것을 쓴다.
    /// `quiet`이면 글을 적는 칸을 띄워도 쓰던 창에서 키보드 입력을 가져오지 않는다(확인용으로 돌릴 때).
    pub fn new(dpi: u32, quiet: bool, seed: u64, wall: When) -> Postits {
        window::QUIET.store(quiet, Ordering::Relaxed);
        Postits {
            scale: dpi.max(96) as f32 / 96.0,
            monitors: Vec::new(),
            panel_scale: 1.0,
            quiet,
            tools: None,
            written: None,
            notes: Vec::new(),
            editor: None,
            panel: None,
            panel_at: (0, 0),
            draft: Draft::from(Alarm::None, (wall.day(), wall.minute_of_day())),
            hsv: (0.0, 0.0, 1.0),
            list: None,
            search: None,
            list_spot: None,
            fan: None,
            focus: None,
            focus_spot: None,
            awake: false,
            last: Instant::now(),
            wall,
            seed: seed as u32 ^ (seed >> 32) as u32,
            outcomes: Vec::new(),
        }
    }

    /// 모니터마다의 범위와 DPI를 알려 준다. 시작할 때와 모니터 구성이 바뀔 때 부른다.
    /// 화면 배율이 달라진 모니터에 놓인 포스트잇은 그 배율로 다시 그린다.
    pub fn set_monitors(&mut self, monitors: &[(Rect, u32)]) {
        self.monitors = monitors.iter().map(|&(area, dpi)| (area, dpi.max(96) as f32 / 96.0)).collect();
        for index in 0..self.notes.len() {
            self.settle(index);
        }
        self.settle_list();
    }

    /// 화면의 한 점이 놓인 모니터의 화면 배율. 어느 모니터에도 들지 않으면 가장 가까운 모니터의 것이다.
    fn scale_at(&self, (x, y): (f32, f32)) -> f32 {
        let away = |area: &Rect| {
            let dx = (area.x as f32 - x).max(x - area.right() as f32).max(0.0);
            let dy = (area.y as f32 - y).max(y - area.bottom() as f32).max(0.0);
            dx * dx + dy * dy
        };
        let nearest = self.monitors.iter().min_by(|a, b| away(&a.0).total_cmp(&away(&b.0)));
        nearest.map_or(self.scale, |&(_, scale)| scale)
    }

    /// 포스트잇이 지금 놓인 모니터의 화면 배율을 따르게 한다. 배율이 다른 모니터로 옮겨졌으면 다시 그린다.
    fn settle(&mut self, index: usize) {
        let scale = self.scale_at(self.notes[index].middle());
        if scale != self.notes[index].scale {
            (self.notes[index].scale, self.notes[index].stale) = (scale, true);
            self.wake();
        }
    }

    /// 메모들을 화면에 붙인다. 시작할 때 한 번 부른다. 아직 붙인 적이 없는 메모는 자리를 잡아 준다.
    pub fn attach(&mut self, memos: &mut Memos) {
        // 오래된 것부터 붙인다. 새로 만든 것이 뒤에 오게 하려는 것이다.
        let ids: Vec<u64> = memos.list().iter().rev().map(|memo| memo.id).collect();
        for id in ids {
            let Some(memo) = memos.get_mut(id) else { continue };
            if memo.sheet.at.is_none() {
                memo.sheet = self.fresh_sheet(self.grid_spot(self.notes.len()));
                self.outcomes.push(Outcome::Changed);
            }
            let memo = memo.clone();
            self.add(&memo, false);
        }
        // 다 붙였다. 움직일 것이 없으므로 그리는 데 쓴 버퍼를 돌려준다.
        self.rest();
    }

    /// 움직이는 것이 없으면 그리는 데 쓰던 버퍼를 돌려준다.
    fn rest(&mut self) {
        if !self.awake {
            (self.tools, self.written) = (None, None);
        }
    }

    fn px(&self, units: f32) -> i32 {
        (units * self.scale).round() as i32
    }

    fn random(&mut self) -> f32 {
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.seed >> 8) as f32 / (1u32 << 24) as f32
    }

    /// 새 포스트잇의 모습. 3~7도 사이에서 아무 쪽으로나 기울이고, 색은 기본 색을 차례로 돌려 쓴다.
    fn fresh_sheet(&mut self, at: (i32, i32)) -> Sheet {
        let tilt = (3.0 + self.random() * 4.0) * if self.random() < 0.5 { -1.0 } else { 1.0 };
        let color = COLORS[self.notes.len() % FIRST_COLORS];
        let byte = |value: f32| (value * 255.0 + 0.5) as u32;
        Sheet {
            at: Some(at),
            color: byte(color.0) << 16 | byte(color.1) << 8 | byte(color.2),
            tilt: (tilt * 10.0).round() as i32,
            ..Sheet::default()
        }
    }

    /// 줄 세울 때 `index`번째 포스트잇이 놓일 자리. 마우스 커서가 있는 모니터의 왼쪽 위부터 차례로 놓는다.
    /// `cell`은 가장 큰 포스트잇의 크기다.
    fn grid(&self, index: usize, cell: (f32, f32)) -> (f32, f32) {
        let cursor = win::cursor_position();
        let work = window::work_area(cursor, true).unwrap_or((0, 0, 1920, 1040));
        let scale = self.scale_at((cursor.0 as f32, cursor.1 as f32));
        let (wide, tall) = ((cell.0 + 26.0) * scale, (cell.1 + 54.0) * scale);
        let columns = ((((work.2 - work.0) as f32 * 0.42) / wide).floor() as usize).max(1);
        (
            work.0 as f32 + 36.0 * scale + (index % columns) as f32 * wide,
            work.1 as f32 + 56.0 * scale + (index / columns) as f32 * tall,
        )
    }

    fn grid_spot(&self, index: usize) -> (i32, i32) {
        let spot = self.grid(index, draw::PAPER);
        (spot.0.round() as i32, spot.1.round() as i32)
    }

    /// 그리는 데 쓰는 것들을 마련한다. 글꼴은 처음 한 번만 올린다.
    fn ready(&mut self) {
        if self.tools.is_some() {
            return;
        }
        text::load_fonts();
        let start = draw::canvas(draw::PAPER);
        let (width, height) = ((start.0 * self.scale).ceil() as i32, (start.1 * self.scale).ceil() as i32);
        let painter = Painter::new(width, height);
        self.tools = Some(Tools { line: painter.line_height(), painter, canvas: Canvas::new(width, height) });
    }

    /// 메모 하나를 포스트잇으로 붙인다. `born`이면 톡 튀어나오며 붙고, 다른 창들 앞에 나타난다.
    fn add(&mut self, memo: &Memo, born: bool) {
        let Some(at) = memo.sheet.at else { return };
        window::start();
        let sheet = memo.sheet;
        let rest = (sheet.tilt as f32 / 10.0).to_radians();
        let mut note = Note {
            id: memo.id,
            hwnd: std::ptr::null_mut(),
            text: memo.text.clone(),
            color: draw::rgb(sheet.color),
            paper: (sheet.size.0 as f32, sheet.size.1 as f32),
            alarm: Alarm::from(memo.remind),
            next: memo.next,
            top: sheet.top,
            rest,
            at: (at.0 as f32, at.1 as f32),
            scale: self.scale,
            glide: None,
            phase: if born { Phase::Born(0.0) } else { Phase::Idle },
            then: None,
            tilt: Spring::at(rest),
            zoom: Spring::at(1.0),
            stretch: 1.0,
            opacity: if born { 0.0 } else { 1.0 },
            tools: Spring::at(0.0),
            lift: Spring::at(0.0),
            glow: Spring::at(0.0),
            answers: Spring::at(0.0),
            hover: false,
            hot: None,
            away: None,
            drag: None,
            resize: None,
            found: None,
            seed: self.seed.wrapping_add(memo.id as u32 * 97),
            stale: true,
            since_drawn: 0.0,
        };
        // 종이의 한가운데가 놓인 모니터의 배율을 따른다. 한가운데가 배율에 따라 달라지므로 한 번 더 맞춘다.
        note.scale = self.scale_at(note.middle());
        note.scale = self.scale_at(note.middle());
        note.hwnd = window::create(note.window_at(), note.canvas_px(), false);
        self.notes.push(note);
        let last = self.notes.len() - 1;
        // 처음 한 장을 넘긴 뒤에 보여 준다.
        self.render(last);
        let note = &self.notes[last];
        window::show(note.hwnd);
        window::set_layer(note.hwnd, note.top);
        if !note.top {
            // 새로 붙인 포스트잇은 쓰던 창들 앞에 붙는다. 앱을 켤 때 다시 붙이는 포스트잇은 쓰던 창들을 가리지
            // 않게 맨 뒤(바탕화면 바로 위)에 붙는다. 확인용으로 도는 중에도 쓰던 창들을 가리지 않는다.
            if born && !self.quiet {
                window::raise(note.hwnd);
            } else {
                window::lower(note.hwnd);
            }
        }
        if born {
            self.wake();
        }
    }

    fn look(&self, note: &Note) -> Look {
        let hot = if note.drag.is_some() || note.resize.is_some() { None } else { note.hot };
        let paneled =
            |kind: Kind| self.panel.as_ref().is_some_and(|panel| panel.kind == kind && panel.note == note.hwnd);
        let chip = note.chip(self.wall.day()).is_some();
        Look {
            paper: note.paper,
            color: note.color,
            tilt: note.tilt.x,
            size: (note.zoom.x, note.zoom.x * note.stretch),
            opacity: note.opacity,
            tools: note.tools.x.clamp(0.0, 1.0),
            hot_tool: if let Some(Hit::Tool(index)) = hot { Some(index) } else { None },
            on: [paneled(Kind::Color), chip || paneled(Kind::Alarm), note.top, false],
            top: note.top,
            glow: note.glow.x.clamp(0.0, 1.0),
            answers: note.answers.x.clamp(0.0, 1.0),
            hot_answer: if let Some(Hit::Answer(index)) = hot { Some(index) } else { None },
            lift: note.lift.x.clamp(0.0, 1.0),
            editing: note.phase == Phase::Editing,
            crumple: if let Phase::Crumpling(t) = note.phase { t } else { 0.0 },
            petals: if let Phase::Petals(t) = note.phase { Some(t) } else { None },
            seed: note.seed,
        }
    }

    /// 포스트잇의 글자와 아이콘을 반듯한 모양으로 크게 그려 둔다. 기울이고 키우는 것은 그리는 쪽이 한다.
    fn write(&mut self, index: usize, look: &Look) -> Written {
        let Some(tools) = &mut self.tools else { return Written::default() };
        let note = &self.notes[index];
        let chip = note.chip(self.wall.day());
        // 글자 그림에 들어가는 것이 지난번과 모두 같으면 그려 둔 것을 그대로 쓴다.
        let mut hasher = DefaultHasher::new();
        (note.hwnd as usize, &note.text, &chip, look.editing).hash(&mut hasher);
        [look.paper.0, look.paper.1, look.tools, look.answers, note.scale].map(f32::to_bits).hash(&mut hasher);
        let print = hasher.finish();
        if let Some((last, written)) = self.written
            && last == print
        {
            return written;
        }
        let painter = &mut tools.painter;
        // 너비를 재면 걸어 둔 변환이 풀리므로 먼저 잰다.
        let chip_width = chip.as_ref().map(|label| draw::chip_width(painter.measure(Font::Label, label)));
        let (origin, extent) = draw::sheet(look.paper);
        let density = draw::SUPER * note.scale;
        let size = ((extent.0 * density).ceil() as i32, (extent.1 * density).ceil() as i32);
        painter.begin(size.0, size.1, density, origin);
        let dim = note.text.trim().is_empty();
        // 들어가는 줄 수만큼만 보여 준다. 줄이 반만 걸쳐 보이지 않게 한다.
        let rect = draw::text_rect(look.paper);
        let bottom = rect.1 + ((rect.3 - rect.1) / tools.line).floor().max(1.0) * tools.line;
        if !look.editing {
            let shown = if dim { PLACEHOLDER } else { &note.text };
            painter.text(Font::Body, shown, rect, DT_WORDBREAK | DT_EDITCONTROL);
        }
        let body = inked(painter.pixels(), painter.width as usize, size, origin, density, rect);
        if let (Some(label), Some(wide)) = (&chip, chip_width) {
            let (left, top, right, low) = draw::chip_rect(look.paper, wide);
            painter.centered(Font::Bell, ICONS[1], (left + 5.0, top, left + 21.0, low));
            painter.centered(Font::Label, label, (left + 22.0, top - 1.0, right - 7.0, low - 1.0));
        }
        if look.tools > 0.01 {
            for (tool, icon) in ICONS.iter().enumerate() {
                let (cx, cy) = draw::tool_center(tool, look.tools, look.paper);
                painter.centered(Font::Icon, icon, (cx - 10.0, cy - 10.0, cx + 10.0, cy + 10.0));
            }
        }
        if look.answers > 0.01 {
            for (answer, label) in ["확인", "5분 뒤 다시"].into_iter().enumerate() {
                let rect = draw::answer_rect(answer, look.paper);
                painter.centered(Font::Answer, label, (rect.0, rect.1 - 1.0, rect.2, rect.3 - 1.0));
            }
        }
        let written = (size, bottom, chip_width, dim, body);
        self.written = Some((print, written));
        written
    }

    /// 한 장을 공용 버퍼에 그린다. 그린 그림의 크기(px)를 돌려준다.
    fn paint(&mut self, index: usize) -> (i32, i32) {
        self.ready();
        let look = self.look(&self.notes[index]);
        let size = self.notes[index].canvas_px();
        let (sheet, text_bottom, chip, dim, body) = self.write(index, &look);
        let scale = self.notes[index].scale;
        let Some(tools) = &mut self.tools else { return size };
        tools.canvas.fit(size.0, size.1);
        let stride = tools.canvas.width as usize;
        // 글자 그림(읽기)과 창 버퍼(쓰기)는 서로 다른 버퍼다.
        let ink = Ink {
            pixels: tools.painter.pixels(),
            stride: tools.painter.width as usize,
            size: sheet,
            origin: draw::sheet(look.paper).0,
            density: draw::SUPER * scale,
            text_bottom,
            body,
            chip,
            dim,
        };
        draw::draw(tools.canvas.pixels(), stride, size.0, size.1, scale, &look, &ink);
        size
    }

    /// 한 장을 그려 화면에 넘긴다.
    fn render(&mut self, index: usize) {
        let size = self.paint(index);
        let note = &mut self.notes[index];
        (note.stale, note.since_drawn) = (false, 0.0);
        if let Some(tools) = &self.tools {
            tools.canvas.present(note.hwnd, note.window_at(), size);
        }
    }

    /// 움직일 것이 생겼다. 시간을 흘리기 시작한다.
    fn wake(&mut self) {
        if !self.awake {
            (self.awake, self.last) = (true, Instant::now());
        }
    }

    /// 다음에 시간을 흘려야 할 때. 움직이는 것이 없으면 `None`이다.
    pub fn due(&self) -> Option<Instant> {
        self.awake.then_some(self.last + TICK)
    }

    fn find(&self, hwnd: HWND) -> Option<usize> {
        self.notes.iter().position(|note| note.hwnd == hwnd)
    }

    /// 그 메모의 포스트잇. 지워지는 중인 것은 뺀다.
    fn find_id(&self, id: u64) -> Option<usize> {
        self.notes.iter().position(|note| note.id == id && !note.is_leaving())
    }

    /// 그 메모의 포스트잇 창. 물음 창을 그 위에 띄우는 데 쓴다.
    pub fn hwnd(&self, id: u64) -> Option<HWND> {
        self.find_id(id).map(|index| self.notes[index].hwnd)
    }

    /// 포스트잇의 모습과 글을 메모에 옮겨 적는다. 달라진 것이 있으면 저장하라고 알린다.
    fn keep(&mut self, index: usize, memos: &mut Memos) {
        let note = &self.notes[index];
        let Some(memo) = memos.get_mut(note.id) else { return };
        let sheet = note.sheet();
        if memo.sheet != sheet || memo.text != note.text {
            (memo.sheet, memo.text) = (sheet, note.text.clone());
            self.outcomes.push(Outcome::Changed);
        }
    }

    /// 글을 적는 칸에서 누른 키 가운데 먼저 처리할 것이면 처리하고 `true`를 돌려준다.
    /// 여러 줄 입력 칸은 Ctrl+A를 스스로 처리하지 않는다. 메모 목록의 찾는 말을 적는 칸은 Enter와 Esc로 적기를 마친다.
    pub fn handles(&mut self, message: &MSG) -> bool {
        if message.message != WM_KEYDOWN {
            return false;
        }
        let control = unsafe { GetKeyState(VK_CONTROL as i32) } < 0;
        let key = message.wParam as u32;
        if let Some((_, editor)) = self.editor.as_ref().filter(|(_, editor)| editor.edit() == message.hwnd) {
            let all = control && key == 'A' as u32;
            if all {
                editor.select_all();
            }
            return all;
        }
        self.search_key(message.hwnd, key, control)
    }

    /// 한 바퀴의 일을 한다: 창에서 온 일을 처리하고, 움직이는 포스트잇의 시간을 흘린다.
    /// 앱이 해야 할 일을 돌려준다.
    pub fn frame(&mut self, memos: &mut Memos, wall: When, now: Instant) -> Vec<Outcome> {
        if wall.day() != self.wall.day() {
            // 날이 바뀌면 "오늘", "내일"로 적은 알림 시각이 달라진다.
            self.notes.iter_mut().filter(|note| note.alarm != Alarm::None).for_each(|note| note.stale = true);
            self.wake();
        }
        self.wall = wall;
        for event in window::take_events() {
            self.handle(event, memos);
        }
        if self.awake && now >= self.last + TICK {
            self.tick(now, memos);
        }
        // 메모 목록이 열려 있으면, 보일 것이 달라졌을 때 다시 그린다.
        self.sync_list(memos, now);
        std::mem::take(&mut self.outcomes)
    }

    /// 시간을 흘린다. 움직이는 포스트잇만 다시 그리고, 움직이는 것이 없으면 멈춘다.
    fn tick(&mut self, now: Instant, memos: &mut Memos) {
        let dt = now.saturating_duration_since(self.last).as_secs_f32().min(0.05);
        self.last = now;

        let mut moving = false;
        let mut gone = Vec::new();
        let mut settled = Vec::new();
        for index in 0..self.notes.len() {
            let note = &mut self.notes[index];
            let mut busy = false;
            let mut redraw = note.stale;
            // 마우스가 벗어난 뒤 잠깐은 단추를 그대로 둔다. 그사이에 돌아오면 없던 일이 된다.
            if let Some(away) = note.away {
                let away = away + dt;
                (note.away, busy) = (Some(away), true);
                if away >= LINGER {
                    (note.hover, note.away) = (false, None);
                }
            }
            // 저마다의 목표: 어디로 기울고, 얼마나 크고, 단추가 나와 있는지.
            let (mut tilt_to, mut zoom_to) = (note.rest, if note.drag.is_some() { 1.04 } else { 1.0 });
            let (mut tools_to, mut glow_to, mut answers_to) = (if note.hover { 1.0 } else { 0.0 }, 0.0, 0.0);
            match note.phase {
                // 메모 목록에서 찾아간 포스트잇은 두 번 커졌다 돌아오며 빛난다.
                Phase::Idle => {
                    if let Some(t) = note.found {
                        let t = t + dt;
                        note.found = (t < FOUND_TIME).then_some(t);
                        let beat = if t < FOUND_TIME { (2.0 * PI * t / FOUND_TIME).sin().powi(2) } else { 0.0 };
                        (zoom_to, glow_to) = (1.0 + FOUND_SIZE * beat, beat);
                        (note.zoom, note.glow) = (Spring::at(zoom_to), Spring::at(glow_to));
                        (busy, redraw) = (true, true);
                    }
                }
                Phase::Born(t) => {
                    let t = (t + dt / 0.5).min(1.0);
                    // 작게 튀어나와, 조금 크게 지나쳤다가, 눌리며 붙는다.
                    let (turn, zoom, stretch) = if t < 0.55 {
                        let k = 1.0 - (1.0 - t / 0.55).powi(3);
                        (lerp(-16.0, 3.0, k), lerp(0.15, 1.14, k), 1.0)
                    } else if t < 0.78 {
                        let k = (t - 0.55) / 0.23;
                        (lerp(3.0, 0.0, k), lerp(1.14, 0.95, k), lerp(1.0, 1.08, k))
                    } else {
                        let k = (t - 0.78) / 0.22;
                        (0.0, lerp(0.95, 1.0, k), lerp(1.08, 1.0, k))
                    };
                    note.tilt = Spring::at(note.rest + turn.to_radians());
                    note.zoom = Spring::at(zoom);
                    (note.stretch, note.opacity) = (stretch, (t / 0.3).min(1.0));
                    note.phase = if t >= 1.0 { Phase::Idle } else { Phase::Born(t) };
                    (busy, redraw) = (true, true);
                }
                Phase::Ringing(t) => {
                    let t = t + dt;
                    note.phase = Phase::Ringing(t);
                    (zoom_to, glow_to, answers_to, tools_to) = (RING_SIZE, 1.0, 1.0, 0.0);
                    if t < RING_SHAKE {
                        // 좌우로 흔들린다. 끝날 때는 잦아든다.
                        let fade = ((RING_SHAKE - t) / 0.6).min(1.0);
                        tilt_to = note.rest + (2.5f32).to_radians() * (t * 2.0 * PI).sin() * fade;
                        note.tilt.x = tilt_to;
                        (busy, redraw) = (true, true);
                    }
                }
                Phase::Crumpling(t) => {
                    let t = (t + dt / 0.42).min(1.0);
                    let zoom =
                        if t < 0.3 { lerp(1.0, 1.06, t / 0.3) } else { lerp(1.06, 0.16, ((t - 0.3) / 0.7).powi(2)) };
                    let turn = lerp(-6.0, 70.0, t * t).to_radians() * if t < 0.3 { t / 0.3 } else { 1.0 };
                    note.tilt = Spring::at(note.rest + turn);
                    note.zoom = Spring::at(zoom);
                    note.phase = if t >= 1.0 { Phase::Petals(0.0) } else { Phase::Crumpling(t) };
                    note.tools = Spring::at(0.0);
                    (busy, redraw) = (true, true);
                }
                Phase::Petals(t) => {
                    let t = t + dt / 1.1;
                    note.phase = Phase::Petals(t);
                    if t >= 1.0 {
                        gone.push(index);
                    }
                    (busy, redraw) = (true, true);
                }
                Phase::Editing => (tilt_to, tools_to) = (0.0, 1.0),
            }
            if matches!(note.phase, Phase::Idle | Phase::Ringing(_) | Phase::Editing) {
                (note.tilt.to, note.zoom.to) = (tilt_to, zoom_to);
                (note.tools.to, note.glow.to, note.answers.to) = (tools_to, glow_to, answers_to);
                note.lift.to = if note.drag.is_some() { 1.0 } else { 0.0 };
                let mut changed = note.tilt.step(dt, 260.0, 18.0) | note.zoom.step(dt, 300.0, 21.0);
                for spring in [&mut note.tools, &mut note.lift, &mut note.glow, &mut note.answers] {
                    changed |= spring.step(dt, 420.0, 41.0);
                }
                busy |= changed;
                redraw |= changed;
            }
            // 다른 자리로 미끄러져 간다. 그림은 그대로 두고 창만 옮긴다.
            let mut moved = false;
            if let Some((from, to, t)) = note.glide {
                let t = (t + dt / 0.55).min(1.0);
                let k = overshoot(t);
                note.at = (lerp(from.0, to.0, k), lerp(from.1, to.1, k));
                note.glide = (t < 1.0).then_some((from, to, t));
                (busy, moved) = (true, true);
                if t >= 1.0 {
                    settled.push(index);
                }
            }
            // 크기를 바꾸는 동안에도 그림을 계속 고친다.
            busy |= note.resize.is_some();
            // 가만히 붙었으면 미뤄 둔 일을 한다.
            busy |= note.phase == Phase::Idle && note.then.is_some();
            moving |= busy;
            note.since_drawn += dt;
            // 움직이는 동안에는 그리는 횟수를 줄이고, 멈출 때는 마지막 모습을 꼭 그린다.
            if redraw && busy && note.since_drawn < REDRAW_EVERY {
                note.stale = true;
            } else if redraw {
                self.render(index);
            } else if moved {
                window::move_to(note.hwnd, note.window_at());
            }
        }
        // 미끄러져 간 자리를 메모에 적는다. 배율이 다른 모니터로 갔으면 그 배율로 다시 그린다.
        for index in settled {
            self.settle(index);
            self.keep(index, memos);
        }
        // 붙는 움직임이 끝난 포스트잇의 미뤄 둔 일.
        for index in 0..self.notes.len() {
            if self.notes[index].phase != Phase::Idle {
                continue;
            }
            match self.notes[index].then.take() {
                Some(Then::Edit) if self.editor.is_none() => self.edit(index),
                Some(Then::Ring) => self.start_ringing(index),
                _ => {}
            }
        }
        for index in gone.into_iter().rev() {
            window::destroy(self.notes.remove(index).hwnd);
        }

        // 반듯하게 펴졌으면 글을 적는 칸을 올린다.
        if self.editor.is_none()
            && let Some(index) = self.notes.iter().position(|note| note.phase == Phase::Editing)
            && self.notes[index].tilt.x.abs() < 0.004
        {
            self.open_editor(index);
        }
        // 꽃핀 머리 위의 단추들이 떠 있는 동안에도 시간을 흘린다. 펼쳐지고, 때가 되면 닫혀야 한다.
        moving |= self.tick_fan(dt);
        // 위에서 깨운 것이 있으면 계속 흘린다.
        self.awake = moving || self.notes.iter().any(|note| note.stale || note.then.is_some());
        self.rest();
    }

    fn open_editor(&mut self, index: usize) {
        // 글을 적는 칸은 한 번에 하나만 둔다.
        self.close_search();
        let note = &self.notes[index];
        let text = draw::text_rect(note.paper);
        let px = |units: f32| (units * note.scale).round() as i32;
        let rect = (
            note.at.0.round() as i32 + px(text.0),
            note.at.1.round() as i32 + px(text.1),
            px(text.2 - text.0),
            px(text.3 - text.1),
        );
        let byte = |value: f32| (value * 255.0 + 0.5) as u32;
        let paper = byte(note.color.2) << 16 | byte(note.color.1) << 8 | byte(note.color.0);
        let ink = if draw::is_dark(note.color) { 0x00F9_FDFF } else { 0x003D_3A4A };
        let font = text::clear_font(text::FACE, px(15.0), false);
        self.editor = Some((note.hwnd, Editor::open(note.hwnd, rect, &note.text, (paper, ink), font, note.top)));
    }

    /// 적기를 마친다. 적은 글을 포스트잇과 메모에 옮기고 다시 삐딱하게 붙인다.
    fn close_editor(&mut self, memos: &mut Memos) {
        let Some((hwnd, editor)) = self.editor.take() else { return };
        let text: String = editor.text().trim_end().chars().take(TEXT_LIMIT).collect();
        drop(editor);
        if let Some(index) = self.find(hwnd) {
            let note = &mut self.notes[index];
            (note.text, note.phase, note.stale) = (text, Phase::Idle, true);
            self.keep(index, memos);
        }
        self.wake();
    }

    fn handle(&mut self, event: Event, memos: &mut Memos) {
        if self.fan_event(event)
            || self.focus_event(event)
            || self.list_event(event, memos)
            || self.panel_event(event, memos)
        {
            self.wake();
            return;
        }
        let local = |x: i32, y: i32, scale: f32| (x as f32 / scale, y as f32 / scale);
        match event {
            Event::Press { hwnd, x, y } => {
                let Some(index) = self.find(hwnd) else { return };
                let (u, v) = local(x, y, self.notes[index].scale);
                let hit = draw::hit(&self.look(&self.notes[index]), u, v);
                // 늘 위에 두지 않는 포스트잇은 보통 창처럼, 누르면 다른 창들 앞으로 나온다.
                if hit.is_some() && !self.notes[index].top && !self.quiet {
                    window::raise(self.notes[index].hwnd);
                }
                let phase = self.notes[index].phase;
                let cursor = win::cursor_position();
                match hit {
                    Some(Hit::Paper) if phase == Phase::Idle => {
                        self.close_panel();
                        let note = &mut self.notes[index];
                        note.drag = Some((cursor.0 as f32 - note.at.0, cursor.1 as f32 - note.at.1));
                        note.glide = None;
                    }
                    Some(Hit::Corner) if phase == Phase::Idle => {
                        self.close_panel();
                        self.notes[index].resize = Some((self.notes[index].paper, cursor));
                    }
                    Some(Hit::Tool(tool)) if matches!(phase, Phase::Idle | Phase::Editing) => {
                        window::stop_pressing();
                        self.act(index, tool, memos);
                    }
                    Some(Hit::Pin) if matches!(phase, Phase::Idle | Phase::Editing) => {
                        window::stop_pressing();
                        self.act(index, 2, memos);
                    }
                    Some(Hit::Answer(answer)) if matches!(phase, Phase::Ringing(_)) => {
                        window::stop_pressing();
                        self.answer(index, answer == 1);
                    }
                    _ => window::stop_pressing(),
                }
            }
            Event::Move { hwnd, x, y } => {
                let Some(index) = self.find(hwnd) else { return };
                let cursor = win::cursor_position();
                let scale = self.notes[index].scale;
                if let Some(offset) = self.notes[index].drag {
                    let note = &mut self.notes[index];
                    note.at = (cursor.0 as f32 - offset.0, cursor.1 as f32 - offset.1);
                    window::move_to(note.hwnd, note.window_at());
                } else if let Some((start, from)) = self.notes[index].resize {
                    // 커서가 움직인 만큼을 종이의 방향으로 돌려, 그만큼 늘이거나 줄인다.
                    let (dx, dy) = ((cursor.0 - from.0) as f32 / scale, (cursor.1 - from.1) as f32 / scale);
                    let note = &mut self.notes[index];
                    let (sin, cos) = note.rest.sin_cos();
                    note.paper = (
                        (start.0 + dx * cos + dy * sin).clamp(draw::SMALLEST.0, draw::LARGEST.0).round(),
                        (start.1 - dx * sin + dy * cos).clamp(draw::SMALLEST.1, draw::LARGEST.1).round(),
                    );
                    note.stale = true;
                } else {
                    let (u, v) = local(x, y, scale);
                    let hot = draw::hit(&self.look(&self.notes[index]), u, v);
                    let note = &mut self.notes[index];
                    note.stale |= note.hot != hot;
                    (note.hover, note.hot, note.away) = (true, hot, None);
                    window::set_cursor(match hot {
                        Some(Hit::Tool(_) | Hit::Answer(_) | Hit::Pin) => Cursor::Hand,
                        Some(Hit::Corner) if note.phase == Phase::Idle => Cursor::Resize,
                        Some(Hit::Paper) if note.phase == Phase::Idle => Cursor::Move,
                        _ => Cursor::Arrow,
                    });
                }
            }
            Event::Lift { hwnd } => {
                if let Some(index) = self.find(hwnd) {
                    let note = &mut self.notes[index];
                    let touched = note.drag.is_some() || note.resize.is_some();
                    (note.drag, note.resize, note.stale) = (None, None, true);
                    if touched {
                        // 배율이 다른 모니터에 놓았으면 그 배율로 다시 그린다.
                        self.settle(index);
                        // 옮긴 자리와 바꾼 크기를 메모에 적는다.
                        self.keep(index, memos);
                    }
                }
            }
            Event::Leave { hwnd } => {
                if let Some(index) = self.find(hwnd) {
                    let note = &mut self.notes[index];
                    note.stale |= note.hot.is_some();
                    note.hot = None;
                    // 바로 거두지 않는다. 단추로 가다가 잠깐 창 밖을 스쳐도 단추가 남아 있게 한다.
                    if note.hover && note.away.is_none() {
                        note.away = Some(0.0);
                    }
                }
            }
            Event::Double { hwnd, x, y } => {
                let Some(index) = self.find(hwnd) else { return };
                let (u, v) = local(x, y, self.notes[index].scale);
                let hit = draw::hit(&self.look(&self.notes[index]), u, v);
                if hit == Some(Hit::Paper) && self.notes[index].phase == Phase::Idle && self.editor.is_none() {
                    self.close_panel();
                    self.edit(index);
                }
            }
            Event::Menu { hwnd } => {
                if let Some(index) = self.find(hwnd) {
                    let (x, y) = win::cursor_position();
                    self.outcomes.push(Outcome::Menu { id: self.notes[index].id, x, y });
                }
            }
            Event::EditDone { host } => {
                if self.editor.as_ref().is_some_and(|(_, editor)| editor.host() == host) {
                    self.close_editor(memos);
                }
            }
            Event::EditChanged { .. } | Event::Key { .. } | Event::Wheel { .. } => {}
        }
        self.wake();
    }

    /// 단추를 눌렀다: 색, 알림, 항상 위, 지우기.
    fn act(&mut self, index: usize, tool: usize, memos: &mut Memos) {
        match tool {
            0 => self.toggle_panel(index, Kind::Color),
            1 => self.toggle_panel(index, Kind::Alarm),
            2 => {
                let note = &mut self.notes[index];
                note.top = !note.top;
                window::set_layer(note.hwnd, note.top);
                self.keep(index, memos);
            }
            _ => {
                // 적던 글이 있으면 먼저 옮겨 적는다. 빈 메모인지는 그 뒤에 본다.
                if self.editor.as_ref().is_some_and(|(editing, _)| *editing == self.notes[index].hwnd) {
                    self.close_editor(memos);
                }
                let id = self.notes[index].id;
                // 아무것도 적지 않은 메모는 묻지 않고 지운다.
                if self.notes[index].text.trim().is_empty() {
                    self.remove(memos, id);
                } else {
                    self.outcomes.push(Outcome::AskDelete(id));
                }
            }
        }
        if let Some(note) = self.notes.get_mut(index) {
            note.stale = true;
        }
        self.wake();
    }

    /// 메모를 지운다. 포스트잇은 구겨져 꽃잎으로 흩어진다.
    pub fn remove(&mut self, memos: &mut Memos, id: u64) {
        if memos.remove(id) {
            self.outcomes.push(Outcome::Changed);
        }
        let Some(index) = self.find_id(id) else { return };
        let hwnd = self.notes[index].hwnd;
        if self.editor.as_ref().is_some_and(|(editing, _)| *editing == hwnd) {
            self.editor = None;
        }
        if self.panel.as_ref().is_some_and(|panel| panel.note == hwnd) {
            self.close_panel();
        }
        let note = &mut self.notes[index];
        (note.phase, note.then, note.stale) = (Phase::Crumpling(0.0), None, true);
        self.wake();
    }

    fn edit(&mut self, index: usize) {
        self.notes[index].phase = Phase::Editing;
        self.notes[index].stale = true;
        self.wake();
    }

    /// 그 메모의 알림이 울린다. 커지고 빛나며, 다른 창에 가려 있었더라도 맨 앞으로 나온다.
    pub fn ring(&mut self, memos: &mut Memos, id: u64) {
        let Some(index) = self.find_id(id) else { return };
        let hwnd = self.notes[index].hwnd;
        // 적는 중이었으면 적던 글을 옮겨 적고 울린다.
        if self.editor.as_ref().is_some_and(|(editing, _)| *editing == hwnd) {
            self.close_editor(memos);
        }
        match self.notes[index].phase {
            Phase::Idle | Phase::Editing => self.start_ringing(index),
            // 붙는 중이면 다 붙은 뒤에 울린다.
            Phase::Born(_) => self.notes[index].then = Some(Then::Ring),
            _ => {}
        }
        self.wake();
    }

    fn start_ringing(&mut self, index: usize) {
        if self.panel.as_ref().is_some_and(|panel| panel.note == self.notes[index].hwnd) {
            self.close_panel();
        }
        let note = &mut self.notes[index];
        (note.phase, note.drag, note.resize, note.stale) = (Phase::Ringing(0.0), None, None, true);
        window::set_layer(note.hwnd, true);
        self.wake();
    }

    /// 알림에 답했다. 제 모습으로 돌아가고, 있던 층으로 돌아간다.
    fn answer(&mut self, index: usize, snooze: bool) {
        let note = &mut self.notes[index];
        if !matches!(note.phase, Phase::Ringing(_)) {
            return;
        }
        (note.phase, note.stale) = (Phase::Idle, true);
        window::set_layer(note.hwnd, note.top);
        self.outcomes.push(Outcome::Answered { id: note.id, snooze });
        self.wake();
    }

    /// 메모의 알림이 밖에서 바뀌었다(울렸거나 미뤘다). 포스트잇에 적힌 알림 시각을 맞춘다.
    pub fn refresh(&mut self, memos: &Memos) {
        let mut changed = false;
        for note in &mut self.notes {
            let Some(memo) = memos.get(note.id) else { continue };
            let alarm = Alarm::from(memo.remind);
            if note.alarm != alarm || note.next != memo.next {
                (note.alarm, note.next, note.stale) = (alarm, memo.next, true);
                changed = true;
            }
        }
        if changed {
            self.wake();
        }
    }

    /// 새 메모를 만들어 `near`(화면 좌표) 가까이에 붙인다. 다 붙으면 바로 글을 적을 수 있게 편다.
    /// 더 둘 수 없으면 `None`이다.
    pub fn create(&mut self, memos: &mut Memos, near: (i32, i32), write: bool) -> Option<u64> {
        let id = memos.add(self.wall)?;
        let scale = self.scale_at((near.0 as f32, near.1 as f32));
        // 화면 밖으로 나가지 않게 한다. 위로는 단추가, 아래로는 답하는 단추가 나올 자리를 남긴다.
        let work = window::work_area(near, true).unwrap_or((0, 0, 1920, 1040));
        let paper = draw::PAPER;
        let fit = |value: f32, least: f32, most: f32| value.clamp(least, most.max(least));
        let at = (
            fit(
                near.0 as f32 - paper.0 / 2.0 * scale,
                work.0 as f32 + 20.0 * scale,
                work.2 as f32 - (paper.0 + 20.0) * scale,
            ),
            fit(near.1 as f32 + 24.0 * scale, work.1 as f32 + 60.0 * scale, work.3 as f32 - (paper.1 + 60.0) * scale),
        );
        let sheet = self.fresh_sheet((at.0.round() as i32, at.1.round() as i32));
        let memo = memos.get_mut(id)?;
        memo.sheet = sheet;
        let memo = memo.clone();
        self.outcomes.push(Outcome::Changed);
        self.add(&memo, true);
        if let (true, Some(note)) = (write, self.notes.last_mut()) {
            note.then = Some(Then::Edit);
        }
        Some(id)
    }

    /// 다시 줄 세운다. 마우스 커서가 있는 모니터의 왼쪽 위부터 차례로 놓는다.
    pub fn tidy(&mut self) {
        self.close_panel();
        // 지워지는 중인 것은 세지 않는다.
        let staying: Vec<usize> = (0..self.notes.len()).filter(|&index| !self.notes[index].is_leaving()).collect();
        // 가장 큰 포스트잇에 맞춰 칸을 잡는다.
        let cell = staying
            .iter()
            .map(|&index| self.notes[index].paper)
            .fold((0.0f32, 0.0f32), |cell, paper| (cell.0.max(paper.0), cell.1.max(paper.1)));
        for (spot, index) in staying.into_iter().enumerate() {
            let to = self.grid(spot, cell);
            let note = &mut self.notes[index];
            note.glide = Some((note.at, to, 0.0));
        }
        self.wake();
    }

    /// 모니터 구성이 바뀌었다. 화면 밖에 남은 포스트잇을 가장 가까운 화면 안으로 데려온다.
    pub fn rescue(&mut self) {
        for note in &mut self.notes {
            let scale = note.scale;
            let middle = (note.middle().0 as i32, note.middle().1 as i32);
            if window::work_area(middle, false).is_some() {
                continue;
            }
            if let Some(work) = window::work_area(middle, true) {
                let fit = |value: f32, least: f32, most: f32| value.clamp(least, most.max(least));
                let to = (
                    fit(note.at.0, work.0 as f32 + 20.0 * scale, work.2 as f32 - (note.paper.0 + 20.0) * scale),
                    fit(note.at.1, work.1 as f32 + 40.0 * scale, work.3 as f32 - (note.paper.1 + 20.0) * scale),
                );
                note.glide = Some((note.at, to, 0.0));
            }
        }
        self.settle_list();
        self.wake();
    }

    /// 모든 포스트잇을 다른 창들 앞으로 한 번 가져온다. 다른 창을 누르면 다시 그 창에 가려진다.
    pub fn show_all(&self) {
        for note in self.notes.iter().filter(|note| !note.top) {
            window::raise(note.hwnd);
        }
    }

    /// 끝내기 전에 부른다. 적던 글이 있으면 메모에 옮겨 적는다.
    pub fn commit(&mut self, memos: &mut Memos) -> Vec<Outcome> {
        self.close_editor(memos);
        std::mem::take(&mut self.outcomes)
    }

    /// 포스트잇 옆에 판을 띄운다. 같은 판이 이미 떠 있으면 닫는다.
    fn toggle_panel(&mut self, index: usize, kind: Kind) {
        let hwnd = self.notes[index].hwnd;
        let same = self.panel.as_ref().is_some_and(|panel| panel.kind == kind && panel.note == hwnd);
        self.close_panel();
        if same {
            return;
        }
        let size = Panel::size(kind);
        let note = &self.notes[index];
        // 판은 포스트잇과 같은 배율로 그린다.
        let scale = note.scale;
        self.panel_scale = scale;
        let (wide, tall) = ((size.0 * scale).round() as i32, (size.1 * scale).round() as i32);
        // 오른쪽에 띄우고, 자리가 없으면 왼쪽에 띄운다.
        let work = window::work_area((note.at.0 as i32, note.at.1 as i32), true).unwrap_or((0, 0, 1920, 1040));
        let mut x = (note.at.0 + (note.paper.0 + 6.0) * scale) as i32;
        if x + wide > work.2 {
            x = (note.at.0 - 6.0 * scale) as i32 - wide;
        }
        let y = ((note.at.1 - 24.0 * scale) as i32).clamp(work.1, (work.3 - tall).max(work.1));
        self.panel_at = (x, y);
        self.draft = Draft::from(note.alarm, (self.wall.day(), self.wall.minute_of_day()));
        self.hsv = draw::to_hsv(note.color);
        self.panel = Some(Panel::new(window::create(self.panel_at, (wide, tall), true), kind, hwnd));
        self.paint_panel();
        if let Some(panel) = &self.panel {
            window::show(panel.hwnd);
        }
        self.notes[index].stale = true;
    }

    fn close_panel(&mut self) {
        if let Some(panel) = self.panel.take() {
            window::destroy(panel.hwnd);
            if let Some(index) = self.find(panel.note) {
                self.notes[index].stale = true;
            }
        }
    }

    /// 판을 공용 버퍼에 그린다. 그린 그림의 크기(px)를 돌려준다.
    fn draw_panel(&mut self) -> Option<(i32, i32)> {
        self.ready();
        let note = self.panel.as_ref().and_then(|panel| self.find(panel.note))?;
        let (panel, tools) = (self.panel.as_mut()?, self.tools.as_mut()?);
        let size = Panel::size(panel.kind);
        let scale = self.panel_scale;
        let (wide, tall) = ((size.0 * scale).round() as i32, (size.1 * scale).round() as i32);
        tools.canvas.fit(wide, tall);
        let draft = self.draft;
        let model = Model {
            color: self.notes[note].color,
            hsv: self.hsv,
            mode: draft.mode,
            day: draft.day,
            hour: draft.hour,
            minute: draft.minute,
            today: self.wall.day(),
            week: draft.week,
            past: draft.mode == 1 && draft.alarm().is_past(self.wall),
        };
        let stride = tools.canvas.width as usize;
        let mut surface = Surface::new(tools.canvas.pixels(), stride, (wide, tall), scale);
        panel.paint(&mut surface, &model);
        Some((wide, tall))
    }

    fn paint_panel(&mut self) {
        let Some(size) = self.draw_panel() else { return };
        if let (Some(panel), Some(tools)) = (&self.panel, &self.tools) {
            tools.canvas.present(panel.hwnd, self.panel_at, size);
        }
    }

    /// 판에서 일어난 일이면 처리하고 `true`를 돌려준다.
    fn panel_event(&mut self, event: Event, memos: &mut Memos) -> bool {
        let Some(panel) = &mut self.panel else { return false };
        let scale = self.panel_scale;
        match event {
            Event::Press { hwnd, x, y } if hwnd == panel.hwnd => match panel.hit(x as f32 / scale, y as f32 / scale) {
                Some(Action::Slide(which)) => {
                    panel.sliding = Some(which);
                    self.slide(x);
                }
                Some(action) => {
                    window::stop_pressing();
                    self.panel_act(action, memos);
                }
                None => window::stop_pressing(),
            },
            Event::Move { hwnd, x, y } if hwnd == panel.hwnd => {
                if panel.sliding.is_some() {
                    self.slide(x);
                } else {
                    let hot = panel.hit(x as f32 / scale, y as f32 / scale);
                    window::set_cursor(if hot.is_some() { Cursor::Hand } else { Cursor::Arrow });
                    if panel.hot != hot {
                        panel.hot = hot;
                        self.paint_panel();
                    }
                }
            }
            Event::Lift { hwnd } if hwnd == panel.hwnd => {
                // 막대를 놓았다. 고른 색을 메모에 적는다.
                let note = panel.note;
                if panel.sliding.take().is_some()
                    && let Some(index) = self.find(note)
                {
                    self.keep(index, memos);
                }
            }
            Event::Leave { hwnd } if hwnd == panel.hwnd => {
                if panel.hot.take().is_some() {
                    self.paint_panel();
                }
            }
            Event::Wheel { hwnd, x, y, up } if hwnd == panel.hwnd => {
                // 시와 분 위에서 휠을 굴리면 그 값이 바뀐다.
                let step = if up { 1 } else { -1 };
                if let Some(action) = panel.wheel(x as f32 / scale, y as f32 / scale, step) {
                    self.panel_act(action, memos);
                }
            }
            Event::Double { hwnd, .. } | Event::Menu { hwnd } if hwnd == panel.hwnd => {}
            _ => return false,
        }
        true
    }

    /// 색을 직접 고르는 막대를 민다.
    fn slide(&mut self, x: i32) {
        let Some(panel) = &self.panel else { return };
        let (Some(which), Some(index)) = (panel.sliding, self.find(panel.note)) else { return };
        let value = panel.slider_value(which, x as f32 / self.panel_scale);
        match which {
            0 => self.hsv.0 = value,
            1 => self.hsv.1 = value,
            _ => self.hsv.2 = value,
        }
        self.notes[index].color = draw::from_hsv(self.hsv.0, self.hsv.1, self.hsv.2);
        self.notes[index].stale = true;
        self.paint_panel();
        self.wake();
    }

    /// 판에서 누른 것을 포스트잇과 메모에 반영한다.
    fn panel_act(&mut self, action: Action, memos: &mut Memos) {
        let Some(index) = self.panel.as_ref().and_then(|panel| self.find(panel.note)) else { return };
        match action {
            Action::Close => return self.close_panel(),
            Action::Swatch(color) => {
                self.notes[index].color = COLORS[color % COLORS.len()];
                self.hsv = draw::to_hsv(self.notes[index].color);
                self.keep(index, memos);
            }
            Action::Slide(_) => {}
            _ => {
                self.draft.apply(action, (self.wall.day(), self.wall.minute_of_day()));
                let alarm = self.draft.alarm();
                self.set_alarm(index, alarm, memos);
            }
        }
        self.notes[index].stale = true;
        self.paint_panel();
        self.wake();
    }

    /// 포스트잇의 알림을 바꾸고 메모에 적는다. 다음에 알릴 시각은 메모가 정한다.
    fn set_alarm(&mut self, index: usize, alarm: Alarm, memos: &mut Memos) {
        let note = &mut self.notes[index];
        note.alarm = alarm;
        let Some(memo) = memos.get_mut(note.id) else { return };
        if memo.remind != alarm.remind() {
            memo.set_remind(alarm.remind(), self.wall);
            self.outcomes.push(Outcome::Changed);
        }
        (note.next, note.stale) = (memo.next, true);
    }

    /// 확인용: 사람이 포스트잇에서 하는 일을 흉내 낸다. 메모는 번호로 가리킨다.
    pub fn script(&mut self, step: &MemoStep, memos: &mut Memos, shot: Option<&Path>) {
        let index = |postits: &Postits, id: u64| postits.find_id(id);
        match step {
            MemoStep::New(text) => {
                // 앞서 만든 것의 오른쪽에 놓는다. 화면에 겹쳐 쌓이지 않게 하려는 것이다.
                let near = self
                    .notes
                    .last()
                    .map_or((300, 200), |note| (note.at.0 as i32 + self.px(330.0), note.at.1 as i32 - self.px(64.0)));
                if let (Some(id), false) = (self.create(memos, near, false), text.is_empty())
                    && let Some(at) = index(self, id)
                {
                    self.notes[at].text = text.clone();
                    self.keep(at, memos);
                }
            }
            MemoStep::Text(id, text) => {
                if let Some(at) = index(self, *id) {
                    (self.notes[at].text, self.notes[at].stale) = (text.clone(), true);
                    self.keep(at, memos);
                }
            }
            MemoStep::Remind(id, remind) => {
                if let Some(at) = index(self, *id) {
                    self.set_alarm(at, Alarm::from(*remind), memos);
                }
            }
            MemoStep::Hold(id) => {
                if let Some(memo) = memos.get_mut(*id) {
                    memo.held = !memo.held;
                    self.outcomes.push(Outcome::Changed);
                }
            }
            MemoStep::Tool(id, tool) => {
                if let Some(at) = index(self, *id) {
                    self.act(at, *tool, memos);
                }
            }
            MemoStep::Hover(id, tool) => {
                if let Some(at) = index(self, *id) {
                    let note = &mut self.notes[at];
                    (note.hover, note.hot, note.away, note.stale) = (true, tool.map(Hit::Tool), None, true);
                }
            }
            MemoStep::Leave(id) => {
                if let Some(at) = index(self, *id) {
                    let note = &mut self.notes[at];
                    (note.hot, note.away) = (None, note.hover.then_some(0.0));
                }
            }
            MemoStep::Edit(id) => {
                if let Some(at) = index(self, *id) {
                    self.edit(at);
                }
            }
            MemoStep::Done => self.close_editor(memos),
            MemoStep::Move(id, x, y) => {
                if let Some(at) = index(self, *id) {
                    let note = &mut self.notes[at];
                    note.glide = Some((note.at, (*x as f32, *y as f32), 0.0));
                }
            }
            MemoStep::Size(id, wide, tall) => {
                if let Some(at) = index(self, *id) {
                    let note = &mut self.notes[at];
                    note.paper = (
                        (*wide as f32).clamp(draw::SMALLEST.0, draw::LARGEST.0),
                        (*tall as f32).clamp(draw::SMALLEST.1, draw::LARGEST.1),
                    );
                    note.stale = true;
                    self.keep(at, memos);
                }
            }
            MemoStep::Panel(id, which) => match (index(self, *id), which.as_str()) {
                (Some(at), "color") => self.toggle_panel(at, Kind::Color),
                (Some(at), "alarm") => self.toggle_panel(at, Kind::Alarm),
                _ => self.close_panel(),
            },
            MemoStep::Press(what, amount) => {
                let action = match what.as_str() {
                    "swatch" => Action::Swatch(*amount as usize),
                    "mode" => Action::Mode(*amount as u8),
                    "quick" => Action::Quick(*amount as u8),
                    "half" => Action::Half(*amount != 0),
                    "hour" => Action::Hour(*amount),
                    "minute" => Action::Minute(*amount),
                    "day" => Action::Day(self.wall.day() + *amount as i64),
                    "week" => Action::Week(*amount),
                    _ => Action::Close,
                };
                self.panel_act(action, memos);
            }
            MemoStep::Custom(hue, saturation, value) => {
                if let Some(at) = self.panel.as_ref().and_then(|panel| self.find(panel.note)) {
                    self.hsv = (*hue, *saturation, *value);
                    self.notes[at].color = draw::from_hsv(*hue, *saturation, *value);
                    self.notes[at].stale = true;
                    self.keep(at, memos);
                    self.paint_panel();
                }
            }
            MemoStep::Answer(id, snooze) => {
                if let Some(at) = index(self, *id) {
                    self.answer(at, *snooze);
                }
            }
            MemoStep::Tidy => self.tidy(),
            MemoStep::Show => self.show_all(),
            MemoStep::List(step) => self.list_script(step, memos),
            MemoStep::Snap => {
                if let Some(path) = shot {
                    self.snap(path, memos);
                }
            }
        }
        self.wake();
    }

    /// 확인용: 지금의 포스트잇들과 판, 메모 목록을 화면에 놓인 자리대로 한 장의 그림(BMP)으로 저장한다.
    fn snap(&mut self, path: &Path, memos: &Memos) {
        // 창마다 자리와 크기.
        let mut windows: Vec<((i32, i32), (i32, i32))> =
            self.notes.iter().map(|note| (note.window_at(), note.canvas_px())).collect();
        if let Some(panel) = &self.panel {
            let (size, scale) = (Panel::size(panel.kind), self.panel_scale);
            windows.push((self.panel_at, ((size.0 * scale).round() as i32, (size.1 * scale).round() as i32)));
        }
        // 메모 목록은 맨 위에 얹는다.
        let listed = self.list_frame();
        windows.extend(listed);
        let last = windows.len().saturating_sub(1);
        let left = windows.iter().map(|w| w.0.0).min().unwrap_or(0);
        let top = windows.iter().map(|w| w.0.1).min().unwrap_or(0);
        let right = windows.iter().map(|w| w.0.0 + w.1.0).max().unwrap_or(1);
        let bottom = windows.iter().map(|w| w.0.1 + w.1.1).max().unwrap_or(1);
        let (width, height) = ((right - left).max(1) as usize, (bottom - top).max(1) as usize);
        // 바탕화면을 흉내 낸 파란 바탕.
        let mut desk = vec![[0x3Bu8, 0x78, 0xDC]; width * height];
        for (index, &(at, size)) in windows.iter().enumerate() {
            if index < self.notes.len() {
                self.paint(index);
            } else if listed.is_some() && index == last {
                self.draw_list(memos);
            } else {
                self.draw_panel();
            }
            let Some(tools) = &mut self.tools else { continue };
            let stride = tools.canvas.width as usize;
            let pixels = tools.canvas.pixels();
            for y in 0..size.1 {
                for x in 0..size.0 {
                    let pixel = pixels[y as usize * stride + x as usize];
                    if pixel >> 24 != 0 {
                        let under = &mut desk[(at.1 - top + y) as usize * width + (at.0 - left + x) as usize];
                        crate::dev::blend(under, pixel);
                    }
                }
            }
        }
        crate::dev::write_bmp(path, width, &desk);
        // 화면에 떠 있는 그림은 그대로이므로, 다음에 고칠 때 다시 그리게만 해 둔다.
        self.notes.iter_mut().for_each(|note| note.stale = true);
        self.written = None;
    }

    /// 확인용: 포스트잇들과 메모 목록이 지금 어떤지를 적은 줄들.
    pub fn describe(&self, memos: &Memos) -> Vec<String> {
        let notes = self.notes.iter();
        let mut lines: Vec<String> = notes
            .map(|note| {
                let phase = match note.phase {
                    Phase::Idle if note.found.is_some() => "반짝이는 중",
                    Phase::Idle => "붙어 있음",
                    Phase::Born(_) => "붙는 중",
                    Phase::Ringing(_) => "울리는 중",
                    Phase::Crumpling(_) | Phase::Petals(_) => "지워지는 중",
                    Phase::Editing => "적는 중",
                };
                let sheet = note.sheet();
                format!(
                    "포스트잇 {}: {phase}, 자리 {:?}, 크기 {}x{}(배율 {:.0}%), 색 #{:06X}, 기울기 {:.1}도, 항상 위 {}, 알림 {}",
                    note.id,
                    sheet.at.unwrap_or_default(),
                    sheet.size.0,
                    sheet.size.1,
                    note.scale * 100.0,
                    sheet.color,
                    sheet.tilt as f32 / 10.0,
                    if note.top { "켬" } else { "끔" },
                    note.chip(self.wall.day()).unwrap_or_else(|| "없음".to_string()),
                )
            })
            .collect();
        lines.extend(self.describe_list(memos));
        lines
    }
}
