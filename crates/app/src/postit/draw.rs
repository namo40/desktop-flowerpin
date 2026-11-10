//! 포스트잇 한 장을 그린다.
//!
//! 그림 파일을 쓰지 않는다. 종이, 말린 귀퉁이, 그림자, 꽃 핀, 단추를 모두 식으로 그린다. 픽셀마다 그 자리가
//! 포스트잇의 어디인지를 거꾸로 셈해서(기울기와 크기를 되돌려서) 색을 정한다. 그래서 기울이거나 키워도
//! 가장자리가 매끈하다.
//!
//! 글자와 아이콘은 여기서 그리지 않는다. 반듯한 모양으로 크게 그려 둔 것(`Ink`)을 받아, 포스트잇의 기울기와
//! 크기에 맞춰 옮겨 찍고 자리에 맞는 색을 입힌다. 종이와 글자가 한 장의 그림처럼 함께 움직이므로,
//! 흔들리거나 커질 때 글자만 따로 움직여 보이지 않는다.
//!
//! 길이의 단위는 화면 배율 100%일 때의 픽셀이다. 창을 모른다. 받은 버퍼에 그리기만 한다.

use std::f32::consts::{FRAC_1_SQRT_2, PI, SQRT_2};
use std::sync::LazyLock;

/// 종이 크기의 처음 값과, 줄이고 늘릴 수 있는 범위.
pub const PAPER: (f32, f32) = (196.0, 196.0);
pub const SMALLEST: (f32, f32) = (150.0, 120.0);
pub const LARGEST: (f32, f32) = (420.0, 420.0);
/// 포스트잇이 도는 중심의 높이. 꽃 핀이 꽂힌 자리다. 종이 기준과 창 기준.
const PIVOT_Y: f32 = 14.0;
const PIVOT_CANVAS_Y: f32 = 84.0;
/// 접혀 올라온 귀퉁이의 크기. 여기를 끌면 크기가 바뀐다.
const FOLD: f32 = 26.0;
/// 마우스를 올리면 나오는 단추의 수와 반지름, 놓이는 높이.
pub const TOOLS: usize = 4;
const TOOL_R: f32 = 15.0;
const TOOL_Y: f32 = -29.0;
/// 꽃 핀의 높이.
const PIN_Y: f32 = 4.0;
/// 글자를 화면에 보이는 것의 몇 배로 그려 둘지. 알림이 울릴 때 1.28배로 커져도 늘리지 않고 줄여 쓰게 된다.
pub const SUPER: f32 = 2.0;

pub type Rgb = (f32, f32, f32);

pub const fn rgb(hex: u32) -> Rgb {
    (((hex >> 16) & 0xFF) as f32 / 255.0, ((hex >> 8) & 0xFF) as f32 / 255.0, (hex & 0xFF) as f32 / 255.0)
}

pub const CREAM: Rgb = rgb(0xFFFDF9);
pub const PINK: Rgb = rgb(0xE7879C);
pub const ROSE: Rgb = rgb(0xB23A55);
/// 작은 글씨에 쓰는 짙은 장미색. 가는 글씨는 `ROSE`로 쓰면 흐려 보인다.
pub const DEEP: Rgb = rgb(0x8A3049);
pub const INK: Rgb = rgb(0x4A3A3D);
pub const WHITE: Rgb = rgb(0xFFFFFF);
pub const HOT: Rgb = rgb(0xFFE9EE);
const GLOW: Rgb = rgb(0xFFD45C);
const SHADOW: Rgb = rgb(0x141E46);

/// 고를 수 있는 기본 종이 색 열 가지.
pub const COLORS: [Rgb; 10] = [
    rgb(0xFFF3A6), // 버터 노랑
    rgb(0xFFD0DC), // 벚꽃 분홍
    rgb(0xCFF1D2), // 민트
    rgb(0xCBE6FB), // 하늘
    rgb(0xE2D4F7), // 라벤더
    rgb(0xFFDCC2), // 복숭아
    rgb(0xE4F3B5), // 연두
    rgb(0xC4EEE8), // 물빛
    rgb(0xFFC7BD), // 산호
    rgb(0xF6F1E7), // 크림
];

pub fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t)
}

/// 색을 색상(0~1), 진하기(0~1), 밝기(0~1)로 바꾼다.
pub fn to_hsv(color: Rgb) -> (f32, f32, f32) {
    let (high, low) = (color.0.max(color.1).max(color.2), color.0.min(color.1).min(color.2));
    let spread = high - low;
    let hue = if spread < 1e-5 {
        0.0
    } else if high == color.0 {
        ((color.1 - color.2) / spread).rem_euclid(6.0)
    } else if high == color.1 {
        (color.2 - color.0) / spread + 2.0
    } else {
        (color.0 - color.1) / spread + 4.0
    };
    (hue / 6.0, if high <= 0.0 { 0.0 } else { spread / high }, high)
}

pub fn from_hsv(hue: f32, saturation: f32, value: f32) -> Rgb {
    let part = |offset: f32| {
        let k = (offset + hue * 6.0).rem_euclid(6.0);
        value - value * saturation * k.min(4.0 - k).clamp(0.0, 1.0)
    };
    (part(5.0), part(3.0), part(1.0))
}

/// 종이 색이 어두우면 글씨를 밝게 쓴다.
pub fn is_dark(color: Rgb) -> bool {
    0.299 * color.0 + 0.587 * color.1 + 0.114 * color.2 < 0.55
}

/// 창(그림 버퍼)의 크기. 커지고 기울고 단추가 나와도 들어갈 만큼 넉넉하다.
pub fn canvas(paper: (f32, f32)) -> (f32, f32) {
    (paper.0 * 1.3 + 86.0, PIVOT_CANVAS_Y + (paper.1 - PIVOT_Y) * 1.3 + 84.0)
}

fn pivot(paper: (f32, f32)) -> (f32, f32) {
    (paper.0 / 2.0, PIVOT_Y)
}

fn pivot_canvas(paper: (f32, f32)) -> (f32, f32) {
    (canvas(paper).0 / 2.0, PIVOT_CANVAS_Y)
}

/// 기울기가 없을 때, 창의 왼쪽 위에서 종이의 왼쪽 위까지의 거리.
pub fn paper_offset(paper: (f32, f32)) -> (f32, f32) {
    (pivot_canvas(paper).0 - pivot(paper).0, PIVOT_CANVAS_Y - PIVOT_Y)
}

/// 글자를 그려 두는 그림이 덮는 범위: 왼쪽 위의 자리와 크기. 단추와 답 단추까지 들어간다.
pub fn sheet(paper: (f32, f32)) -> ((f32, f32), (f32, f32)) {
    ((-10.0, -54.0), (paper.0 + 20.0, paper.1 + 54.0 + 46.0))
}

/// 글이 놓이는 범위(왼쪽, 위, 오른쪽, 아래).
pub fn text_rect(paper: (f32, f32)) -> (f32, f32, f32, f32) {
    (16.0, 30.0, paper.0 - 16.0, paper.1 - 38.0)
}

/// 알림 시각이 놓이는 범위. 왼쪽 아래다. `width`는 글의 길이에 따라 달라진다.
pub fn chip_rect(paper: (f32, f32), width: f32) -> (f32, f32, f32, f32) {
    (12.0, paper.1 - 31.0, 12.0 + width, paper.1 - 11.0)
}

/// 알림 시각 칸의 너비. `text`는 글의 너비다.
pub fn chip_width(text: f32) -> f32 {
    6.0 + 13.0 + 4.0 + text + 9.0
}

pub fn tool_center(index: usize, tools: f32, paper: (f32, f32)) -> (f32, f32) {
    (paper.0 / 2.0 + (index as f32 - (TOOLS as f32 - 1.0) / 2.0) * 36.0, TOOL_Y + (1.0 - tools) * 8.0)
}

/// 알림이 울릴 때 나오는 답 단추 둘(확인, 5분 뒤 다시)의 범위.
pub fn answer_rect(index: usize, paper: (f32, f32)) -> (f32, f32, f32, f32) {
    let left = paper.0 / 2.0 - 81.0;
    let (from, to) = if index == 0 { (left, left + 58.0) } else { (left + 66.0, left + 162.0) };
    (from, paper.1 + 12.0, to, paper.1 + 40.0)
}

/// 지금 이 순간의 모습.
#[derive(Clone, Copy)]
pub struct Look {
    /// 종이의 크기와 색.
    pub paper: (f32, f32),
    pub color: Rgb,
    /// 기울기(라디안). 시계 방향이 양수다.
    pub tilt: f32,
    /// 크기. 가로와 세로를 따로 줄 수 있다(붙을 때 눌리는 모습).
    pub size: (f32, f32),
    pub opacity: f32,
    /// 단추들이 나온 정도(0~1)와, 마우스가 올라가 있는 단추, 켜져 있는 단추.
    pub tools: f32,
    pub hot_tool: Option<usize>,
    pub on: [bool; TOOLS],
    /// 항상 위. 꽃 핀이 커지고 금색이 된다.
    pub top: bool,
    /// 알림이 울릴 때의 빛(0~1)과 답 단추가 나온 정도(0~1).
    pub glow: f32,
    pub answers: f32,
    pub hot_answer: Option<usize>,
    /// 들어 올린 정도(0~1). 그림자가 멀어지고 흐려진다.
    pub lift: f32,
    /// 적는 중. 분홍 테를 두르고 글은 그리지 않는다(그 자리에 입력 칸이 올라온다).
    pub editing: bool,
    /// 구겨진 정도(0~1). 1이면 공이 된다.
    pub crumple: f32,
    /// 꽃잎이 흩어진 정도(0~1). 지울 때만 쓴다.
    pub petals: Option<f32>,
    pub seed: u32,
}

/// 반듯한 모양으로 크게 그려 둔 글자와 아이콘. 픽셀마다 흰 바탕에서 어두워진 정도가 들어 있다.
pub struct Ink<'a> {
    pub pixels: &'a [u32],
    pub stride: usize,
    /// 그려 둔 그림의 크기(px).
    pub size: (i32, i32),
    /// 그림의 왼쪽 위에 오는 포스트잇의 자리와, 길이 1이 몇 픽셀로 그려져 있는지.
    pub origin: (f32, f32),
    pub density: f32,
    /// 글이 그려진 범위의 아래 끝. 이 아래로 넘친 줄은 보여 주지 않는다.
    pub text_bottom: f32,
    /// 글이 실제로 그려져 있는 범위. 이 밖에서는 글자 그림을 읽지 않는다.
    pub body: (f32, f32, f32, f32),
    /// 알림 시각 칸의 너비. 알림이 없으면 `None`이다.
    pub chip: Option<f32>,
    /// 글이 비어 있어 "여기에 적어요"를 흐리게 보여 주는 중이다.
    pub dim: bool,
}

/// 포스트잇에서 마우스가 닿은 곳.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Paper,
    Pin,
    /// 접힌 귀퉁이. 끌면 크기가 바뀐다.
    Corner,
    Tool(usize),
    Answer(usize),
}

/// 창 안의 점을 포스트잇 위의 점으로 바꾸는 셈. 한 장을 그리는 동안에는 달라지지 않으므로 한 번만 구해 둔다.
/// 픽셀마다 사인과 코사인을 다시 구하면 그리는 시간의 큰 몫이 거기에 든다.
#[derive(Clone, Copy)]
struct Inverse {
    center: (f32, f32),
    origin: (f32, f32),
    sin: f32,
    cos: f32,
    /// 크기로 나누는 대신 곱할 값. 픽셀마다 하는 나눗셈은 곱셈보다 몇 배 느리다.
    per_size: (f32, f32),
}

impl Inverse {
    fn of(look: &Look) -> Inverse {
        let (sin, cos) = look.tilt.sin_cos();
        let per_size = (1.0 / look.size.0, 1.0 / look.size.1);
        Inverse { center: pivot_canvas(look.paper), origin: pivot(look.paper), sin, cos, per_size }
    }

    /// 창 안의 한 점(100% 기준 단위)이 포스트잇의 어디인지 거꾸로 셈한다.
    fn local(&self, x: f32, y: f32) -> (f32, f32) {
        let (dx, dy) = (x - self.center.0, y - self.center.1);
        (
            self.origin.0 + (dx * self.cos + dy * self.sin) * self.per_size.0,
            self.origin.1 + (-dx * self.sin + dy * self.cos) * self.per_size.1,
        )
    }
}

/// 창 안의 한 점(100% 기준 단위)이 포스트잇의 어디인지 거꾸로 셈한다.
fn to_local(look: &Look, x: f32, y: f32) -> (f32, f32) {
    Inverse::of(look).local(x, y)
}

/// 꽃 핀의 모양에서 한 번만 구해 두는 값.
struct PinShape {
    /// 꽃잎 다섯 장의 가운데. 위에서부터 72도씩 돌아간다.
    petals: [(f32, f32); 5],
    /// "항상 위"일 때 돌아간 각도의 사인과 코사인.
    turned: (f32, f32),
}

static PIN: LazyLock<PinShape> = LazyLock::new(|| {
    let center = |k: usize| {
        let angle = (-90.0 + 72.0 * k as f32).to_radians();
        (7.6 * angle.cos(), 7.6 * angle.sin())
    };
    PinShape { petals: std::array::from_fn(center), turned: (18.0f32).to_radians().sin_cos() }
});

/// 모서리가 둥근 네모까지의 거리. 안쪽이 음수다. `rect`는 (왼쪽, 위, 오른쪽, 아래)다.
pub fn round_box(u: f32, v: f32, rect: (f32, f32, f32, f32), radius: f32) -> f32 {
    let (cx, cy, hx, hy) =
        ((rect.0 + rect.2) / 2.0, (rect.1 + rect.3) / 2.0, (rect.2 - rect.0) / 2.0, (rect.3 - rect.1) / 2.0);
    let (qx, qy) = ((u - cx).abs() - hx + radius, (v - cy).abs() - hy + radius);
    qx.max(qy).min(0.0) + (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() - radius
}

/// 종이까지의 거리. 오른쪽 아래 귀퉁이가 접힌 만큼 잘려 있고, 구겨질수록 둥글어진다.
fn paper_edge(u: f32, v: f32, paper: (f32, f32), crumple: f32) -> f32 {
    let radius = 3.0 + crumple * (paper.0.min(paper.1) / 2.0 - 3.0);
    let body = round_box(u, v, (0.0, 0.0, paper.0, paper.1), radius);
    let cut = (u + v - (paper.0 + paper.1 - FOLD * (1.0 - crumple))) * FRAC_1_SQRT_2;
    body.max(cut)
}

/// 꽃 핀(꽃잎 다섯 장)까지의 거리와, 가운데 동그라미까지의 거리.
fn flower(u: f32, v: f32, paper: (f32, f32), top: bool) -> (f32, f32) {
    let (mut x, mut y) = (u - paper.0 / 2.0, v - PIN_Y);
    let (centers, (sin, cos)) = (&PIN.petals, PIN.turned);
    if top {
        // "항상 위"면 조금 커지고 돌아가 있다.
        (x, y) = ((x * cos + y * sin) / 1.18, (-x * sin + y * cos) / 1.18);
    }
    let mut petals = f32::MAX;
    for (cx, cy) in centers {
        petals = petals.min(((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - 6.4);
    }
    (petals, (x * x + y * y).sqrt() - 4.0)
}

/// 창 안의 한 점에 무엇이 있는지. 그림자나 빛만 있는 곳은 `None`이다.
pub fn hit(look: &Look, x: f32, y: f32) -> Option<Hit> {
    let (u, v) = to_local(look, x, y);
    let inside = |rect: (f32, f32, f32, f32)| (rect.0..=rect.2).contains(&u) && (rect.1..=rect.3).contains(&v);
    if look.answers > 0.5
        && let Some(index) = (0..2).find(|&index| inside(answer_rect(index, look.paper)))
    {
        return Some(Hit::Answer(index));
    }
    if look.tools > 0.5 {
        for index in 0..TOOLS {
            let (cx, cy) = tool_center(index, look.tools, look.paper);
            if (u - cx).powi(2) + (v - cy).powi(2) <= (TOOL_R + 1.0).powi(2) {
                return Some(Hit::Tool(index));
            }
        }
    }
    if flower(u, v, look.paper, look.top).0 <= 1.0 {
        return Some(Hit::Pin);
    }
    // 접힌 귀퉁이는 잘려 나간 세모까지 쳐서 잡기 쉽게 한다.
    if inside((look.paper.0 - FOLD - 4.0, look.paper.1 - FOLD - 4.0, look.paper.0 + 3.0, look.paper.1 + 3.0)) {
        return Some(Hit::Corner);
    }
    (paper_edge(u, v, look.paper, look.crumple) <= 0.0).then_some(Hit::Paper)
}

/// 겹쳐 칠해 가는 픽셀 하나. 색에는 투명도가 미리 곱해져 있다.
#[derive(Clone, Copy, Default)]
pub struct Pixel {
    r: f32,
    g: f32,
    b: f32,
    a: f32,
}

impl Pixel {
    pub fn unpack(value: u32) -> Pixel {
        let part = |shift: u32| ((value >> shift) & 0xFF) as f32 / 255.0;
        Pixel { r: part(16), g: part(8), b: part(0), a: part(24) }
    }

    /// 위에 `color`를 빨강·초록·파랑마다 다른 정도로 덧칠한다. ClearType으로 그린 글자를 찍을 때 쓴다.
    pub fn tint(&mut self, color: Rgb, alpha: [f32; 3]) {
        let most = alpha[0].max(alpha[1]).max(alpha[2]).clamp(0.0, 1.0);
        self.r = color.0 * alpha[0] + self.r * (1.0 - alpha[0]);
        self.g = color.1 * alpha[1] + self.g * (1.0 - alpha[1]);
        self.b = color.2 * alpha[2] + self.b * (1.0 - alpha[2]);
        self.a = most + self.a * (1.0 - most);
    }

    /// 위에 `color`를 `alpha`만큼 덧칠한다.
    pub fn over(&mut self, color: Rgb, alpha: f32) {
        let alpha = alpha.clamp(0.0, 1.0);
        let keep = 1.0 - alpha;
        self.r = color.0 * alpha + self.r * keep;
        self.g = color.1 * alpha + self.g * keep;
        self.b = color.2 * alpha + self.b * keep;
        self.a = alpha + self.a * keep;
    }

    pub fn pack(self, opacity: f32) -> u32 {
        let byte = |value: f32| (value * opacity * 255.0 + 0.5).clamp(0.0, 255.0) as u32;
        byte(self.a) << 24 | byte(self.r) << 16 | byte(self.g) << 8 | byte(self.b)
    }
}

/// 순서 없는 수를 차례로 내놓는다. 꽃잎이 흩어지는 방향을 정하는 데 쓴다.
fn noise(seed: &mut u32) -> f32 {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (*seed >> 8) as f32 / (1u32 << 24) as f32
}

/// 흩어지는 꽃잎 한 장: 가운데, 크기, 돌아간 각, 투명도.
fn petal(look: &Look, index: usize, progress: f32) -> ((f32, f32), f32, f32, f32) {
    let mut seed = look.seed.wrapping_add(index as u32 * 7919);
    let (angle, far, turn) =
        (noise(&mut seed) * 2.0 * PI, 40.0 + noise(&mut seed) * 60.0, noise(&mut seed) * 2.0 * PI - PI);
    let (dx, dy) = (angle.cos() * far, angle.sin() * far);
    let center = (pivot_canvas(look.paper).0, PIVOT_CANVAS_Y + look.paper.1 / 2.0 - PIVOT_Y);
    // 처음 절반쯤은 퍼져 나가고, 그 뒤로는 떨어지며 사라진다.
    let (spread, fall) = ((progress / 0.45).min(1.0), ((progress - 0.45) / 0.55).max(0.0));
    let ease = 1.0 - (1.0 - spread).powi(3);
    let at = (center.0 + dx * (ease + 0.25 * fall), center.1 + dy * ease + 90.0 * fall * fall);
    (at, 0.3 + 0.7 * ease - 0.2 * fall, turn * (ease + fall), 1.0 - fall)
}

/// 종이 밖으로 무엇인가 그려질 수 있는 가장 먼 거리(옆, 위, 아래). 종이의 길이 단위다.
/// 무엇이 나와 있든 이 밖은 셈하지 않는다.
const FAR: (f32, f32, f32) = (26.0, 52.0, 50.0);

/// 지금 모습에서 종이 밖으로 무엇인가 그려질 수 있는 거리(옆, 위, 아래). 이 밖은 셈하지 않고 비워 둔다.
///
/// 그림자, 빛, 꽃 핀, 단추가 닿는 데까지다. 나와 있지 않은 것의 몫을 빼면 셈할 넓이가 크게 준다.
/// 가만히 붙어 있을 때는 [`FAR`]로 셈할 때의 3분의 2쯤이다. 그려지는 그림은 [`FAR`]로 셈한 것과 같다.
fn reach(look: &Look, scale: f32) -> (f32, f32, f32) {
    let (drop, blur) = (7.0 + 9.0 * look.lift, 7.0 + 7.0 * look.lift);
    let half = look.paper.0 / 2.0;
    // 가장자리를 부드럽게 하는 폭. 작게 그릴수록 종이의 길이로는 넓어진다.
    let soft = 0.5 / (scale * look.size.0.min(look.size.1)).max(0.01) + 1.0;
    let glow = if look.glow > 0.0 { 15.0 } else { 0.0 };
    let mut side = blur.max(glow);
    // 꽃 핀은 종이 위로 조금 올라온다. "항상 위"일 때의 큰 핀까지 친다.
    let mut top = glow.max(13.0);
    let mut bottom = (drop + blur).max(glow);
    if look.tools > 0.0 {
        // 단추들과, 단추와 종이 사이에서 마우스를 받는 자리.
        side = side.max((TOOLS as f32 * 18.0 + 8.0).max(70.0 + soft) - half);
        top = FAR.1;
    }
    if look.answers > 0.0 {
        side = side.max(81.0 - half);
        bottom = bottom.max(40.0);
    }
    ((side + soft).min(FAR.0), (top + soft).min(FAR.1), (bottom + soft).min(FAR.2))
}

/// 포스트잇 한 장을 `pixels`에 그린다. 한 줄이 `stride`개인 버퍼의 왼쪽 위 `width`×`height`만 쓴다.
/// 값은 미리 곱한 알파의 BGRA이고, `scale`은 화면 배율(100%면 1)이다.
pub fn draw(pixels: &mut [u32], stride: usize, width: i32, height: i32, scale: f32, look: &Look, ink: &Ink) {
    let plan = Plan { bound: reach(look, scale), shortcut: true };
    draw_with(pixels, stride, (width, height), scale, look, ink, plan);
}

/// 셈을 줄이는 방법. 무엇을 고르든 그려지는 그림은 같다.
#[derive(Clone, Copy)]
struct Plan {
    /// 종이 밖으로 어디까지 셈할지(옆, 위, 아래).
    bound: (f32, f32, f32),
    /// 아무것도 얹히지 않은 종이 한가운데는 색만 정해 칠한다.
    shortcut: bool,
}

/// [`draw`]와 같다. 셈을 줄이는 방법을 따로 받는다.
fn draw_with(
    pixels: &mut [u32],
    stride: usize,
    (width, height): (i32, i32),
    scale: f32,
    look: &Look,
    ink: &Ink,
    plan: Plan,
) {
    let (w, h) = look.paper;
    let bound = plan.bound;
    let inverse = Inverse::of(look);
    // 그늘진 아래쪽은 조금 더 진하고, 접힌 귀퉁이는 종이의 뒷면이라 더 밝다.
    let (hue, saturation, value) = to_hsv(look.color);
    let shade = from_hsv(hue, (saturation * 1.25 + 0.03).min(1.0), value * 0.975);
    let fold = mix(look.color, WHITE, 0.55);
    let ink_color = if is_dark(look.color) { CREAM } else { INK };
    // 종이 위에서의 한 픽셀 크기. 가장자리를 이만큼의 폭으로 부드럽게 한다.
    let sharp = scale * look.size.0.min(look.size.1);
    let cover = |distance: f32| (0.5 - distance * sharp).clamp(0.0, 1.0);
    let (shadow_drop, shadow_blur) = (7.0 + 9.0 * look.lift, 7.0 + 7.0 * look.lift);
    let text = text_rect(look.paper);
    let chip = ink.chip.map(|wide| chip_rect(look.paper, wide));
    let petals: Vec<_> =
        look.petals.map(|progress| (0..7).map(|k| petal(look, k, progress)).collect()).unwrap_or_default();

    // 크게 그려 둔 글자 그림에서, 포스트잇의 한 점에 해당하는 값을 읽는다.
    // 화면의 한 픽셀이 글자 그림의 여러 픽셀을 덮으므로, 네 군데를 읽어 고르게 섞는다.
    let quarter = ink.density / (scale * look.size.0.min(look.size.1).max(0.2)) / 4.0;
    let tap = |x: f32, y: f32| -> f32 {
        let (x, y) = (x - 0.5, y - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        if x0 < 0.0 || y0 < 0.0 || x0 as i32 + 1 >= ink.size.0 || y0 as i32 + 1 >= ink.size.1 {
            return 0.0;
        }
        let (fx, fy) = (x - x0, y - y0);
        let at = y0 as usize * ink.stride + x0 as usize;
        let dark = |offset: usize| (255 - ((ink.pixels[at + offset] >> 8) & 0xFF)) as f32;
        let top = dark(0) * (1.0 - fx) + dark(1) * fx;
        let bottom = dark(ink.stride) * (1.0 - fx) + dark(ink.stride + 1) * fx;
        (top * (1.0 - fy) + bottom * fy) / 255.0
    };
    let read = |u: f32, v: f32| -> f32 {
        let (x, y) = ((u - ink.origin.0) * ink.density, (v - ink.origin.1) * ink.density);
        0.25 * (tap(x - quarter, y - quarter)
            + tap(x + quarter, y - quarter)
            + tap(x - quarter, y + quarter)
            + tap(x + quarter, y + quarter))
    };

    // 픽셀마다 나누는 대신 곱할 값들.
    let (per_scale, per_h, per_blur) = (1.0 / scale, 1.0 / h, 1.0 / (2.0 * shadow_blur));
    // 종이 색만 칠해도 되는 곳의 조건. 구겨지는 중에는 종이의 모양과 면이 달라지므로 쓰지 않는다.
    // 가장자리에서 `inset`보다 깊이 들어와 있어야 한다. 가장자리를 부드럽게 하는 폭보다 넉넉히 깊다.
    let shortcut = plan.shortcut && look.crumple == 0.0;
    let inset = (1.0 / sharp.max(0.001)).max(3.0);
    // 접혀 잘려 나간 귀퉁이의 선에서도 그만큼 떨어져 있어야 한다.
    let uncut = w + h - FOLD - inset * SQRT_2;
    for py in 0..height {
        let y = (py as f32 + 0.5) * per_scale;
        for px in 0..width {
            let x = (px as f32 + 0.5) * per_scale;
            let at = py as usize * stride + px as usize;
            let mut pixel = Pixel::default();

            if look.petals.is_none() {
                let (u, v) = inverse.local(x, y);
                // 포스트잇에서 먼 곳은 건너뛴다. 빛, 그림자, 단추가 닿는 데까지만 본다.
                if !(-bound.0..=w + bound.0).contains(&u) || !(-bound.1..=h + bound.2).contains(&v) {
                    pixels[at] = 0;
                    continue;
                }
                let body = ink.body;
                let in_text = u > body.0 && u < body.2 && v > body.1 && v < body.3.min(ink.text_bottom + 2.0);
                // 종이 한가운데의, 아무것도 얹히지 않은 곳은 가장 넓으면서 가장 단순하다. 가장자리에서 멀어
                // 빛도 그림자도 닿지 않고, 글자도 핀도 접힌 귀퉁이도 없다. 종이 색만 정해 칠하고 넘어간다.
                if shortcut
                    && u >= inset
                    && u <= w - inset
                    && v >= inset
                    && v <= h - inset
                    && u + v <= uncut
                    && !(u > w - FOLD && v > h - FOLD)
                    && !(v < 26.0 && (u - w / 2.0).abs() < 22.0)
                    && !in_text
                    && !chip.is_some_and(|rect| {
                        u >= rect.0 - 2.0 && u <= rect.2 + 2.0 && v >= rect.1 - 2.0 && v <= rect.3 + 2.0
                    })
                {
                    let color = mix(look.color, shade, ((v * per_h - 0.7) * (1.0 / 0.3)).clamp(0.0, 1.0));
                    pixels[at] = Pixel { r: color.0, g: color.1, b: color.2, a: 1.0 }.pack(look.opacity);
                    continue;
                }
                let distance = paper_edge(u, v, look.paper, look.crumple);
                // 그 자리에 그려져 있는 글자나 아이콘이 덮은 정도. 글자가 있을 수 있는 곳에서만 읽는다.
                let in_chip = chip.is_some_and(|rect| u > rect.0 && u < rect.2 && v > rect.1 && v < rect.3);
                let inked = (distance < 1.0 && (in_text || in_chip))
                    || (look.tools > 0.0 && v < TOOL_Y + 26.0)
                    || (look.answers > 0.0 && v > h + 10.0);
                let written = if inked { read(u, v) } else { 0.0 };

                // 빛과 그림자. 종이 안쪽 깊은 곳은 종이에 가려지므로 셈하지 않는다.
                let edge = distance > -1.5;
                if edge && look.glow > 0.0 {
                    pixel.over(GLOW, look.glow * 0.9 * (1.0 - distance * (1.0 / 15.0)).clamp(0.0, 1.0).powi(2));
                }
                if edge {
                    let under = paper_edge(u, v - shadow_drop, look.paper, look.crumple);
                    let soft = (0.5 - under * per_blur).clamp(0.0, 1.0);
                    pixel.over(SHADOW, 0.34 * soft * soft * (3.0 - 2.0 * soft));
                }

                // 종이. 아래쪽 30%는 조금 그늘지고, 오른쪽 아래에는 접혀 올라온 귀퉁이가 있다.
                let on_paper = cover(distance);
                if on_paper > 0.0 {
                    let mut color = mix(look.color, shade, ((v * per_h - 0.7) * (1.0 / 0.3)).clamp(0.0, 1.0));
                    let corner = FOLD * (1.0 - look.crumple);
                    if u > w - corner && v > h - corner {
                        let crease = (u + v - (w + h - corner)) / SQRT_2;
                        color = mix(fold, mix(fold, SHADOW, 0.35), (1.0 + crease / 1.6).clamp(0.0, 1.0));
                    } else if look.crumple > 0.0 {
                        // 구겨지면 면이 꺾인다. 방향이 다른 줄무늬 셋을 휘어 겹쳐 밝은 면과 어두운 면을 만든다.
                        let ridge = |x: f32| ((x - x.floor()) - 0.5).abs() * 2.0;
                        let (a, b) = (u + 9.0 * (v * 0.045).sin(), v + 11.0 * (u * 0.038).sin());
                        let facets = ridge(a * 0.023 + b * 0.011)
                            + ridge(b * 0.027 - a * 0.008 + 0.4)
                            + ridge((a - b) * 0.015 + 0.7);
                        let bend = ((facets - 1.5) * 1.4).clamp(-1.0, 1.0) * look.crumple;
                        color =
                            if bend > 0.0 { mix(color, WHITE, 0.45 * bend) } else { mix(color, SHADOW, -0.22 * bend) };
                    }
                    if look.editing && distance > -2.0 {
                        color = PINK;
                    }
                    pixel.over(color, on_paper);

                    // 알림 시각. 그 안의 글자는 장밋빛이다.
                    let in_chip = chip.is_some_and(|rect| {
                        // 칸에서 먼 곳은 거리를 재지 않는다.
                        if u < rect.0 - 2.0 || u > rect.2 + 2.0 || v < rect.1 - 2.0 || v > rect.3 + 2.0 {
                            return false;
                        }
                        let pill = round_box(u, v, rect, 10.0);
                        if pill < 1.0 {
                            pixel.over(PINK, cover(pill));
                            pixel.over(CREAM, cover(pill + 1.5));
                        }
                        pill < 0.0
                    });
                    // 글. 넘친 줄은 보여 주지 않는다.
                    if in_chip {
                        pixel.over(ROSE, written);
                    } else if !look.editing && look.crumple < 0.5 && v < ink.text_bottom && v > text.1 - 4.0 {
                        let faint = if ink.dim { 0.38 } else { 1.0 } * (1.0 - 2.0 * look.crumple);
                        pixel.over(ink_color, written * on_paper * faint);
                    }
                }

                // 꽃 핀. 종이 위쪽 가운데에 꽂혀 있다. 구겨질 때는 먼저 사라진다.
                let pinned = (1.0 - 2.5 * look.crumple).clamp(0.0, 1.0);
                if pinned > 0.0 && v < 26.0 && (u - w / 2.0).abs() < 22.0 {
                    let (below, _) = flower(u, v - 2.0, look.paper, look.top);
                    pixel.over(rgb(0x781E3C), 0.3 * (0.5 - below / 4.0).clamp(0.0, 1.0) * pinned);
                    let (petal, core) = flower(u, v, look.paper, look.top);
                    pixel.over(PINK, cover(petal) * pinned);
                    pixel.over(rgb(0xFF9DB5), cover(petal + 1.0) * pinned);
                    pixel.over(PINK, cover(core) * pinned);
                    pixel.over(if look.top { rgb(0xFFC93C) } else { rgb(0xFFE27A) }, cover(core + 1.0) * pinned);
                }

                // 마우스를 올리면 나오는 단추들.
                if look.tools > 0.0 && v < TOOL_Y + 26.0 {
                    for index in 0..TOOLS {
                        let (cx, cy) = tool_center(index, look.tools, look.paper);
                        let ring = ((u - cx).powi(2) + (v - cy).powi(2)).sqrt() - TOOL_R;
                        if ring < 1.0 {
                            let on = look.on[index];
                            let fill = if on {
                                ROSE
                            } else if look.hot_tool == Some(index) {
                                HOT
                            } else {
                                CREAM
                            };
                            pixel.over(if on { ROSE } else { PINK }, cover(ring) * look.tools);
                            pixel.over(fill, cover(ring + 1.5) * look.tools);
                            pixel.over(if on { WHITE } else { ROSE }, written * look.tools);
                        }
                    }
                }

                // 알림이 울릴 때의 답 단추.
                if look.answers > 0.0 && v > h + 10.0 {
                    for index in 0..2 {
                        let rect = answer_rect(index, look.paper);
                        let pill = round_box(u, v, rect, (rect.3 - rect.1) / 2.0);
                        if pill < 1.0 {
                            let (filled, hot) = (index == 0, look.hot_answer == Some(index));
                            let fill = match (filled, hot) {
                                (true, false) => ROSE,
                                (true, true) => mix(ROSE, WHITE, 0.15),
                                (false, false) => CREAM,
                                (false, true) => HOT,
                            };
                            pixel.over(ROSE, cover(pill) * look.answers);
                            pixel.over(fill, cover(pill + 1.5) * look.answers);
                            pixel.over(if filled { WHITE } else { ROSE }, written * look.answers);
                        }
                    }
                }
            } else {
                // 지워진 뒤: 꽃잎만 흩어진다.
                for &((cx, cy), size, turn, alpha) in &petals {
                    let (dx, dy) = (x - cx, y - cy);
                    if dx.abs() > 14.0 || dy.abs() > 14.0 {
                        continue;
                    }
                    let (sin, cos) = turn.sin_cos();
                    let (qx, qy) = ((dx * cos + dy * sin) / size, (-dx * sin + dy * cos) / size);
                    let reach = ((qx / 8.0).powi(2) + (qy / 11.0).powi(2)).sqrt();
                    let edge = (0.5 - (reach - 1.0) * 8.0 * scale * size).clamp(0.0, 1.0);
                    pixel.over(mix(rgb(0xFFD6E0), rgb(0xF29DB4), reach.min(1.0)), edge * alpha);
                }
            }
            let mut packed = pixel.pack(look.opacity);
            // 단추가 나와 있는 동안에는 단추와 종이 사이의 빈 곳도 창이 마우스를 받게 한다.
            // 완전히 투명한 픽셀 위에서는 마우스가 창을 벗어난 것이 되어, 단추로 가는 길에 단추가 사라진다.
            // 눈에 보이지 않을 만큼만(255분의 1) 칠해 둔다.
            if packed >> 24 == 0 && look.tools > 0.0 && look.petals.is_none() {
                let (u, v) = inverse.local(x, y);
                let reach = (TOOLS as f32 * 18.0 + 8.0).max(w / 2.0);
                if (u - w / 2.0).abs() <= reach && (TOOL_Y - TOOL_R - 8.0..=2.0).contains(&v) {
                    packed = 1 << 24;
                }
            }
            pixels[at] = packed;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(paper: (f32, f32)) -> Look {
        Look {
            paper,
            color: rgb(0xFFF3A6),
            tilt: 0.0,
            size: (1.0, 1.0),
            opacity: 1.0,
            tools: 0.0,
            hot_tool: None,
            on: [false; TOOLS],
            top: false,
            glow: 0.0,
            answers: 0.0,
            hot_answer: None,
            lift: 0.0,
            editing: false,
            crumple: 0.0,
            petals: None,
            seed: 1,
        }
    }

    #[test]
    fn narrowing_what_is_computed_changes_no_pixel() {
        // 셈을 줄여 그린 그림이, 가장 먼 데까지 지름길 없이 다 셈한 그림과 한 픽셀도 다르지 않아야 한다.
        let paper = SMALLEST;
        let size = canvas(paper);
        let scale = 0.75;
        let (width, height) = ((size.0 * scale).ceil() as i32, (size.1 * scale).ceil() as i32);
        let (origin, extent) = sheet(paper);
        let density = SUPER * scale;
        let ink_size = ((extent.0 * density).ceil() as i32, (extent.1 * density).ceil() as i32);
        // 글자 대신 점을 고르게 찍어 둔 그림. 글이 있는 범위는 종이의 가운데쯤이다.
        let dotted: Vec<u32> =
            (0..ink_size.0 * ink_size.1).map(|at| if at % 7 < 3 { 0x0000_0000 } else { 0x00FF_FFFF }).collect();
        let text = text_rect(paper);
        let ink = |chip| Ink {
            pixels: &dotted,
            stride: ink_size.0 as usize,
            size: ink_size,
            origin,
            density,
            text_bottom: text.3,
            body: (text.0 + 10.0, text.1 + 4.0, text.2 - 30.0, text.1 + 30.0),
            chip,
            dim: false,
        };
        let base = plain(paper);
        let looks = [
            base,
            Look { tilt: 0.12, ..base },
            Look { tilt: -0.3, lift: 1.0, size: (1.04, 1.04), ..base },
            Look { top: true, tilt: 0.05, ..base },
            Look { tools: 1.0, ..base },
            Look { tools: 0.4, tilt: -0.1, top: true, ..base },
            Look { glow: 1.0, answers: 1.0, size: (1.28, 1.28), tilt: 0.04, ..base },
            Look { glow: 0.3, answers: 0.5, size: (1.1, 1.1), ..base },
            Look { glow: 1.0, tools: 1.0, lift: 0.5, ..base },
            // 붙을 때: 작게 나타나고, 눌리며 붙는다.
            Look { size: (0.15, 0.15), opacity: 0.4, tilt: -0.25, ..base },
            Look { size: (0.95, 0.95 * 1.08), ..base },
            // 적는 중과 구겨지는 중.
            Look { editing: true, tools: 1.0, ..base },
            Look { crumple: 0.5, size: (0.7, 0.7), tilt: 0.6, ..base },
            Look { crumple: 1.0, size: (0.16, 0.16), tilt: 1.2, ..base },
        ];
        let mut wide = vec![0u32; (width * height) as usize];
        let mut tight = wide.clone();
        for (number, look) in looks.iter().enumerate() {
            for chip in [None, Some(84.0)] {
                // 견줄 그림은 가장 먼 데까지, 지름길 없이 셈한다.
                let slow = Plan { bound: FAR, shortcut: false };
                draw_with(&mut wide, width as usize, (width, height), scale, look, &ink(chip), slow);
                draw(&mut tight, width as usize, width, height, scale, look, &ink(chip));
                assert!(wide == tight, "{number}번째 모습(알림 칸 {chip:?})의 그림이 달라졌다");
                // 좁힌 범위가 가장 먼 범위를 넘지는 않는다.
                let near = reach(look, scale);
                assert!(near.0 <= FAR.0 && near.1 <= FAR.1 && near.2 <= FAR.2);
            }
            // 무엇인가 그려지기는 했다.
            assert!(tight.iter().any(|pixel| *pixel != 0), "{number}번째 모습이 비어 있다");
        }
        // 가만히 붙어 있을 때는 셈할 넓이가 뚜렷이 준다.
        let (near, area) = (reach(&base, 1.0), |r: (f32, f32, f32)| (paper.0 + 2.0 * r.0) * (paper.1 + r.1 + r.2));
        assert!(area(near) < 0.7 * area(FAR));
    }
}
