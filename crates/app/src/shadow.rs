//! 발밑 그림자. 그림에는 그림자가 없어서 코드로 그린다.
//!
//! 바닥의 둥근 그림자를 비스듬히 내려다본 모양이라 납작한 타원이다. 가운데가 가장 진하고 가장자리로
//! 갈수록 옅어진다. 모양은 원본 데모의 것과 같다. 꽃핀 그림 밑에 깔리도록, 이미 그려진 그림의
//! 투명한 곳과 반투명한 곳에만 섞어 넣는다.

use crate::geom::Rect;

/// 그림자의 크기(그림 1배 기준 px).
pub const WIDTH: f64 = 58.0;
pub const HEIGHT: f64 = 20.0;
/// 그림자의 색(R, G, B).
const COLOR: (u32, u32, u32) = (28, 32, 42);
/// 진하기. 가운데에서 0.42, 반지름의 60%에서 0.22, 가장자리에서 0이다.
const CENTER: f64 = 0.42;
const MIDDLE: (f64, f64) = (0.6, 0.22);

pub struct Shadow {
    width: i32,
    height: i32,
    /// 픽셀마다의 진하기(0~255). 한 줄이 `width`개다.
    alpha: Vec<u8>,
}

impl Shadow {
    /// `scale`배로 보여 줄 그림자를 만든다.
    pub fn new(scale: f64) -> Shadow {
        let width = (WIDTH * scale).round().max(1.0) as i32;
        let height = (HEIGHT * scale).round().max(1.0) as i32;
        let mut alpha = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                // 가운데에서 가장자리까지를 0~1로 본 거리.
                let dx = (x as f64 + 0.5) / width as f64 * 2.0 - 1.0;
                let dy = (y as f64 + 0.5) / height as f64 * 2.0 - 1.0;
                let r = (dx * dx + dy * dy).sqrt();
                let strength = if r < MIDDLE.0 {
                    CENTER + (MIDDLE.1 - CENTER) * r / MIDDLE.0
                } else {
                    MIDDLE.1 * (1.0 - (r - MIDDLE.0) / (1.0 - MIDDLE.0)).max(0.0)
                };
                alpha.push((strength * 255.0).round() as u8);
            }
        }
        Shadow { width, height, alpha }
    }

    /// 가운데가 `center`에 올 때 그림자가 차지하는 범위.
    pub fn rect(&self, center: (i32, i32)) -> Rect {
        Rect::new(center.0 - self.width / 2, center.1 - self.height / 2, self.width, self.height)
    }

    /// 그림자를 `pixels`(한 줄이 `width`개, 높이 `height`)에 이미 그려진 그림 밑에 깐다.
    ///
    /// `level`은 진하기(0~255)다. 버퍼 밖으로 나가는 부분은 그리지 않는다. 그린 범위를 돌려준다.
    pub fn draw_under(&self, pixels: &mut [u32], width: i32, height: i32, center: (i32, i32), level: u8) -> Rect {
        let whole = self.rect(center);
        let x0 = whole.x.max(0);
        let y0 = whole.y.max(0);
        let (x1, y1) = (whole.right().min(width), whole.bottom().min(height));
        if level == 0 || x0 >= x1 || y0 >= y1 {
            return Rect::EMPTY;
        }
        for y in y0..y1 {
            let from = ((y - whole.y) * self.width + (x0 - whole.x)) as usize;
            let to = (y * width + x0) as usize;
            let row = &self.alpha[from..from + (x1 - x0) as usize];
            for (pixel, &strength) in pixels[to..to + row.len()].iter_mut().zip(row) {
                let over = *pixel;
                let seen_through = 255 - (over >> 24);
                if seen_through == 0 {
                    continue;
                }
                // 그림자의 알파. 위에 덮인 그림이 가린 만큼은 보이지 않는다.
                let a = strength as u32 * level as u32 / 255 * seen_through / 255;
                let shade = |color: u32| color * a / 255;
                *pixel = over + (a << 24 | shade(COLOR.0) << 16 | shade(COLOR.1) << 8 | shade(COLOR.2));
            }
        }
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn darkest_in_the_middle_and_clear_at_the_rim() {
        let shadow = Shadow::new(1.0);
        assert_eq!((shadow.width, shadow.height), (58, 20));
        let at = |x: i32, y: i32| shadow.alpha[(y * shadow.width + x) as usize];
        assert!(at(29, 10) > 100 && at(29, 10) <= 107);
        assert!(at(29, 10) > at(15, 10) && at(15, 10) > at(3, 10));
        // 타원 밖의 모서리는 비어 있다.
        assert_eq!(at(0, 0), 0);
        assert_eq!(at(57, 19), 0);
    }

    #[test]
    fn size_follows_the_scale() {
        assert_eq!((Shadow::new(1.5).width, Shadow::new(1.5).height), (87, 30));
        assert_eq!((Shadow::new(0.75).width, Shadow::new(0.75).height), (44, 15));
    }

    #[test]
    fn goes_under_what_is_already_drawn() {
        let shadow = Shadow::new(1.0);
        let (width, height) = (80, 40);
        let mut pixels = vec![0u32; (width * height) as usize];
        let middle = (20 * width + 40) as usize;
        pixels[middle] = 0xFF11_2233; // 불투명한 그림
        pixels[middle + 1] = 0x8040_0000; // 반투명한 그림
        let drawn = shadow.draw_under(&mut pixels, width, height, (40, 20), 255);
        assert_eq!(drawn, Rect::new(11, 10, 58, 20));
        // 불투명한 픽셀은 그대로다.
        assert_eq!(pixels[middle], 0xFF11_2233);
        // 반투명한 픽셀은 그림자가 비쳐 더 불투명해지고, 그림의 색은 남는다.
        assert!(pixels[middle + 1] >> 24 > 0x80 && (pixels[middle + 1] >> 16) & 255 >= 0x40);
        // 빈 곳에는 그림자만 들어간다. 색은 알파를 미리 곱한 값이라 알파보다 클 수 없다.
        let bare = pixels[middle + 2];
        let (a, r, g, b) = (bare >> 24, (bare >> 16) & 255, (bare >> 8) & 255, bare & 255);
        assert!(a > 90 && r <= a && g <= a && b <= a && b > r);
        // 그림자 범위 밖은 건드리지 않는다.
        assert_eq!(pixels[0], 0);
    }

    #[test]
    fn level_makes_it_fainter() {
        let shadow = Shadow::new(1.0);
        let draw = |level: u8| {
            let mut pixels = vec![0u32; 80 * 40];
            shadow.draw_under(&mut pixels, 80, 40, (40, 20), level);
            pixels[20 * 80 + 40] >> 24
        };
        assert!(draw(255) > draw(128) && draw(128) > draw(10));
        assert_eq!(draw(0), 0);
    }

    #[test]
    fn clipped_at_the_buffer_edge() {
        let shadow = Shadow::new(1.0);
        let mut pixels = vec![0u32; 80 * 40];
        // 가운데가 버퍼의 아래 끝에 걸치면 위쪽 절반만 그린다.
        assert_eq!(shadow.draw_under(&mut pixels, 80, 40, (40, 40), 255), Rect::new(11, 30, 58, 10));
        // 완전히 벗어나면 아무것도 그리지 않는다.
        assert_eq!(shadow.draw_under(&mut pixels, 80, 40, (40, 80), 255), Rect::EMPTY);
    }
}
