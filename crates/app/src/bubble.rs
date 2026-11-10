//! 말풍선. 꽃핀의 머리 위에 뜨는 작은 창이다.
//!
//! 둥근 네모에 꼬리가 달린 모양이고, 제목 한 줄과 내용 몇 줄을 담는다. 모양은 코드로 그린다.
//! 한 번 그리면 내용이 바뀌지 않으므로, 꽃핀이 움직이는 동안에는 창의 자리만 옮긴다.
//!
//! 보통은 머리 위에 뜨고 꼬리가 아래로 머리를 가리킨다. 머리 위에 자리가 없으면(화면 위쪽 끝에 가까우면)
//! 꽃핀 옆에 뜨고 꼬리가 옆으로 꽃핀을 가리킨다.

use crate::character_window::CharacterWindow;
use crate::geom::Rect;
use crate::text::{self, Label};

/// 길이는 모니터 배율 100%(DPI 96)일 때의 px다. 쓸 때 배율을 곱한다.
/// 글자가 차지할 수 있는 가장 큰 너비와, 내용의 줄 수.
const TEXT_WIDTH: f64 = 220.0;
const BODY_LINES: i32 = 3;
/// 글자와 테두리 사이의 여백(가로, 세로).
const PAD: (f64, f64) = (11.0, 8.0);
/// 모서리의 둥근 정도와 테두리 굵기.
const RADIUS: f64 = 9.0;
const BORDER: f64 = 1.5;
/// 꼬리의 길이와 밑변의 절반.
const TAIL: f64 = 9.0;
const TAIL_HALF: f64 = 7.0;
/// 꽃핀 옆에 뜰 때, 머리 꼭대기에서 이만큼 내려간 높이를 꼬리가 가리킨다.
const SIDE_DROP: f64 = 34.0;
/// 화면 끝에서 띄우는 거리.
const EDGE: f64 = 4.0;

/// 바탕, 테두리, 제목 글자, 내용 글자의 색(0xRRGGBB)과 바탕의 불투명도. 꽃핀의 분홍 꽃과 붉은 머리에 맞췄다.
const FILL: u32 = 0xFFFDF9;
const LINE: u32 = 0xE7879C;
const TITLE: u32 = 0xB23A55;
const BODY: u32 = 0x4A3A3D;
const OPACITY: f64 = 0.97;

/// 말풍선에 담을 글.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Speech {
    pub title: String,
    pub body: String,
}

/// 꼬리가 달린 쪽.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tail {
    /// 말풍선이 머리 위에 있고, 꼬리가 아래로 향한다.
    Down,
    /// 말풍선이 꽃핀의 오른쪽에 있고, 꼬리가 왼쪽으로 향한다.
    Left,
    /// 말풍선이 꽃핀의 왼쪽에 있고, 꼬리가 오른쪽으로 향한다.
    Right,
}

/// 말풍선을 놓을 자리.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    /// 둥근 네모의 왼쪽 위(화면 좌표).
    pub corner: (i32, i32),
    pub tail: Tail,
    /// 꼬리가 달린 변에서 꼬리 끝의 자리. 아래로 향하면 왼쪽에서, 옆으로 향하면 위에서 잰 거리다.
    pub tail_at: i32,
}

/// 배율을 곱한 길이들.
#[derive(Clone, Copy, Debug)]
struct Sizes {
    pad: (i32, i32),
    radius: f64,
    border: f64,
    tail: i32,
    tail_half: f64,
    side_drop: i32,
    edge: i32,
}

impl Sizes {
    fn at(dpi: u32) -> Sizes {
        let scale = dpi as f64 / 96.0;
        let px = |length: f64| (length * scale).round() as i32;
        Sizes {
            pad: (px(PAD.0), px(PAD.1)),
            radius: RADIUS * scale,
            border: BORDER * scale,
            tail: px(TAIL),
            tail_half: TAIL_HALF * scale,
            side_drop: px(SIDE_DROP),
            edge: px(EDGE),
        }
    }

    /// 꼬리 끝이 변의 양 끝에서 적어도 이만큼은 떨어져 있어야 둥근 모서리와 겹치지 않는다.
    fn tail_inset(&self) -> i32 {
        (self.radius + self.tail_half).ceil() as i32 + 2
    }
}

/// 말풍선을 놓을 자리를 정한다.
///
/// `head`는 꽃핀 머리 꼭대기의 가운데(화면 좌표), `half_body`는 몸 너비의 절반, `size`는 둥근 네모의 크기,
/// `bounds`는 말풍선이 벗어나면 안 되는 범위(모니터)다.
fn place(head: (i32, i32), half_body: i32, size: (i32, i32), sizes: &Sizes, bounds: Rect) -> Placement {
    let (width, height) = size;
    let clamp = |value: i32, low: i32, high: i32| value.max(low).min(high.max(low));
    let (left, right) = (bounds.x + sizes.edge, bounds.right() - sizes.edge - width);
    let (top, bottom) = (bounds.y + sizes.edge, bounds.bottom() - sizes.edge - height);
    let inset = sizes.tail_inset();

    // 머리 위에 자리가 있으면 거기에 둔다. 화면 옆 끝에 닿으면 말풍선은 안쪽으로 밀리고 꼬리만 머리를 따라간다.
    let above = head.1 - sizes.tail - height;
    if above >= top {
        let x = clamp(head.0 - width / 2, left, right);
        return Placement { corner: (x, above), tail: Tail::Down, tail_at: clamp(head.0 - x, inset, width - inset) };
    }
    // 없으면 꽃핀 옆에 둔다. 오른쪽이 먼저고, 오른쪽에 자리가 없으면 왼쪽이다.
    let aim = head.1 + sizes.side_drop;
    let y = clamp(aim - height / 2, top, bottom);
    let tail_at = clamp(aim - y, inset, height - inset);
    let on_right = head.0 + half_body + sizes.tail;
    if on_right <= right {
        Placement { corner: (on_right, y), tail: Tail::Left, tail_at }
    } else {
        let x = clamp(head.0 - half_body - sizes.tail - width, left, right);
        Placement { corner: (x, y), tail: Tail::Right, tail_at }
    }
}

/// 점에서 둥근 네모의 테두리까지의 거리. 안쪽이면 음수다. 네모는 `(0, 0)`에서 `(w, h)`까지다.
fn round_box_distance(p: (f64, f64), w: f64, h: f64, radius: f64) -> f64 {
    let (qx, qy) = ((p.0 - w / 2.0).abs() - (w / 2.0 - radius), (p.1 - h / 2.0).abs() - (h / 2.0 - radius));
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius
}

/// 점에서 세모의 변까지의 거리. 안쪽이면 음수다.
fn triangle_distance(p: (f64, f64), corners: [(f64, f64); 3]) -> f64 {
    let [a, b, c] = corners;
    let winding = ((b.0 - a.0) * (a.1 - c.1) - (b.1 - a.1) * (a.0 - c.0)).signum();
    let (mut nearest, mut inside) = (f64::MAX, f64::MAX);
    for (from, to) in [(a, b), (b, c), (c, a)] {
        let (edge, to_p) = ((to.0 - from.0, to.1 - from.1), (p.0 - from.0, p.1 - from.1));
        let along = ((to_p.0 * edge.0 + to_p.1 * edge.1) / (edge.0 * edge.0 + edge.1 * edge.1)).clamp(0.0, 1.0);
        let (dx, dy) = (to_p.0 - edge.0 * along, to_p.1 - edge.1 * along);
        nearest = nearest.min(dx * dx + dy * dy);
        // 세 변 모두의 안쪽에 있어야 세모의 안쪽이다.
        inside = inside.min(winding * (to_p.0 * edge.1 - to_p.1 * edge.0));
    }
    -nearest.sqrt() * inside.signum()
}

/// 색 `color`(0xRRGGBB)를 불투명도 `alpha`(0~1)로 칠한 픽셀. 미리 곱한 알파의 0xAARRGGBB다.
fn pixel(color: (f64, f64, f64), alpha: f64) -> u32 {
    let part = |value: f64| (value * alpha).round().clamp(0.0, 255.0) as u32;
    ((alpha * 255.0).round() as u32) << 24 | part(color.0) << 16 | part(color.1) << 8 | part(color.2)
}

fn channels(color: u32) -> (f64, f64, f64) {
    (((color >> 16) & 0xFF) as f64, ((color >> 8) & 0xFF) as f64, (color & 0xFF) as f64)
}

fn mix(from: (f64, f64, f64), to: (f64, f64, f64), amount: f64) -> (f64, f64, f64) {
    (from.0 + (to.0 - from.0) * amount, from.1 + (to.1 - from.1) * amount, from.2 + (to.2 - from.2) * amount)
}

/// 말풍선을 창 버퍼에 그린다. 버퍼는 둥근 네모의 왼쪽과 오른쪽, 아래에 꼬리 길이만큼의 여백이 있다.
///
/// 가장자리는 거리로 부드럽게 한다. 테두리에서 반 픽셀 안쪽이면 다 칠하고, 반 픽셀 바깥이면 칠하지 않는다.
fn paint(pixels: &mut [u32], size: (i32, i32), label: &Label, sizes: &Sizes, tail: Tail, tail_at: i32) {
    let (w, h) = (size.0 as f64, size.1 as f64);
    let (reach, half, at) = (sizes.tail as f64, sizes.tail_half, tail_at as f64);
    // 꼬리의 밑변은 둥근 네모 안으로 조금 들어가 있다. 그래야 이은 자리에 테두리 선이 생기지 않는다.
    let sunk = sizes.border + 2.0;
    let corners = match tail {
        Tail::Down => [(at - half, h - sunk), (at + half, h - sunk), (at, h + reach)],
        Tail::Left => [(sunk, at - half), (sunk, at + half), (-reach, at)],
        Tail::Right => [(w - sunk, at - half), (w - sunk, at + half), (w + reach, at)],
    };
    let (fill, line) = (channels(FILL), channels(LINE));
    let (title, body) = (channels(TITLE), channels(BODY));
    let width = size.0 + 2 * sizes.tail;
    let rows = size.1 + sizes.tail;
    for y in 0..rows {
        for x in 0..width {
            // 둥근 네모의 왼쪽 위를 원점으로 한 픽셀 가운데.
            let p = ((x - sizes.tail) as f64 + 0.5, y as f64 + 0.5);
            let distance = round_box_distance(p, w, h, sizes.radius).min(triangle_distance(p, corners));
            let shape = (0.5 - distance).clamp(0.0, 1.0);
            let slot = &mut pixels[(y * width + x) as usize];
            if shape <= 0.0 {
                *slot = 0;
                continue;
            }
            // 테두리 굵기보다 안쪽이면 바탕색이다.
            let mut color = mix(line, fill, (0.5 - (distance + sizes.border)).clamp(0.0, 1.0));
            let (tx, ty) = (x - sizes.tail - sizes.pad.0, y - sizes.pad.1);
            if tx >= 0 && ty >= 0 && tx < label.width && ty < label.height {
                let cover = label.coverage[(ty * label.width + tx) as usize] as f64 / 255.0;
                let ink = if ty < label.title_height { title } else { body };
                color = mix(color, ink, cover);
            }
            *slot = pixel(color, shape * OPACITY);
        }
    }
}

/// 화면에 떠 있는 말풍선.
pub struct Bubble {
    window: CharacterWindow,
    label: Label,
    sizes: Sizes,
    /// 둥근 네모의 크기.
    size: (i32, i32),
    /// 지금 그려져 있는 꼬리. 달라지면 다시 그린다.
    drawn: Option<(Tail, i32)>,
}

impl Bubble {
    /// 말풍선을 만든다. 아직 화면에 보이지 않는다. [`Bubble::follow`]를 부르면 보인다.
    ///
    /// `dpi`는 말풍선이 뜰 모니터의 DPI이고, `topmost`면 다른 창들보다 늘 위에 있다.
    pub fn new(speech: &Speech, dpi: u32, topmost: bool) -> Bubble {
        let sizes = Sizes::at(dpi);
        let max_width = (TEXT_WIDTH * dpi as f64 / 96.0).round() as i32;
        let label = text::label(&speech.title, &speech.body, dpi, max_width, BODY_LINES);
        // 꼬리를 달 수 있을 만큼은 넓고 높아야 한다.
        let least = 2 * sizes.tail_inset();
        let size = ((label.width + 2 * sizes.pad.0).max(least), (label.height + 2 * sizes.pad.1).max(least));
        let window = CharacterWindow::new(size.0 + 2 * sizes.tail, size.1 + sizes.tail, 0, 0, topmost);
        Bubble { window, label, sizes, size, drawn: None }
    }

    pub fn window(&self) -> &CharacterWindow {
        &self.window
    }

    /// 꽃핀의 머리를 따라간다. 꼬리가 달린 자리가 달라졌을 때만 다시 그리고, 아니면 창만 옮긴다.
    ///
    /// `head`는 머리 꼭대기의 가운데, `half_body`는 몸 너비의 절반, `bounds`는 벗어나면 안 되는 범위다.
    pub fn follow(&mut self, head: (i32, i32), half_body: i32, bounds: Rect) {
        let spot = place(head, half_body, self.size, &self.sizes, bounds);
        let mut dirty = Rect::EMPTY;
        if self.drawn != Some((spot.tail, spot.tail_at)) {
            paint(self.window.pixels(), self.size, &self.label, &self.sizes, spot.tail, spot.tail_at);
            self.drawn = Some((spot.tail, spot.tail_at));
            dirty = Rect::new(0, 0, self.window.width(), self.window.height());
        }
        // 창은 둥근 네모보다 왼쪽으로 꼬리 길이만큼 넓다.
        self.window.move_to(spot.corner.0 - self.sizes.tail, spot.corner.1);
        self.window.present(dirty);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect::new(0, 0, 1920, 1040);

    fn sizes() -> Sizes {
        Sizes::at(96)
    }

    #[test]
    fn sits_above_the_head_with_the_tail_on_the_head() {
        let spot = place((1000, 600), 60, (200, 70), &sizes(), SCREEN);
        // 가로로는 머리 가운데에 맞추고, 꼬리 끝이 머리 꼭대기에 닿는다.
        assert_eq!(spot, Placement { corner: (900, 600 - 9 - 70), tail: Tail::Down, tail_at: 100 });
    }

    #[test]
    fn stays_on_the_screen_and_the_tail_still_points_at_the_head() {
        // 꽃핀이 화면 왼쪽 끝에 있다. 말풍선은 안쪽으로 밀리고 꼬리가 머리 쪽으로 간다.
        let spot = place((40, 600), 60, (200, 70), &sizes(), SCREEN);
        assert_eq!((spot.corner.0, spot.tail, spot.tail_at), (4, Tail::Down, 36));
        // 더 끝으로 가면 꼬리는 둥근 모서리 앞에서 멈춘다.
        let spot = place((6, 600), 60, (200, 70), &sizes(), SCREEN);
        assert_eq!((spot.corner.0, spot.tail_at), (4, sizes().tail_inset()));
        // 오른쪽 끝도 같다.
        let spot = place((1900, 600), 60, (200, 70), &sizes(), SCREEN);
        assert_eq!((spot.corner.0, spot.tail_at), (1920 - 4 - 200, 182));
    }

    #[test]
    fn moves_to_the_side_when_there_is_no_room_above() {
        // 머리가 화면 위쪽 끝에서 60px 아래에 있다. 높이 70인 말풍선이 위에 들어가지 않는다.
        let spot = place((1000, 60), 60, (200, 70), &sizes(), SCREEN);
        assert_eq!((spot.corner, spot.tail), ((1000 + 60 + 9, 60 + 34 - 35), Tail::Left));
        assert_eq!(spot.tail_at, 35);
        // 오른쪽에 자리가 없으면 왼쪽에 뜬다.
        let spot = place((1800, 60), 60, (200, 70), &sizes(), SCREEN);
        assert_eq!((spot.corner.0, spot.tail), (1800 - 60 - 9 - 200, Tail::Right));
        // 아주 위쪽이면 화면 안으로 내려오고, 꼬리가 머리 쪽으로 올라간다.
        let spot = place((1000, 0), 60, (200, 70), &sizes(), SCREEN);
        assert_eq!((spot.corner.1, spot.tail_at), (4, 30));
    }

    #[test]
    fn distances_are_negative_inside_and_positive_outside() {
        assert!((round_box_distance((50.0, 20.0), 100.0, 40.0, 8.0) + 20.0).abs() < 1e-9);
        assert!((round_box_distance((110.0, 20.0), 100.0, 40.0, 8.0) - 10.0).abs() < 1e-9);
        // 모서리는 둥글다. 네모의 꼭짓점은 바깥이다.
        assert!(round_box_distance((0.5, 0.5), 100.0, 40.0, 8.0) > 0.0);
        let triangle = [(0.0, 0.0), (10.0, 0.0), (5.0, 10.0)];
        assert!(triangle_distance((5.0, 3.0), triangle) < 0.0);
        assert!((triangle_distance((5.0, -4.0), triangle) - 4.0).abs() < 1e-9);
        // 꼭짓점을 반대 순서로 줘도 같다.
        let reversed = [(5.0, 10.0), (10.0, 0.0), (0.0, 0.0)];
        assert!(triangle_distance((5.0, 3.0), reversed) < 0.0 && triangle_distance((5.0, -4.0), reversed) > 0.0);
    }

    /// 그린 말풍선에서 픽셀 하나의 불투명도(0~255).
    fn alpha(pixels: &[u32], width: i32, x: i32, y: i32) -> u32 {
        pixels[(y * width + x) as usize] >> 24
    }

    #[test]
    fn the_painted_shape_is_a_box_with_a_tail() {
        let sizes = sizes();
        let label = Label { width: 1, height: 1, title_height: 0, coverage: vec![0] };
        let size = (120, 50);
        let (width, rows) = (size.0 + 2 * sizes.tail, size.1 + sizes.tail);
        let mut pixels = vec![0xDEAD_BEEFu32; (width * rows) as usize];
        paint(&mut pixels, size, &label, &sizes, Tail::Down, 60);
        let at = |x: i32, y: i32| alpha(&pixels, width, x, y);
        // 가운데는 거의 불투명하고, 네모 밖(왼쪽 여백, 모서리)은 비어 있다.
        assert_eq!(at(sizes.tail + 60, 25), (OPACITY * 255.0).round() as u32);
        assert_eq!((at(2, 25), at(sizes.tail, 0), at(width - 1, rows - 1)), (0, 0, 0));
        // 꼬리는 네모 아래로 내려와 있고, 꼬리 끝의 옆은 비어 있다.
        assert!(at(sizes.tail + 60, size.1 + 3) > 200);
        assert_eq!(at(sizes.tail + 60 - 8, size.1 + 6), 0);
        // 테두리는 분홍, 안쪽은 흰색에 가깝다(미리 곱한 값이라 불투명도만큼 어둡다).
        let middle = pixels[(25 * width + sizes.tail + 60) as usize];
        let edge = pixels[(25 * width + sizes.tail) as usize];
        assert!((middle >> 8) & 0xFF > 240 && (edge >> 8) & 0xFF < 200, "{middle:08X} {edge:08X}");

        // 꼬리를 옆에 달면 그쪽 여백으로 나온다.
        paint(&mut pixels, size, &label, &sizes, Tail::Left, 25);
        let at = |x: i32, y: i32| alpha(&pixels, width, x, y);
        assert!(at(sizes.tail - 3, 25) > 200 && at(sizes.tail + 60, size.1 + 3) == 0);
        paint(&mut pixels, size, &label, &sizes, Tail::Right, 25);
        let at = |x: i32, y: i32| alpha(&pixels, width, x, y);
        assert!(at(sizes.tail + size.0 + 2, 25) > 200 && at(sizes.tail - 3, 25) == 0);
    }

    #[test]
    fn letters_are_painted_in_their_colours() {
        let sizes = sizes();
        // 글자 그림: 위 두 줄은 제목, 아래 두 줄은 내용이고 모두 꽉 덮여 있다.
        let label = Label { width: 4, height: 4, title_height: 2, coverage: vec![255; 16] };
        let size = (60, 40);
        let width = size.0 + 2 * sizes.tail;
        let mut pixels = vec![0u32; (width * (size.1 + sizes.tail)) as usize];
        paint(&mut pixels, size, &label, &sizes, Tail::Down, 30);
        let colour = |x: i32, y: i32| pixels[((sizes.pad.1 + y) * width + sizes.tail + sizes.pad.0 + x) as usize];
        let expect = |ink: u32| pixel(channels(ink), OPACITY);
        assert_eq!((colour(0, 0), colour(3, 1)), (expect(TITLE), expect(TITLE)));
        assert_eq!((colour(0, 2), colour(3, 3)), (expect(BODY), expect(BODY)));
        // 글자 그림 밖은 바탕색이다.
        assert_eq!(colour(4, 0), expect(FILL));
    }
}
