//! 메모 목록: 메모를 한데 모아 보고 관리하는 창.
//!
//! 좁은 창에 메모를 한 줄씩 보여 준다. 찾기, 고르기(모두·알림·항상 위·맡김), 차례 바꾸기가 되고, 줄을 누르면 그 아래에
//! 단추가 나온다: 찾아가기, 항상 위, 맡기기, 지우기. 글과 색과 알림은 여기서 고치지 않고 포스트잇에서 고친다.
//!
//! 색을 고르는 판이나 알림을 정하는 판과 같은 방식으로 직접 그린다. 찾는 말을 적는 칸만 Windows의 입력 칸을
//! 그 자리에 올린다. 포스트잇과 달리 누르면 앞으로 나오고 키보드를 받는 보통 창이다.
//!
//! 앞쪽에는 무엇을 보여 줄지 고르고 차례를 정하는 일, 줄을 놓는 일, 그리는 일, 누른 곳을 가리는 일이 있다([`List`]).
//! 뒤쪽에는 그것을 포스트잇과 잇는 일이 있다.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::f32::consts::FRAC_1_SQRT_2;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::HWND;

use super::draw::{self, COLORS, CREAM, DEEP, HOT, INK, PINK, ROSE, Rgb, WHITE, from_hsv, mix, rgb, round_box, to_hsv};
use super::panel::Surface;
use super::text::{self, Block};
use super::window::{self, Cursor, Editor, Event, KEY_ESCAPE};
use super::{Outcome, Phase, Postits};
use crate::dev::ListStep;
use crate::memos::{COUNT_LIMIT, Memos};
use crate::when::When;
use crate::win;

/// 창 둘레에 그림자가 들어갈 여백.
const MARGIN: f32 = 14.0;
/// 창 안쪽(테두리까지)의 크기.
const BODY: (f32, f32) = (348.0, 550.0);
/// 찾기 칸과 차례 단추가 놓인 줄의 위 끝.
const TOOLS_TOP: f32 = 44.0;
/// 차례 단추의 너비.
const SORT_WIDE: f32 = 108.0;
/// 고르는 단추들이 놓인 줄의 위 끝.
const CHIPS_TOP: f32 = 84.0;
/// 메모 줄들이 보이는 곳의 위 끝과 높이.
const ROWS_TOP: f32 = 118.0;
const ROWS_TALL: f32 = 376.0;
/// 글이 한 줄인 메모와 두 줄인 메모의 줄 높이, 고른 줄에 더해지는 단추 줄의 높이.
const ROW_ONE: f32 = 36.0;
const ROW_TWO: f32 = 50.0;
const ROW_ACTS: f32 = 34.0;
/// 휠 한 칸에 넘기는 높이.
const WHEEL: f32 = 48.0;
/// 찾는 말의 글자 수.
const QUERY_LIMIT: usize = 40;
/// 그림을 다시 그리는 가장 짧은 간격. 막대를 끌어 넘길 때 마우스가 움직일 때마다 그리지 않게 한다.
const REDRAW_EVERY: Duration = Duration::from_millis(30);
/// 글자 그림을 이만큼 넘게 모았으면 버리고 다시 모은다.
const KEPT_LABELS: usize = 400;

const DIM: Rgb = rgb(0x8E7A7F);
const FAINT: Rgb = rgb(0xB9A8AC);
const LINE: Rgb = rgb(0xF3C3CD);
const HOVER: Rgb = rgb(0xFFF4F6);
const TAG: Rgb = rgb(0xFBDDE4);
const SHADOW: Rgb = rgb(0x141E46);
/// 아이콘 글꼴의 글자: 닫기, 돋보기, 종, 차례.
const CLOSE: &str = "\u{E711}";
const SEARCH: &str = "\u{E721}";
const BELL: &str = "\u{EA8F}";
const ORDER: &str = "\u{E8CB}";

type Rect = (f32, f32, f32, f32);

/// 목록에 보이는 메모 하나.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: u64,
    /// 글의 첫 줄. 빈 메모면 비어 있다.
    pub first: String,
    /// 나머지 줄들을 한 줄로 이은 것.
    pub rest: String,
    /// 찾을 때 견주는 글. 글 전체를 소문자로 바꾼 것이다.
    pub search: String,
    /// 알림 시각("오늘 오후 7:00"). 알릴 일이 남아 있을 때만 있다.
    pub chip: Option<String>,
    /// 다음에 알릴 시각. 차례를 정하는 데 쓴다.
    pub next: Option<When>,
    pub top: bool,
    /// 꽃핀에게 맡긴 메모다.
    pub held: bool,
    pub color: Rgb,
    /// 포스트잇이 붙어 있는 기울기(라디안). 색 조각을 그만큼 기울인다.
    pub tilt: f32,
}

impl Entry {
    /// 줄이 두 줄짜리인가. 첫 줄 아래에 보여 줄 것(꼬리표나 나머지 글)이 있으면 그렇다.
    fn is_tall(&self) -> bool {
        self.top || self.held || !self.rest.is_empty()
    }
}

/// 어떤 메모만 보여 줄지.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Filter {
    All,
    /// 알릴 일이 남아 있는 메모.
    Alarm,
    /// "항상 위"를 켠 메모.
    Top,
    /// 꽃핀에게 맡긴 메모.
    Held,
}

impl Filter {
    pub const ALL: [Filter; 4] = [Filter::All, Filter::Alarm, Filter::Top, Filter::Held];

    pub fn name(self) -> &'static str {
        match self {
            Filter::All => "모두",
            Filter::Alarm => "알림",
            Filter::Top => "항상 위",
            Filter::Held => "맡김",
        }
    }

    fn passes(self, entry: &Entry) -> bool {
        match self {
            Filter::All => true,
            Filter::Alarm => entry.chip.is_some(),
            Filter::Top => entry.top,
            Filter::Held => entry.held,
        }
    }
}

/// 어떤 차례로 보여 줄지.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sort {
    /// 알림이 빠른 것부터. 알림이 없는 메모는 그 뒤에 새로 만든 것부터 온다.
    Due,
    /// 새로 만든 것부터.
    New,
    /// 같은 색끼리 모아서.
    Color,
}

impl Sort {
    pub fn name(self) -> &'static str {
        match self {
            Sort::Due => "알림 빠른 순",
            Sort::New => "새로 만든 순",
            Sort::Color => "색깔별",
        }
    }

    /// 차례 단추를 누르면 바뀌는 다음 차례.
    fn next(self) -> Sort {
        match self {
            Sort::Due => Sort::New,
            Sort::New => Sort::Color,
            Sort::Color => Sort::Due,
        }
    }
}

/// 고른 줄 아래에 나오는 단추.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Act {
    /// 그 포스트잇을 앞으로 가져와 반짝이게 한다.
    Find,
    Top,
    Hold,
    Delete,
    /// 지울지 물었을 때의 "지우기"와 "아니요".
    Really,
    Keep,
}

/// 맨 아래의 단추.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Foot {
    Add,
    Tidy,
    Raise,
}

/// 목록 창에서 누를 수 있는 것.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Spot {
    /// 제목 줄. 끌어서 창을 옮긴다.
    Head,
    Close,
    /// 찾는 말을 적는 칸.
    Search,
    /// 찾는 말 지우기.
    Clear,
    Sort,
    Filter(Filter),
    Row(u64),
    Act(Act),
    Foot(Foot),
    /// 넘기는 막대.
    Scroll,
}

/// `entries`(새로 만든 것이 앞) 가운데 보여 줄 것들을 보여 줄 차례대로 고른다. 돌려주는 것은 `entries`에서의 자리다.
pub fn shown(entries: &[Entry], query: &str, filter: Filter, sort: Sort) -> Vec<usize> {
    let needle = query.trim().to_lowercase();
    let mut picked: Vec<usize> = (0..entries.len())
        .filter(|&at| filter.passes(&entries[at]) && (needle.is_empty() || entries[at].search.contains(&needle)))
        .collect();
    match sort {
        Sort::New => {}
        Sort::Due => picked.sort_by_key(|&at| (entries[at].next.is_none(), entries[at].next, at)),
        Sort::Color => picked.sort_by_key(|&at| (shade_rank(entries[at].color), at)),
    }
    picked
}

/// 색으로 줄 세울 때의 차례. 기본 색은 고르는 판에 놓인 차례대로, 직접 고른 색은 그 뒤에 색상 차례로 온다.
fn shade_rank(color: Rgb) -> (usize, u32) {
    let near = |other: &Rgb| (other.0 - color.0).abs() + (other.1 - color.1).abs() + (other.2 - color.2).abs() < 0.01;
    match COLORS.iter().position(near) {
        Some(base) => (base, 0),
        None => (COLORS.len(), (to_hsv(color).0 * 3600.0) as u32),
    }
}

/// 줄 하나가 놓이는 자리. `top`은 줄들이 놓인 곳의 위 끝에서 잰 것이다.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    /// `entries`에서의 자리.
    pub at: usize,
    pub top: f32,
    pub tall: f32,
}

/// 보여 줄 줄들을 위에서부터 차례로 놓는다. 고른 줄은 단추 줄만큼 더 높다. 줄들 전체의 높이도 돌려준다.
pub fn slots(entries: &[Entry], shown: &[usize], selected: Option<u64>) -> (Vec<Slot>, f32) {
    let mut top = 2.0;
    let slots = shown
        .iter()
        .map(|&at| {
            let entry = &entries[at];
            let tall = if entry.is_tall() { ROW_TWO } else { ROW_ONE }
                + if selected == Some(entry.id) { ROW_ACTS } else { 0.0 };
            top += tall;
            Slot { at, top: top - tall, tall }
        })
        .collect();
    (slots, top + 6.0)
}

/// 그릴 준비를 마친 것: 보여 줄 줄들과, 고르는 단추마다의 메모 수.
pub struct Layout {
    pub slots: Vec<Slot>,
    pub counts: [usize; 4],
}

/// 그릴 때 받는 것.
pub struct View<'a> {
    pub entries: &'a [Entry],
    /// 둘 수 있는 메모의 수.
    pub limit: usize,
    /// 찾는 말을 적는 중이다. 적는 칸이 그 자리를 덮고 있으므로 찾는 말을 그리지 않는다.
    pub typing: bool,
}

/// 글을 어디에 맞춰 놓을지.
#[derive(Clone, Copy)]
enum Anchor {
    Left(f32),
    Center(f32),
}

/// 단추의 생김새.
#[derive(Clone, Copy, PartialEq)]
enum Style {
    /// 테두리만 있는 단추.
    Plain,
    /// 테두리만 있고 글이 굵은 단추.
    Strong,
    /// 색으로 채운 단추. 켜져 있거나 가장 먼저 누를 것이다.
    Filled,
}

pub struct List {
    pub hwnd: HWND,
    /// 창의 왼쪽 위가 놓인 화면 좌표.
    pub at: (i32, i32),
    /// 화면 배율(100%면 1). 창이 놓인 모니터의 것을 따른다.
    pub scale: f32,
    pub query: String,
    pub filter: Filter,
    pub sort: Sort,
    pub selected: Option<u64>,
    /// 고른 메모를 지울지 묻는 중이다.
    pub asking: bool,
    /// 줄들을 위로 넘긴 높이.
    pub scroll: f32,
    /// 마우스가 올라가 있는 것.
    pub hot: Option<Spot>,
    /// 창을 끄는 중: 커서에서 창의 왼쪽 위까지의 거리.
    moving: Option<(i32, i32)>,
    /// 넘기는 막대를 끄는 중: 잡은 곳에서 막대의 위 끝까지의 거리.
    scrolling: Option<f32>,
    /// 고른 줄이 다 보이게 넘겨야 한다.
    reveal: bool,
    /// 줄들 전체의 높이. 줄을 놓을 때마다 고친다.
    content: f32,
    /// 마지막으로 그린 것의 지문과 그린 때. 같은 것을 또 그리지 않으려고 둔다.
    painted: Option<u64>,
    painted_at: Instant,
    /// 마지막으로 그렸을 때, 누를 수 있는 곳들.
    spots: Vec<(Rect, Spot)>,
    /// 한 번 그린 글자는 다시 그리지 않는다.
    labels: HashMap<(String, i32, bool, bool), Block>,
    /// 칸에 맞춰 줄인 글도 다시 재지 않는다. 열쇠는 글, 글자 크기, 굵은지, 칸의 너비다.
    fits: HashMap<(String, i32, bool, i32), String>,
}

impl List {
    pub fn new(hwnd: HWND, at: (i32, i32), scale: f32) -> List {
        List {
            hwnd,
            at,
            scale,
            query: String::new(),
            filter: Filter::All,
            sort: Sort::Due,
            selected: None,
            asking: false,
            scroll: 0.0,
            hot: None,
            moving: None,
            scrolling: None,
            reveal: false,
            content: 0.0,
            painted: None,
            painted_at: Instant::now(),
            spots: Vec::new(),
            labels: HashMap::new(),
            fits: HashMap::new(),
        }
    }

    /// 창의 크기. 둘레의 그림자 여백을 더한 것이다.
    pub fn size() -> (f32, f32) {
        (BODY.0 + 2.0 * MARGIN, BODY.1 + 2.0 * MARGIN + 6.0)
    }

    /// 창의 크기(px).
    fn size_px(&self) -> (i32, i32) {
        let size = List::size();
        ((size.0 * self.scale).ceil() as i32, (size.1 * self.scale).ceil() as i32)
    }

    /// 창 안의 한 점에 있는, 누를 수 있는 것.
    pub fn hit(&self, x: f32, y: f32) -> Option<Spot> {
        let found =
            self.spots.iter().find(|(rect, _)| (rect.0..=rect.2).contains(&x) && (rect.1..=rect.3).contains(&y));
        found.map(|&(_, spot)| spot)
    }

    /// 찾는 말을 적는 칸이 올라갈 자리(왼쪽, 위, 오른쪽, 아래).
    fn search_rect() -> Rect {
        let top = MARGIN + TOOLS_TOP;
        (MARGIN + 44.0, top + 7.0, MARGIN + BODY.0 - 14.0 - SORT_WIDE - 8.0 - 28.0, top + 24.0)
    }

    /// 고른 메모를 바꾼다. 다 보이게 넘기고, 지울지 묻던 것은 그만둔다.
    fn select(&mut self, id: Option<u64>) {
        if self.selected != id {
            (self.selected, self.asking, self.reveal) = (id, false, id.is_some());
        }
    }

    /// 보여 줄 것을 고르고 줄을 놓는다. 고른 메모가 보이지 않게 됐으면 고른 것을 푼다.
    pub fn arrange(&mut self, entries: &[Entry]) -> Layout {
        let shown = shown(entries, &self.query, self.filter, self.sort);
        if self.selected.is_some_and(|id| !shown.iter().any(|&at| entries[at].id == id)) {
            (self.selected, self.asking) = (None, false);
        }
        let (slots, content) = slots(entries, &shown, self.selected);
        if std::mem::take(&mut self.reveal)
            && let Some(slot) = slots.iter().find(|slot| Some(entries[slot.at].id) == self.selected)
        {
            self.scroll = self.scroll.max(slot.top + slot.tall + 4.0 - ROWS_TALL).min(slot.top - 2.0);
        }
        self.content = content;
        self.scroll = self.scroll.clamp(0.0, (content - ROWS_TALL).max(0.0));
        Layout { slots, counts: Filter::ALL.map(|filter| entries.iter().filter(|entry| filter.passes(entry)).count()) }
    }

    /// 넘기는 막대의 위 끝과 길이. 줄들이 보이는 곳의 위 끝에서 잰다. 넘길 것이 없으면 `None`이다.
    fn thumb(&self) -> Option<(f32, f32)> {
        let over = self.content - ROWS_TALL;
        if over <= 0.0 {
            return None;
        }
        let track = ROWS_TALL - 8.0;
        let long = (track * ROWS_TALL / self.content).max(28.0);
        Some((4.0 + (track - long) * (self.scroll / over).clamp(0.0, 1.0), long))
    }

    /// 넘기는 막대가 놓인 곳을 눌렀다. 막대 밖을 눌렀으면 막대의 한가운데가 그리로 온다.
    fn grab_scroll(&mut self, y: f32) {
        let Some((top, long)) = self.thumb() else { return };
        let y = y - MARGIN - ROWS_TOP;
        let held = if (top..=top + long).contains(&y) { y - top } else { long / 2.0 };
        self.scrolling = Some(held);
        self.drag_scroll(y + MARGIN + ROWS_TOP);
    }

    /// 넘기는 막대를 끈다.
    fn drag_scroll(&mut self, y: f32) {
        let (Some(held), Some((_, long))) = (self.scrolling, self.thumb()) else { return };
        let (over, track) = (self.content - ROWS_TALL, ROWS_TALL - 8.0);
        let top = y - MARGIN - ROWS_TOP - held;
        self.scroll = ((top - 4.0) / (track - long).max(1.0) * over).clamp(0.0, over);
    }

    /// 그려질 것의 지문. 이것이 같으면 그림도 같다.
    fn print(&self, view: &View) -> u64 {
        let mut hasher = DefaultHasher::new();
        (view.limit, view.typing, &self.query, self.filter, self.sort, self.selected, self.asking).hash(&mut hasher);
        (self.hot, self.scrolling.is_some(), self.scroll.to_bits(), self.scale.to_bits()).hash(&mut hasher);
        for entry in view.entries {
            (entry.id, &entry.first, &entry.rest, &entry.search, &entry.chip, entry.top, entry.held).hash(&mut hasher);
            entry.next.map(|next| (next.day(), next.minute_of_day())).hash(&mut hasher);
            [entry.color.0, entry.color.1, entry.color.2, entry.tilt].map(f32::to_bits).hash(&mut hasher);
        }
        hasher.finish()
    }

    /// 한 줄짜리 글의 그림. 한 번 그린 것은 그대로 쓴다.
    fn label(&mut self, text: &str, size: f32, bold: bool, scale: f32) -> &Block {
        let icon = text.chars().next().is_some_and(|c| ('\u{E000}'..='\u{F8FF}').contains(&c));
        let pixels = (size * scale).round() as i32;
        self.labels
            .entry((text.to_string(), pixels, bold, icon))
            .or_insert_with(|| text::label(text, pixels, bold, icon))
    }

    /// 글의 너비.
    fn measure(&mut self, text: &str, size: f32, bold: bool, scale: f32) -> f32 {
        self.label(text, size, bold, scale).w as f32 / scale
    }

    /// 글을 찍고 그 너비를 돌려준다. `middle`이 글의 세로 한가운데다.
    #[allow(clippy::too_many_arguments)]
    fn put(
        &mut self,
        surface: &mut Surface,
        text: &str,
        size: f32,
        bold: bool,
        anchor: Anchor,
        middle: f32,
        color: Rgb,
    ) -> f32 {
        let scale = surface.scale;
        let block = self.label(text, size, bold, scale);
        let wide = block.w as f32 / scale;
        let center = match anchor {
            Anchor::Left(left) => left + wide / 2.0,
            Anchor::Center(center) => center,
        };
        surface.stamp(block, (center, middle), color, 1.0);
        wide
    }

    /// 글이 `room`보다 길면 뒤를 잘라 "…"를 붙인다.
    fn fit(&mut self, text: &str, size: f32, bold: bool, room: f32, scale: f32) -> String {
        let (pixels, room) = ((size * scale).round() as i32, (room * scale) as i32);
        let key = (text.to_string(), pixels, bold, room);
        if let Some(fitted) = self.fits.get(&key) {
            return fitted.clone();
        }
        let fitted = if text::width(text, pixels, bold) <= room {
            text.to_string()
        } else {
            let letters: Vec<char> = text.chars().collect();
            let cut = |count: usize| letters[..count].iter().collect::<String>().trim_end().to_string() + "…";
            // 들어가는 가장 긴 앞부분을 찾는다.
            let (mut least, mut most) = (0, letters.len());
            while least < most {
                let middle = (least + most).div_ceil(2);
                if text::width(&cut(middle), pixels, bold) <= room {
                    least = middle;
                } else {
                    most = middle - 1;
                }
            }
            cut(least)
        };
        self.fits.insert(key, fitted.clone());
        fitted
    }

    /// 누를 수 있는 곳으로 적어 둔다. `within`이 있으면 그 안에 든 만큼만 적는다.
    fn spot(&mut self, rect: Rect, spot: Spot, within: Option<Rect>) {
        let rect = match within {
            Some(edge) => (rect.0.max(edge.0), rect.1.max(edge.1), rect.2.min(edge.2), rect.3.min(edge.3)),
            None => rect,
        };
        if rect.0 < rect.2 && rect.1 < rect.3 {
            self.spots.push((rect, spot));
        }
    }

    /// 둥근 단추 하나를 그리고, 누를 곳으로 적어 둔다. `fill`은 비어 있는 단추의 바탕색이다.
    #[allow(clippy::too_many_arguments)]
    fn button(
        &mut self,
        surface: &mut Surface,
        rect: Rect,
        text: &str,
        spot: Spot,
        style: Style,
        fill: Rgb,
        within: Option<Rect>,
    ) {
        let radius = (rect.3 - rect.1) / 2.0;
        let hot = self.hot == Some(spot);
        if style == Style::Filled {
            surface.pill(rect, radius, if hot { DEEP } else { ROSE }, 1.0);
        } else {
            surface.frame(rect, radius, PINK, if hot { HOT } else { fill }, 1.5);
        }
        let color = match style {
            Style::Plain => DEEP,
            Style::Strong => ROSE,
            Style::Filled => WHITE,
        };
        let center = Anchor::Center((rect.0 + rect.2) / 2.0);
        self.put(surface, text, 12.0, style != Style::Plain, center, (rect.1 + rect.3) / 2.0, color);
        self.spot(rect, spot, within);
    }

    /// 창을 그린다. 누를 수 있는 곳도 이때 다시 적는다.
    pub fn paint(&mut self, surface: &mut Surface, view: &View, layout: &Layout) {
        if self.labels.len() > KEPT_LABELS || self.fits.len() > KEPT_LABELS {
            self.labels.clear();
            self.fits.clear();
        }
        self.spots.clear();
        surface.clip_to(None);
        surface.clear();
        let scale = surface.scale;
        let body = (MARGIN, MARGIN, MARGIN + BODY.0, MARGIN + BODY.1);
        surface.shadow(body, 16.0);
        surface.frame(body, 16.0, PINK, CREAM, 2.0);
        let (x, y, right) = (body.0, body.1, body.2);

        // 제목, 메모의 수, 닫기.
        let wide = self.put(surface, "메모", 14.5, true, Anchor::Left(x + 16.0), y + 22.0, ROSE);
        let count = format!("{}/{}", view.entries.len(), view.limit);
        self.put(surface, &count, 12.0, false, Anchor::Left(x + 24.0 + wide), y + 23.0, DEEP);
        let close = (right - 36.0, y + 10.0, right - 12.0, y + 34.0);
        if self.hot == Some(Spot::Close) {
            surface.disc((right - 24.0, y + 22.0), 12.0, HOT, 1.0);
        }
        self.put(surface, CLOSE, 11.0, false, Anchor::Center(right - 24.0), y + 22.0, ROSE);
        self.spot(close, Spot::Close, None);

        // 찾는 말을 적는 칸.
        let (tools, middle) = (y + TOOLS_TOP, y + TOOLS_TOP + 15.0);
        let order = (right - 14.0 - SORT_WIDE, tools, right - 14.0, tools + 30.0);
        let search = (x + 14.0, tools, order.0 - 8.0, tools + 30.0);
        surface.frame(search, 15.0, PINK, WHITE, 1.5);
        self.put(surface, SEARCH, 12.0, false, Anchor::Center(search.0 + 17.0), middle, PINK);
        let words = List::search_rect();
        if self.query.is_empty() {
            if !view.typing {
                self.put(surface, "찾기", 13.0, false, Anchor::Left(words.0), middle, FAINT);
            }
        } else {
            if !view.typing {
                let query = self.query.clone();
                let query = self.fit(&query, 13.0, false, words.2 - words.0, scale);
                self.put(surface, &query, 13.0, false, Anchor::Left(words.0), middle, INK);
            }
            let clear = (search.2 - 28.0, tools + 3.0, search.2 - 4.0, tools + 27.0);
            if self.hot == Some(Spot::Clear) {
                surface.disc((search.2 - 16.0, middle), 11.0, HOT, 1.0);
            }
            self.put(surface, CLOSE, 8.5, false, Anchor::Center(search.2 - 16.0), middle, DIM);
            self.spot(clear, Spot::Clear, None);
        }
        self.spot(search, Spot::Search, None);

        // 차례를 바꾸는 단추.
        surface.frame(order, 15.0, PINK, if self.hot == Some(Spot::Sort) { HOT } else { WHITE }, 1.5);
        self.put(surface, ORDER, 11.0, false, Anchor::Center(order.0 + 17.0), middle, DEEP);
        self.put(surface, self.sort.name(), 12.0, false, Anchor::Left(order.0 + 29.0), middle, DEEP);
        self.spot(order, Spot::Sort, None);

        // 고르는 단추들. 저마다 몇 개가 있는지 함께 적는다.
        let mut left = x + 14.0;
        for (filter, count) in Filter::ALL.into_iter().zip(layout.counts) {
            let chosen = self.filter == filter;
            let name = format!("{} {count}", filter.name());
            let wide = self.measure(&name, 12.0, chosen, scale) + 22.0;
            let rect = (left, y + CHIPS_TOP, left + wide, y + CHIPS_TOP + 26.0);
            let style = if chosen { Style::Filled } else { Style::Plain };
            self.button(surface, rect, &name, Spot::Filter(filter), style, WHITE, None);
            left += wide + 6.0;
        }

        // 메모 줄들. 보이는 곳을 벗어난 것은 잘라 그린다.
        let (rows_top, rows_low) = (y + ROWS_TOP, y + ROWS_TOP + ROWS_TALL);
        let window = (x + 2.0, rows_top, right - 2.0, rows_low);
        surface.clip_to(Some(window));
        for slot in &layout.slots {
            let top = rows_top + slot.top - self.scroll;
            if top + slot.tall >= rows_top && top <= rows_low {
                self.paint_row(surface, &view.entries[slot.at], top, slot.tall, window);
            }
        }
        if layout.slots.is_empty() {
            let center = Anchor::Center((x + right) / 2.0);
            if view.entries.is_empty() {
                self.put(surface, "아직 메모가 없어요", 13.0, false, center, rows_top + 150.0, DIM);
                self.put(surface, "아래의 \"+ 새 메모\"로 붙여 보세요", 12.0, false, center, rows_top + 174.0, FAINT);
            } else {
                self.put(surface, "찾는 메모가 없어요", 13.0, false, center, rows_top + 160.0, DIM);
            }
        }
        // 넘기는 막대.
        if let Some((top, long)) = self.thumb() {
            let track = (right - 9.0, rows_top + 4.0, right - 5.0, rows_low - 4.0);
            surface.pill(track, 2.0, LINE, 0.5);
            let held = self.hot == Some(Spot::Scroll) || self.scrolling.is_some();
            let thumb = (track.0, rows_top + top, track.2, rows_top + top + long);
            surface.pill(thumb, 2.0, if held { ROSE } else { PINK }, 0.9);
            self.spot((right - 13.0, rows_top, right - 1.0, rows_low), Spot::Scroll, None);
        }
        surface.clip_to(None);

        // 아래의 단추들. 위와는 점선으로 나눈다.
        let mut dash = x + 8.0;
        while dash < right - 8.0 {
            surface.pill((dash, rows_low, (dash + 5.0).min(right - 8.0), rows_low + 1.5), 0.75, LINE, 1.0);
            dash += 9.0;
        }
        let wide = (BODY.0 - 28.0 - 16.0) / 3.0;
        let feet = [("+ 새 메모", Foot::Add), ("다시 줄 세우기", Foot::Tidy), ("모두 앞으로", Foot::Raise)];
        for (place, (name, foot)) in feet.into_iter().enumerate() {
            let left = x + 14.0 + (wide + 8.0) * place as f32;
            let rect = (left, rows_low + 11.5, left + wide, rows_low + 41.5);
            self.button(surface, rect, name, Spot::Foot(foot), Style::Strong, CREAM, None);
        }
        // 제목 줄은 다른 것이 놓이지 않은 곳만 잡힌다. 그래서 맨 뒤에 적는다.
        self.spot((x, y, right, y + TOOLS_TOP), Spot::Head, None);
    }

    /// 메모 한 줄을 그린다. `top`은 창에서의 위 끝이다.
    fn paint_row(&mut self, surface: &mut Surface, entry: &Entry, top: f32, tall: f32, window: Rect) {
        let scale = surface.scale;
        let (left, right) = (MARGIN + 8.0, MARGIN + BODY.0 - 14.0);
        let rect = (left, top + 1.0, right, top + tall - 1.0);
        let chosen = self.selected == Some(entry.id);
        if chosen {
            surface.frame(rect, 12.0, PINK, HOT, 1.5);
        } else if self.hot == Some(Spot::Row(entry.id)) {
            surface.pill(rect, 12.0, HOVER, 1.0);
        }
        let base = if entry.is_tall() { ROW_TWO } else { ROW_ONE };
        let middle = top + base / 2.0;
        swatch(surface, (left + 20.0, middle), entry.color, entry.tilt);

        // 알림 시각은 오른쪽 끝에 적는다.
        let mut text_right = right - 10.0;
        if let Some(label) = &entry.chip {
            let wide = self.measure(label, 11.0, true, scale) + 25.0;
            let chip = (right - 9.0 - wide, middle - 9.5, right - 9.0, middle + 9.5);
            surface.frame(chip, 9.5, PINK, CREAM, 1.5);
            self.put(surface, BELL, 10.0, false, Anchor::Center(chip.0 + 11.0), middle, ROSE);
            self.put(surface, label, 11.0, true, Anchor::Left(chip.0 + 18.0), middle, ROSE);
            text_right = chip.0 - 8.0;
        }

        // 첫 줄. 그 아래에는 꼬리표(항상 위, 맡김)와 나머지 글이 온다.
        let text_left = left + 44.0;
        let (title, color) = if entry.first.is_empty() { ("(빈 메모)", FAINT) } else { (entry.first.as_str(), INK) };
        let title = self.fit(title, 13.0, false, text_right - text_left, scale);
        let first = if entry.is_tall() { top + 16.0 } else { middle };
        self.put(surface, &title, 13.0, false, Anchor::Left(text_left), first, color);
        if entry.is_tall() {
            let (mut at, second) = (text_left, top + 35.0);
            for (name, on) in [("항상 위", entry.top), ("맡김", entry.held)] {
                if on {
                    let wide = self.measure(name, 10.5, true, scale) + 10.0;
                    surface.pill((at, second - 8.0, at + wide, second + 8.0), 4.0, TAG, 1.0);
                    self.put(surface, name, 10.5, true, Anchor::Center(at + wide / 2.0), second, DEEP);
                    at += wide + 5.0;
                }
            }
            if !entry.rest.is_empty() && text_right - at > 20.0 {
                let rest = self.fit(&entry.rest, 11.5, false, text_right - at, scale);
                self.put(surface, &rest, 11.5, false, Anchor::Left(at), second, DIM);
            }
        }

        // 고른 줄에는 단추가 나온다. 지울지 묻는 중이면 그 물음이 나온다.
        if chosen {
            let (up, low) = (top + base - 1.0, top + base + 25.0);
            let edge = Some(window);
            if self.asking {
                self.put(surface, "이 메모를 지울까요?", 12.5, true, Anchor::Left(left + 12.0), (up + low) / 2.0, ROSE);
                let mut end = right - 9.0;
                for (name, act, style) in [("아니요", Act::Keep, Style::Plain), ("지우기", Act::Really, Style::Filled)]
                {
                    let wide = self.measure(name, 12.0, style == Style::Filled, scale) + 24.0;
                    self.button(surface, (end - wide, up, end, low), name, Spot::Act(act), style, WHITE, edge);
                    end -= wide + 6.0;
                }
            } else {
                let mut at = left + 9.0;
                let acts = [
                    ("찾아가기", Act::Find, true),
                    ("항상 위", Act::Top, entry.top),
                    ("맡기기", Act::Hold, entry.held),
                    ("지우기", Act::Delete, false),
                ];
                for (name, act, on) in acts {
                    let style = if on { Style::Filled } else { Style::Plain };
                    let wide = self.measure(name, 12.0, on, scale) + 24.0;
                    self.button(surface, (at, up, at + wide, low), name, Spot::Act(act), style, WHITE, edge);
                    at += wide + 6.0;
                }
            }
        }
        self.spot(rect, Spot::Row(entry.id), Some(window));
    }
}

/// 색 조각: 그 메모의 포스트잇을 작게 그린 것. 종이 색과 기울기가 같고 오른쪽 아래 귀퉁이가 접혀 있다.
fn swatch(surface: &mut Surface, center: (f32, f32), color: Rgb, tilt: f32) {
    /// 종이의 한 변의 절반과, 접힌 귀퉁이의 크기.
    const HALF: f32 = 11.5;
    const FOLD: f32 = 7.0;
    let scale = surface.scale;
    let (sin, cos) = tilt.sin_cos();
    let (hue, saturation, value) = to_hsv(color);
    // 종이 아래쪽은 조금 더 진하다. 포스트잇을 그릴 때와 같은 색이다.
    let low = from_hsv(hue, (saturation * 1.25 + 0.03).min(1.0), value * 0.975);
    let folded = mix(color, WHITE, 0.55);
    let reach = HALF * 1.5 + 4.0;
    surface.each((center.0 - reach, center.1 - reach, center.0 + reach, center.1 + reach), |u, v, pixel| {
        // 기울인 종이를 기준으로 한 자리.
        let (dx, dy) = (u - center.0, v - center.1);
        let (a, b) = (dx * cos + dy * sin, dy * cos - dx * sin);
        // 귀퉁이를 비스듬히 잘라 낸 네모까지의 거리.
        let sheet = |a: f32, b: f32| {
            round_box(a, b, (-HALF, -HALF, HALF, HALF), 1.5).max((a + b - (2.0 * HALF - FOLD)) * FRAC_1_SQRT_2)
        };
        pixel.over(SHADOW, 0.24 * (0.5 - sheet(a, b - 1.5) / 3.0).clamp(0.0, 1.0));
        let shade = (((b + HALF) / (2.0 * HALF) - 0.6) / 0.4).clamp(0.0, 1.0);
        pixel.over(mix(color, low, shade), (0.5 - sheet(a, b) * scale).clamp(0.0, 1.0));
        // 접혀 올라온 귀퉁이는 세모다.
        let corner = HALF - FOLD;
        let flap = (corner - a).max(corner - b).max((a + b - (2.0 * HALF - FOLD)) * FRAC_1_SQRT_2);
        pixel.over(folded, (0.5 - flap * scale).clamp(0.0, 1.0));
    });
}

// ---------------------------------------------------------------------------------------------------------------
// 목록 창을 포스트잇과 잇는 곳.
// ---------------------------------------------------------------------------------------------------------------

/// 찾아간 포스트잇이 반짝이는 시간(초)과, 그동안 커지는 정도.
pub const FOUND_TIME: f32 = 0.9;
pub const FOUND_SIZE: f32 = 0.13;

impl Postits {
    /// 목록에 보여 줄 메모들. 새로 만든 것이 앞에 온다. 지워지는 중인 것은 뺀다.
    fn entries(&self, memos: &Memos) -> Vec<Entry> {
        let today = self.wall.day();
        let entry = |memo: &crate::memos::Memo| {
            let note = &self.notes[self.find_id(memo.id)?];
            let mut lines = note.text.lines().map(str::trim).filter(|line| !line.is_empty());
            let first = lines.next().unwrap_or_default().to_string();
            let chip = note.chip(today);
            Some(Entry {
                id: memo.id,
                first,
                rest: lines.collect::<Vec<_>>().join(" "),
                search: note.text.to_lowercase(),
                next: note.next.filter(|_| chip.is_some()),
                chip,
                top: note.top,
                held: memo.held,
                color: note.color,
                tilt: note.rest,
            })
        };
        memos.list().iter().filter_map(entry).collect()
    }

    /// 메모 목록을 `near`(화면 좌표) 가까이에 띄운다. 이미 떠 있으면 앞으로 가져온다.
    /// 닫았다가 다시 열면 닫을 때 있던 자리에 뜬다.
    pub fn open_list(&mut self, near: (i32, i32)) {
        if let Some(list) = &self.list {
            window::show_active(list.hwnd);
            return;
        }
        window::start();
        // 전에 있던 자리가 아직 화면 안이면 그 자리에 띄운다.
        let kept = self.list_spot.filter(|&spot| window::work_area(spot, false).is_some());
        let anchor = kept.unwrap_or(near);
        let scale = self.scale_at((anchor.0 as f32, anchor.1 as f32));
        let mut list = List::new(std::ptr::null_mut(), anchor, scale);
        let (wide, tall) = list.size_px();
        if kept.is_none() {
            let work = window::work_area(near, true).unwrap_or((0, 0, 1920, 1040));
            let fit = |value: i32, least: i32, most: i32| value.clamp(least, most.max(least));
            list.at = (
                fit(near.0 - wide / 2, work.0, work.2 - wide),
                fit(near.1 - (60.0 * scale) as i32, work.1, work.3 - tall),
            );
        }
        list.hwnd = window::create_active(list.at, (wide, tall));
        // 그림은 이번 바퀴의 끝에 그려 넘긴다. 그때까지는 창에 아무것도 없어서 보이지 않는다.
        window::show_active(list.hwnd);
        self.list = Some(list);
        self.wake();
    }

    /// 메모 목록을 닫는다. 찾는 말과 고른 것은 남기지 않는다.
    pub fn close_list(&mut self) {
        self.search = None;
        if let Some(list) = self.list.take() {
            self.list_spot = Some(list.at);
            window::destroy(list.hwnd);
        }
    }

    /// 찾는 말을 적는 칸을 올린다.
    fn open_search(&mut self, memos: &mut Memos) {
        if self.search.is_some() {
            return;
        }
        // 글을 적는 칸은 한 번에 하나만 둔다.
        self.close_editor(memos);
        let Some(list) = &self.list else { return };
        let rect = List::search_rect();
        let px = |units: f32| (units * list.scale).round() as i32;
        let screen = (list.at.0 + px(rect.0), list.at.1 + px(rect.1), px(rect.2 - rect.0), px(rect.3 - rect.1));
        let font = text::clear_font(text::FACE, px(13.0), false);
        // 흰 바탕에 메모의 글과 같은 색의 글자.
        let colors = (0x00FF_FFFF, 0x003D_3A4A);
        self.search = Some(Editor::open_line(list.hwnd, screen, &list.query, colors, font, QUERY_LIMIT));
    }

    /// 찾는 말을 적는 칸을 거둔다. 적은 말은 그대로 남는다.
    pub(super) fn close_search(&mut self) {
        if let Some(search) = self.search.take() {
            let query: String = search.text().chars().take(QUERY_LIMIT).collect();
            if let Some(list) = &mut self.list {
                list.query = query;
            }
            self.wake();
        }
    }

    /// 찾는 말을 적는 칸에서 누른 키를 먼저 본다. Enter는 적기를 마치고, Esc는 적은 말을 지우거나(이미 비어
    /// 있으면) 적기를 마친다. 그렇게 처리했으면 `true`를 돌려준다.
    pub(super) fn search_key(&mut self, edit: HWND, key: u32, control: bool) -> bool {
        let Some(search) = self.search.as_ref().filter(|search| search.edit() == edit) else { return false };
        match key {
            window::KEY_ENTER => self.close_search(),
            KEY_ESCAPE if search.text().is_empty() => self.close_search(),
            KEY_ESCAPE => search.set_text(""),
            key if control && key == 'A' as u32 => search.select_all(),
            _ => return false,
        }
        true
    }

    /// 그 메모의 포스트잇을 다른 창들 앞으로 가져오고 두 번 반짝이게 한다. 자리는 옮기지 않는다.
    pub fn find_note(&mut self, id: u64) {
        let Some(at) = self.find_id(id) else { return };
        let quiet = self.quiet;
        let note = &mut self.notes[at];
        // 확인용으로 도는 중이면 쓰던 창들 앞으로 가져오지 않는다.
        if !quiet {
            window::raise(note.hwnd);
        }
        if note.phase == Phase::Idle {
            note.found = Some(0.0);
        }
        self.wake();
    }

    /// 목록 창에서 일어난 일이면 처리하고 `true`를 돌려준다.
    pub(super) fn list_event(&mut self, event: Event, memos: &mut Memos) -> bool {
        // 찾는 말을 적는 칸에서 온 일.
        let search = self.search.as_ref().map(Editor::host);
        match event {
            Event::EditDone { host } if search == Some(host) => {
                self.close_search();
                return true;
            }
            Event::EditChanged { host } if search == Some(host) => {
                let query = self.search.as_ref().map(Editor::text).unwrap_or_default();
                if let Some(list) = &mut self.list {
                    (list.query, list.scroll) = (query.chars().take(QUERY_LIMIT).collect(), 0.0);
                }
                return true;
            }
            _ => {}
        }
        let Some(list) = &mut self.list else { return false };
        let (hwnd, scale) = (list.hwnd, list.scale);
        let local = |x: i32, y: i32| (x as f32 / scale, y as f32 / scale);
        match event {
            Event::Press { hwnd: on, x, y } if on == hwnd => {
                let (u, v) = local(x, y);
                match list.hit(u, v) {
                    Some(Spot::Head) => {
                        let cursor = win::cursor_position();
                        list.moving = Some((cursor.0 - list.at.0, cursor.1 - list.at.1));
                    }
                    Some(Spot::Scroll) => list.grab_scroll(v),
                    Some(spot) => {
                        window::stop_pressing();
                        self.list_act(spot, memos);
                    }
                    // 아무것도 없는 곳을 누르면 고른 것을 푼다.
                    None => {
                        window::stop_pressing();
                        list.select(None);
                    }
                }
            }
            Event::Move { hwnd: on, x, y } if on == hwnd => {
                let (u, v) = local(x, y);
                if let Some(offset) = list.moving {
                    let cursor = win::cursor_position();
                    list.at = (cursor.0 - offset.0, cursor.1 - offset.1);
                    window::move_to(hwnd, list.at);
                } else if list.scrolling.is_some() {
                    list.drag_scroll(v);
                } else {
                    list.hot = list.hit(u, v);
                    window::set_cursor(match list.hot {
                        None | Some(Spot::Head | Spot::Scroll | Spot::Search) => Cursor::Arrow,
                        Some(_) => Cursor::Hand,
                    });
                }
            }
            Event::Lift { hwnd: on } if on == hwnd => {
                list.scrolling = None;
                if list.moving.take().is_some() {
                    // 배율이 다른 모니터에 놓았으면 그 배율로 다시 그린다.
                    let (wide, tall) = list.size_px();
                    let middle = ((list.at.0 + wide / 2) as f32, (list.at.1 + tall / 2) as f32);
                    self.list_spot = Some(list.at);
                    let scale = self.scale_at(middle);
                    if let Some(list) = &mut self.list {
                        list.scale = scale;
                    }
                }
            }
            Event::Leave { hwnd: on } if on == hwnd => list.hot = None,
            Event::Double { hwnd: on, x, y } if on == hwnd => {
                let (u, v) = local(x, y);
                match list.hit(u, v) {
                    // 줄을 두 번 누르면 바로 찾아간다.
                    Some(Spot::Row(id)) => {
                        list.select(Some(id));
                        self.find_note(id);
                    }
                    Some(Spot::Head | Spot::Scroll) | None => {}
                    // 단추를 빠르게 두 번 누른 것은 두 번 누른 것이다.
                    Some(spot) => self.list_act(spot, memos),
                }
            }
            Event::Wheel { hwnd: on, up, .. } if on == hwnd => list.scroll += if up { -WHEEL } else { WHEEL },
            Event::Key { hwnd: on, key } if on == hwnd => {
                if key == KEY_ESCAPE {
                    self.close_list();
                }
            }
            Event::Menu { hwnd: on } if on == hwnd => {}
            _ => return false,
        }
        true
    }

    /// 목록 창에서 누른 것을 한다.
    fn list_act(&mut self, spot: Spot, memos: &mut Memos) {
        let Some(list) = &mut self.list else { return };
        match spot {
            Spot::Head | Spot::Scroll => {}
            Spot::Close => self.close_list(),
            Spot::Search => self.open_search(memos),
            Spot::Clear => {
                (list.scroll, list.query) = (0.0, String::new());
                if let Some(search) = &self.search {
                    search.set_text("");
                }
            }
            Spot::Sort => (list.sort, list.scroll) = (list.sort.next(), 0.0),
            Spot::Filter(filter) => (list.filter, list.scroll) = (filter, 0.0),
            Spot::Row(id) => list.select(Some(id)),
            Spot::Act(act) => {
                let Some(id) = list.selected else { return };
                match act {
                    Act::Find => self.find_note(id),
                    Act::Top => {
                        if let Some(at) = self.find_id(id) {
                            self.act(at, 2, memos);
                        }
                    }
                    Act::Hold => {
                        if let Some(memo) = memos.get_mut(id) {
                            memo.held = !memo.held;
                            self.outcomes.push(Outcome::Changed);
                        }
                    }
                    Act::Delete => {
                        let Some(at) = self.find_id(id) else { return };
                        // 적던 글이 있으면 먼저 옮겨 적는다. 빈 메모인지는 그 뒤에 본다.
                        if self.editor.as_ref().is_some_and(|(editing, _)| *editing == self.notes[at].hwnd) {
                            self.close_editor(memos);
                        }
                        // 아무것도 적지 않은 메모는 묻지 않고 지운다.
                        if self.notes[at].text.trim().is_empty() {
                            self.remove(memos, id);
                        } else if let Some(list) = &mut self.list {
                            (list.asking, list.reveal) = (true, true);
                        }
                    }
                    Act::Really => self.remove(memos, id),
                    Act::Keep => list.asking = false,
                }
            }
            Spot::Foot(Foot::Add) => {
                // 새 포스트잇은 목록 창의 왼쪽에 붙인다. 자리가 없으면 오른쪽에 붙인다.
                let (wide, _) = list.size_px();
                let half = ((draw::PAPER.0 / 2.0 + 30.0) * list.scale) as i32;
                let work = window::work_area(list.at, true).unwrap_or((0, 0, 1920, 1040));
                let x = if list.at.0 - 2 * half >= work.0 { list.at.0 - half } else { list.at.0 + wide + half };
                self.outcomes.push(Outcome::Create { x, y: list.at.1 + (70.0 * list.scale) as i32 });
            }
            Spot::Foot(Foot::Tidy) => self.tidy(),
            Spot::Foot(Foot::Raise) => self.show_all(),
        }
    }

    /// 목록 창을 공용 버퍼에 그린다. 그린 그림의 크기(px)를 돌려준다.
    pub(super) fn draw_list(&mut self, memos: &Memos) -> Option<(i32, i32)> {
        self.list.as_ref()?;
        self.ready();
        let entries = self.entries(memos);
        let view = View { entries: &entries, limit: COUNT_LIMIT, typing: self.search.is_some() };
        let (list, tools) = (self.list.as_mut()?, self.tools.as_mut()?);
        let layout = list.arrange(&entries);
        let size = list.size_px();
        tools.canvas.fit(size.0, size.1);
        let stride = tools.canvas.width as usize;
        let mut surface = Surface::new(tools.canvas.pixels(), stride, size, list.scale);
        list.paint(&mut surface, &view, &layout);
        Some(size)
    }

    /// 목록 창에 보일 것이 달라졌으면 다시 그려 화면에 넘긴다. 한 바퀴의 끝에 부른다.
    ///
    /// 메모는 포스트잇에서도, 메뉴에서도, 알림이 울려서도 바뀐다. 바뀐 곳마다 알리게 하는 대신, 그려질 것의
    /// 지문을 견줘서 달라졌을 때만 그린다. 목록 창이 닫혀 있으면 아무 일도 하지 않는다.
    pub(super) fn sync_list(&mut self, memos: &Memos, now: Instant) {
        if self.list.is_none() {
            return;
        }
        let entries = self.entries(memos);
        let view = View { entries: &entries, limit: COUNT_LIMIT, typing: self.search.is_some() };
        let Some(list) = &mut self.list else { return };
        list.arrange(&entries);
        let print = list.print(&view);
        if list.painted == Some(print) {
            return;
        }
        // 방금 그렸으면 조금 기다렸다 그린다.
        if list.painted.is_some() && now < list.painted_at + REDRAW_EVERY {
            return self.wake();
        }
        (list.painted, list.painted_at) = (Some(print), now);
        if let (Some(size), Some(list), Some(tools)) = (self.draw_list(memos), &self.list, &self.tools) {
            tools.canvas.present(list.hwnd, list.at, size);
        }
        self.rest();
    }

    /// 모니터의 화면 배율이 바뀌었거나 창이 화면 밖에 남았으면 목록 창을 맞춘다.
    pub(super) fn settle_list(&mut self) {
        let Some(list) = &self.list else { return };
        let (wide, tall) = list.size_px();
        let middle = (list.at.0 + wide / 2, list.at.1 + tall / 2);
        let mut at = list.at;
        if window::work_area(middle, false).is_none()
            && let Some(work) = window::work_area(middle, true)
        {
            at = (at.0.clamp(work.0, (work.2 - wide).max(work.0)), at.1.clamp(work.1, (work.3 - tall).max(work.1)));
        }
        let scale = self.scale_at(((at.0 + wide / 2) as f32, (at.1 + tall / 2) as f32));
        if let Some(list) = &mut self.list
            && (list.at != at || list.scale != scale)
        {
            (list.at, list.scale, list.painted) = (at, scale, None);
            self.wake();
        }
    }

    /// 확인용: 그림을 뽑을 때 쓰는, 목록 창의 자리와 크기(px).
    pub(super) fn list_frame(&self) -> Option<((i32, i32), (i32, i32))> {
        self.list.as_ref().map(|list| (list.at, list.size_px()))
    }

    /// 확인용: 사람이 목록 창에서 하는 일을 흉내 낸다. 자리는 배율 100%일 때의 창 안 좌표다.
    pub(super) fn list_script(&mut self, step: &ListStep, memos: &mut Memos) {
        match step {
            ListStep::Open(at) => return self.open_list(at.unwrap_or((500, 300))),
            ListStep::Close => return self.close_list(),
            _ => {}
        }
        let Some(list) = &mut self.list else { return };
        let (hwnd, scale) = (list.hwnd, list.scale);
        let px = |units: i32| (units as f32 * scale).round() as i32;
        match step {
            ListStep::Open(_) | ListStep::Close => {}
            ListStep::Filter(name) => {
                let filter = Filter::ALL
                    [["all", "alarm", "top", "held"].iter().position(|&known| known == name.as_str()).unwrap_or(0)];
                self.list_act(Spot::Filter(filter), memos);
            }
            ListStep::Sort => self.list_act(Spot::Sort, memos),
            ListStep::Find(words) => (list.query, list.scroll) = (words.chars().take(QUERY_LIMIT).collect(), 0.0),
            ListStep::Select(id) => self.list_act(Spot::Row(*id), memos),
            ListStep::Press(name) => {
                let spot = match name.as_str() {
                    "find" => Spot::Act(Act::Find),
                    "top" => Spot::Act(Act::Top),
                    "hold" => Spot::Act(Act::Hold),
                    "delete" => Spot::Act(Act::Delete),
                    "yes" => Spot::Act(Act::Really),
                    "no" => Spot::Act(Act::Keep),
                    "add" => Spot::Foot(Foot::Add),
                    "tidy" => Spot::Foot(Foot::Tidy),
                    "raise" => Spot::Foot(Foot::Raise),
                    "search" => Spot::Search,
                    "clear" => Spot::Clear,
                    _ => Spot::Close,
                };
                self.list_act(spot, memos);
            }
            ListStep::Wheel(steps) => {
                for _ in 0..steps.unsigned_abs() {
                    self.list_event(Event::Wheel { hwnd, x: 0, y: 0, up: *steps < 0 }, memos);
                }
            }
            ListStep::Click(x, y) => {
                self.list_event(Event::Press { hwnd, x: px(*x), y: px(*y) }, memos);
                self.list_event(Event::Lift { hwnd }, memos);
            }
            ListStep::Double(x, y) => {
                self.list_event(Event::Double { hwnd, x: px(*x), y: px(*y) }, memos);
            }
            ListStep::Hover(x, y) => {
                self.list_event(Event::Move { hwnd, x: px(*x), y: px(*y) }, memos);
            }
            ListStep::Esc => {
                self.list_event(Event::Key { hwnd, key: KEY_ESCAPE }, memos);
            }
        }
    }

    /// 확인용: 목록 창이 지금 어떤지를 적은 한 줄. 닫혀 있으면 `None`이다.
    pub(super) fn describe_list(&self, memos: &Memos) -> Option<String> {
        let list = self.list.as_ref()?;
        let entries = self.entries(memos);
        let ids: Vec<u64> =
            shown(&entries, &list.query, list.filter, list.sort).into_iter().map(|at| entries[at].id).collect();
        let chosen = match (list.selected, list.asking) {
            (None, _) => "없음".to_string(),
            (Some(id), false) => id.to_string(),
            (Some(id), true) => format!("{id}(지울지 묻는 중)"),
        };
        Some(format!(
            "메모 목록: 자리 {:?}, 배율 {:.0}%, 고르기 {}, 차례 {}, 찾는 말 \"{}\"{}, 보이는 메모 {ids:?}, 고른 메모 {chosen}, 넘긴 높이 {:.0}, 마우스 {:?}",
            list.at,
            list.scale * 100.0,
            list.filter.name(),
            list.sort.name(),
            list.query,
            if self.search.is_some() { "(적는 중)" } else { "" },
            list.scroll,
            list.hot,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u64, text: &str) -> Entry {
        let mut lines = text.lines();
        Entry {
            id,
            first: lines.next().unwrap_or_default().to_string(),
            rest: lines.collect::<Vec<_>>().join(" "),
            search: text.to_lowercase(),
            chip: None,
            next: None,
            top: false,
            held: false,
            color: COLORS[0],
            tilt: 0.05,
        }
    }

    /// 새로 만든 것이 앞에 오는 견본. 4번은 내일 아침, 2번은 오늘 저녁에 알린다.
    fn sample() -> Vec<Entry> {
        let alarm =
            |entry: Entry, label: &str, next: When| Entry { chip: Some(label.to_string()), next: Some(next), ..entry };
        vec![
            Entry { color: COLORS[3], ..entry(5, "") },
            alarm(entry(4, "택배 찾아오기\n경비실 6시까지"), "내일 오전 9:00", When::new(2026, 11, 12, 9, 0).unwrap()),
            Entry { held: true, color: COLORS[4], ..entry(3, "주말에 볼 것\nMovie 두 편") },
            Entry {
                top: true,
                color: COLORS[3],
                ..alarm(entry(2, "꽃핀 방송 보러 가기"), "오늘 오후 7:00", When::new(2026, 11, 11, 19, 0).unwrap())
            },
            Entry { color: rgb(0x123456), ..entry(1, "우유 사기") },
        ]
    }

    fn ids(entries: &[Entry], shown: Vec<usize>) -> Vec<u64> {
        shown.into_iter().map(|at| entries[at].id).collect()
    }

    /// 배율 100%로 그림 버퍼에 그린다.
    fn draw(list: &mut List, entries: &[Entry], pixels: &mut [u32], typing: bool) -> Layout {
        let (wide, tall) = list.size_px();
        let layout = list.arrange(entries);
        let mut surface = Surface::new(pixels, wide as usize, (wide, tall), 1.0);
        list.paint(&mut surface, &View { entries, limit: 50, typing }, &layout);
        layout
    }

    /// 지금 누를 수 있는 것들 가운데 `keep`에 드는 것.
    fn spots(list: &List, keep: fn(&Spot) -> bool) -> Vec<Spot> {
        list.spots.iter().map(|&(_, spot)| spot).filter(keep).collect()
    }

    #[test]
    fn memos_are_chosen_by_kind_and_by_words() {
        let entries = sample();
        let pick = |query: &str, filter: Filter| ids(&entries, shown(&entries, query, filter, Sort::New));
        assert_eq!(pick("", Filter::All), [5, 4, 3, 2, 1]);
        assert_eq!(pick("", Filter::Alarm), [4, 2]);
        assert_eq!(pick("", Filter::Top), [2]);
        assert_eq!(pick("", Filter::Held), [3]);
        // 글 어디에 있든 찾고, 영어는 대소문자를 가리지 않는다. 앞뒤의 빈칸은 뺀다.
        assert_eq!(pick("경비실", Filter::All), [4]);
        assert_eq!(pick(" movie ", Filter::All), [3]);
        assert_eq!(pick("기", Filter::All), [4, 2, 1]);
        assert_eq!(pick("기", Filter::Alarm), [4, 2]);
        assert_eq!(pick("없는 말", Filter::All), [0u64; 0]);
    }

    #[test]
    fn memos_are_put_in_the_chosen_order() {
        let entries = sample();
        let order = |sort: Sort| ids(&entries, shown(&entries, "", Filter::All, sort));
        assert_eq!(order(Sort::New), [5, 4, 3, 2, 1]);
        // 알림이 빠른 것부터, 알림이 없는 것은 그 뒤에 새로 만든 것부터.
        assert_eq!(order(Sort::Due), [2, 4, 5, 3, 1]);
        // 기본 색은 판에 놓인 차례대로(하늘 5·2, 라벤더 3, 버터 노랑 4가 맨 앞), 직접 고른 색은 맨 뒤.
        assert_eq!(order(Sort::Color), [4, 5, 2, 3, 1]);
        assert_eq!([Sort::Due.next(), Sort::New.next(), Sort::Color.next()], [Sort::New, Sort::Color, Sort::Due]);
    }

    #[test]
    fn rows_stack_and_the_chosen_row_is_taller() {
        let entries = sample();
        let all: Vec<usize> = (0..entries.len()).collect();
        let (rows, content) = slots(&entries, &all, None);
        // 빈 메모와 한 줄짜리 메모는 낮고, 둘째 줄에 보여 줄 것이 있는 메모는 높다.
        let tall: Vec<f32> = rows.iter().map(|slot| slot.tall).collect();
        assert_eq!(tall, [ROW_ONE, ROW_TWO, ROW_TWO, ROW_TWO, ROW_ONE]);
        assert_eq!(rows[0].top, 2.0);
        for pair in rows.windows(2) {
            assert_eq!(pair[1].top, pair[0].top + pair[0].tall);
        }
        assert_eq!(content, 2.0 + 2.0 * ROW_ONE + 3.0 * ROW_TWO + 6.0);
        let (chosen, grown) = slots(&entries, &all, Some(4));
        assert_eq!(chosen[1].tall, ROW_TWO + ROW_ACTS);
        assert_eq!(grown, content + ROW_ACTS);
    }

    #[test]
    fn the_list_scrolls_within_its_rows_and_shows_the_chosen_one() {
        let entries: Vec<Entry> = (1..=30).rev().map(|id| entry(id, "한 줄")).collect();
        let mut list = List::new(std::ptr::null_mut(), (0, 0), 1.0);
        list.sort = Sort::New;
        list.arrange(&entries);
        let over = 2.0 + 30.0 * ROW_ONE + 6.0 - ROWS_TALL;
        assert_eq!(list.content - ROWS_TALL, over);
        // 끝을 넘겨 넘기지 못한다.
        list.scroll = 10_000.0;
        list.arrange(&entries);
        assert_eq!(list.scroll, over);
        list.scroll = -50.0;
        list.arrange(&entries);
        assert_eq!(list.scroll, 0.0);
        // 아래쪽 줄을 고르면 단추 줄까지 보이게 넘긴다.
        list.select(Some(15));
        let layout = list.arrange(&entries);
        let slot = layout.slots.iter().find(|slot| entries[slot.at].id == 15).unwrap();
        assert_eq!(slot.tall, ROW_ONE + ROW_ACTS);
        assert!(slot.top >= list.scroll && slot.top + slot.tall <= list.scroll + ROWS_TALL);
        // 막대는 넘긴 만큼 내려가고, 끌면 그만큼 넘어간다.
        let (top, long) = list.thumb().unwrap();
        assert!(top > 4.0 && top + long < ROWS_TALL - 4.0);
        list.grab_scroll(MARGIN + ROWS_TOP + ROWS_TALL - 6.0);
        assert_eq!(list.scroll, list.content - ROWS_TALL);
        list.drag_scroll(MARGIN + ROWS_TOP);
        assert_eq!(list.scroll, 0.0);
        // 찾는 말에 걸리지 않게 되면 고른 것이 풀린다.
        list.query = "없는 말".to_string();
        list.arrange(&entries);
        assert_eq!((list.selected, list.asking), (None, false));
        assert!(list.thumb().is_none());
    }

    #[test]
    fn what_is_drawn_can_be_pressed() {
        text::load_fonts();
        let entries = sample();
        let mut list = List::new(std::ptr::null_mut(), (0, 0), 1.0);
        list.selected = Some(4);
        let (wide, tall) = list.size_px();
        let mut pixels = vec![0u32; (wide * tall) as usize];
        let layout = draw(&mut list, &entries, &mut pixels, false);
        // 차례는 알림 빠른 순: 2, 4, 5, 3, 1.
        let row = |id: u64| layout.slots.iter().find(|slot| entries[slot.at].id == id).unwrap();
        let (x, y) = (MARGIN, MARGIN + ROWS_TOP);
        assert_eq!(list.hit(x + 100.0, y + row(2).top + 10.0), Some(Spot::Row(2)));
        assert_eq!(list.hit(x + 100.0, y + row(4).top + 10.0), Some(Spot::Row(4)));
        // 고른 줄의 아래쪽에는 단추가 있다. 맨 앞이 찾아가기다.
        assert_eq!(list.hit(x + 30.0, y + row(4).top + ROW_TWO + 12.0), Some(Spot::Act(Act::Find)));
        assert_eq!(list.hit(x + 30.0, y + row(1).top + 10.0), Some(Spot::Row(1)));
        assert_eq!(list.hit(x + BODY.0 - 24.0, MARGIN + 22.0), Some(Spot::Close));
        assert_eq!(list.hit(x + 150.0, MARGIN + 22.0), Some(Spot::Head));
        assert_eq!(list.hit(x + 60.0, MARGIN + TOOLS_TOP + 15.0), Some(Spot::Search));
        assert_eq!(list.hit(x + BODY.0 - 60.0, MARGIN + TOOLS_TOP + 15.0), Some(Spot::Sort));
        assert_eq!(list.hit(x + 30.0, MARGIN + CHIPS_TOP + 13.0), Some(Spot::Filter(Filter::All)));
        assert_eq!(list.hit(x + 60.0, MARGIN + ROWS_TOP + ROWS_TALL + 26.0), Some(Spot::Foot(Foot::Add)));
        assert_eq!(list.hit(x + 174.0, MARGIN + ROWS_TOP + ROWS_TALL + 26.0), Some(Spot::Foot(Foot::Tidy)));
        assert_eq!(list.hit(x + 290.0, MARGIN + ROWS_TOP + ROWS_TALL + 26.0), Some(Spot::Foot(Foot::Raise)));
        // 줄이 다 들어가므로 넘기는 막대는 없다. 찾는 말이 없으면 지우는 단추도 없다.
        assert_eq!(spots(&list, |spot| matches!(spot, Spot::Scroll | Spot::Clear)), []);
        // 창 안쪽은 불투명하고, 둘레의 여백 끝은 비어 있다.
        let alpha = |x: f32, y: f32| pixels[y as usize * wide as usize + x as usize] >> 24;
        assert_eq!(alpha(MARGIN + 100.0, MARGIN + 300.0), 255);
        assert_eq!(alpha(0.0, 0.0), 0);

        // 지울지 묻는 중이면 단추가 "지우기"와 "아니요"로 바뀐다.
        list.asking = true;
        let layout = draw(&mut list, &entries, &mut pixels, false);
        assert_eq!(spots(&list, |spot| matches!(spot, Spot::Act(_))), [Spot::Act(Act::Keep), Spot::Act(Act::Really)]);
        let low = y + layout.slots[1].top + ROW_TWO + 12.0;
        assert_eq!(list.hit(x + BODY.0 - 40.0, low), Some(Spot::Act(Act::Keep)));

        // 찾는 말이 있으면 지우는 단추가 생기고, 걸리는 메모만 줄로 남는다.
        list.query = "우유".to_string();
        draw(&mut list, &entries, &mut pixels, true);
        assert_eq!(spots(&list, |spot| matches!(spot, Spot::Row(_))), [Spot::Row(1)]);
        assert_eq!(spots(&list, |spot| *spot == Spot::Clear), [Spot::Clear]);
    }

    #[test]
    fn rows_scrolled_out_of_sight_cannot_be_pressed() {
        text::load_fonts();
        let entries: Vec<Entry> = (1..=30).rev().map(|id| entry(id, "한 줄")).collect();
        let mut list = List::new(std::ptr::null_mut(), (0, 0), 1.0);
        list.sort = Sort::New;
        list.scroll = ROW_ONE * 3.0 + 10.0;
        let (wide, tall) = list.size_px();
        let mut pixels = vec![0u32; (wide * tall) as usize];
        draw(&mut list, &entries, &mut pixels, false);
        // 위로 넘어간 줄의 자리에는 고르는 단추가 그대로 잡힌다.
        assert_eq!(list.hit(MARGIN + 30.0, MARGIN + CHIPS_TOP + 13.0), Some(Spot::Filter(Filter::All)));
        // 보이는 곳의 맨 위에 걸친 줄은 넷째 줄(27번)이다.
        assert_eq!(list.hit(MARGIN + 100.0, MARGIN + ROWS_TOP + 4.0), Some(Spot::Row(27)));
        // 아래로 넘친 줄의 자리에는 아래의 단추가 잡힌다.
        assert_eq!(list.hit(MARGIN + 60.0, MARGIN + ROWS_TOP + ROWS_TALL + 26.0), Some(Spot::Foot(Foot::Add)));
        assert_eq!(spots(&list, |spot| *spot == Spot::Scroll), [Spot::Scroll]);
        // 줄은 보이는 곳 밖에 그려지지 않는다. 점선 바로 아래의 빈 곳은 창의 바탕색 그대로다.
        let at = (MARGIN + ROWS_TOP + ROWS_TALL + 5.0) as usize * wide as usize + (MARGIN + 100.0) as usize;
        assert_eq!(pixels[at], 0xFFFF_FDF9);
    }
}
