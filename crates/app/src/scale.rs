//! 그림을 표시 배율에 맞춰 줄이고 키운다.
//!
//! 방법은 면적 평균이다. 화면 픽셀 하나가 원본에서 덮는 네모를 구하고, 그 안에 든 원본 픽셀을
//! 덮인 넓이만큼 섞는다. 키울 때는 원본 픽셀이 또렷하게 남고 픽셀 경계만 부드러워지며, 2배처럼
//! 딱 떨어지는 배율에서는 픽셀을 그대로 늘린 것과 같다. 줄일 때는 원본 픽셀을 하나도 빠뜨리지 않는다.
//! 최근접(경계가 들쭉날쭉함), 양선형(흐려짐), 바이큐빅(또렷하지만 몇 배 느림)과 견줘 골랐다.
//!
//! 섞는 자리는 늘 칸(256×256) 기준으로 잡는다. 장은 칸 안의 일부만 잘라 담고 있지만, 잘린 범위가
//! 장마다 달라도 같은 원본 픽셀은 늘 같은 화면 픽셀로 간다. 그래서 장이 바뀔 때 그림이 흔들리지 않는다.

use crate::geom::Rect;

/// 표시 배율. 원본의 `num / den`배로 보여 준다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scale {
    num: i32,
    den: i32,
}

impl Scale {
    /// 사용자가 고른 크기(퍼센트)와 모니터 DPI로 배율을 만든다. DPI 96이 Windows 배율 100%다.
    ///
    /// 절반보다 작게는 줄이지 않는다(섞는 원본 픽셀 수를 한 방향에 [`TAPS`]개로 묶어 두었다).
    pub fn new(size_percent: u32, dpi: u32) -> Scale {
        let (num, den) = (size_percent.clamp(50, 400) * dpi.clamp(96, 960), 100 * 96);
        let divisor = gcd(num, den);
        Scale { num: (num / divisor) as i32, den: (den / divisor) as i32 }
    }

    pub fn is_one(self) -> bool {
        self.num == self.den
    }

    /// 배율을 소수로 돌려준다. 길이와 빠르기를 계산할 때 쓴다.
    pub fn factor(self) -> f64 {
        self.num as f64 / self.den as f64
    }

    /// 원본 칸의 자리(픽셀 경계)를 화면 칸의 자리로 바꾼다. 가장 가까운 픽셀 경계로 맞춘다.
    pub fn point(self, v: i32) -> i32 {
        (2 * v * self.num + self.den).div_euclid(2 * self.den)
    }
}

impl std::fmt::Display for Scale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.num, self.den)
    }
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// 화면 픽셀 하나를 만드는 데 섞는 원본 픽셀 수(한 방향). 배율이 절반 이상이면 셋을 넘지 않는다.
const TAPS: usize = 3;

/// 한 방향에서 화면 픽셀 하나를 만드는 원본 픽셀들.
#[derive(Clone, Copy, Debug)]
struct Tap {
    /// 섞는 첫 원본 픽셀. 여기서부터 [`TAPS`]개에 `weights`를 곱해 더한다.
    first: i32,
    /// 무게가 0이 아닌 마지막 원본 픽셀.
    last: i32,
    /// 합은 256(= 1배).
    weights: [u64; TAPS],
}

/// 길이 `cell`인 원본 한 방향을 배율만큼 바꿀 때, 화면 픽셀마다 섞을 원본 픽셀을 적은 표.
fn build_axis(cell: i32, scale: Scale) -> Vec<Tap> {
    let Scale { num, den } = scale;
    let count = (cell * num + den - 1) / den;
    (0..count)
        .map(|d| {
            // 화면 픽셀 d가 덮는 원본 범위. 길이를 정수로 다루려고 1/num 픽셀 단위로 센다.
            let (from, to) = (d * den, (d + 1) * den);
            let first = from / num;
            let mut weights = [0i32; TAPS];
            let mut last = first;
            for (k, weight) in weights.iter_mut().enumerate() {
                let pixel = first + k as i32;
                let overlap = (to.min((pixel + 1) * num) - from.max(pixel * num)).max(0);
                *weight = (overlap * 256 + den / 2) / den;
                if *weight > 0 {
                    last = pixel;
                }
            }
            // 반올림으로 남거나 모자란 몫은 가장 무거운 자리에서 맞춘다.
            let heavy = (0..TAPS).max_by_key(|&k| weights[k]).unwrap_or(0);
            weights[heavy] += 256 - weights.iter().sum::<i32>();
            Tap { first, last, weights: weights.map(|w| w as u64) }
        })
        .collect()
}

/// 원본 범위 `[from, to)`의 픽셀이 하나라도 섞여 들어가는 화면 픽셀의 범위.
fn span(taps: &[Tap], from: i32, to: i32) -> (i32, i32) {
    let touches = |tap: &Tap| tap.last >= from && tap.first < to;
    let start = taps.iter().position(touches).unwrap_or(0);
    let end = taps.iter().rposition(touches).map_or(start, |i| i + 1);
    (start as i32, end as i32)
}

// 픽셀 하나의 네 값(B, G, R, A)을 64비트 정수에 16비트씩 띄워 놓으면, 곱셈 한 번으로 네 값에
// 같은 무게를 곱할 수 있다. 무게의 합이 256이라 16비트 칸을 넘치지 않는다.
const LANES: u64 = 0x00FF_00FF_00FF_00FF;
const HALF: u64 = 0x0080_0080_0080_0080;

fn spread(pixel: u32) -> u64 {
    let x = pixel as u64;
    let x = (x | (x << 16)) & 0x0000_FFFF_0000_FFFF;
    (x | (x << 8)) & LANES
}

fn compact(x: u64) -> u32 {
    let x = (x | (x >> 8)) & 0x0000_FFFF_0000_FFFF;
    (x | (x >> 16)) as u32
}

fn mix(pixels: [u64; TAPS], weights: [u64; TAPS]) -> u64 {
    ((pixels[0] * weights[0] + pixels[1] * weights[1] + pixels[2] * weights[2] + HALF) >> 8) & LANES
}

/// 정해진 배율 하나로 장을 그리는 도구. 표와 작업 버퍼를 갖고 있어서, 같은 배율이면 함께 쓴다.
pub struct Scaler {
    scale: Scale,
    /// 가로와 세로의 표. 칸이 정사각형이라 내용은 같지만, 읽기 쉽게 따로 둔다.
    xs: Vec<Tap>,
    ys: Vec<Tap>,
    /// 지금 그리는 장의 팔레트를 [`spread`]로 벌려 둔 것.
    palette: [u64; 256],
    /// 원본 한 줄을 팔레트로 펼친 것. 양옆에 [`TAPS`]만큼 투명한 여백을 둔다.
    line: Vec<u64>,
    /// 가로로 먼저 바꿔 둔 원본 줄들. [`TAPS`]줄을 돌려 쓴다.
    rows: Vec<u64>,
    /// `rows`의 각 줄에 지금 원본의 몇째 줄이 들어 있는지.
    row_of: [i32; TAPS],
}

impl Scaler {
    /// 한 변이 `cell`인 칸을 `scale`배로 그리는 도구를 만든다.
    pub fn new(cell: i32, scale: Scale) -> Scaler {
        let xs = build_axis(cell, scale);
        let ys = xs.clone();
        let rows = vec![0; xs.len() * TAPS];
        Scaler {
            scale,
            xs,
            ys,
            palette: [0; 256],
            line: vec![0; cell as usize + 2 * TAPS],
            rows,
            row_of: [i32::MIN; TAPS],
        }
    }

    pub fn scale(&self) -> Scale {
        self.scale
    }

    /// 원본 칸의 범위 `src`가 화면 칸에서 차지하는 범위.
    pub fn map(&self, src: Rect) -> Rect {
        let (x0, x1) = span(&self.xs, src.x, src.right());
        let (y0, y1) = span(&self.ys, src.y, src.bottom());
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }

    /// 칸 안의 `src`에 놓인 장을 배율에 맞춰 `dst`에 그리고, 그린 범위를 `dst` 좌표로 돌려준다.
    ///
    /// `indices`는 장의 팔레트 번호(한 줄이 `src.w`개), `dst`는 한 줄이 `dst_width`개인 버퍼다.
    /// `origin`은 `dst`의 왼쪽 위가 화면 칸의 어느 점인지다. 장 밖은 투명한 것으로 친다.
    pub fn draw(
        &mut self,
        indices: &[u8],
        src: Rect,
        palette: &[u32; 256],
        dst: &mut [u32],
        dst_width: i32,
        origin: (i32, i32),
    ) -> Rect {
        for (wide, &color) in self.palette.iter_mut().zip(palette) {
            *wide = spread(color);
        }
        let out = self.map(src);
        let (width, stride) = (out.w as usize, self.xs.len());
        self.row_of = [i32::MIN; TAPS];

        // 가로를 먼저 바꾸고 세로를 바꾼다. 가로로 바꾼 원본 줄은 화면 줄 여럿이 함께 쓴다.
        for y in out.y..out.bottom() {
            let tap = self.ys[y as usize];
            for row in tap.first..=tap.last {
                let slot = row.rem_euclid(TAPS as i32) as usize;
                if self.row_of[slot] != row {
                    self.fill_row(slot, row, indices, src, out);
                    self.row_of[slot] = row;
                }
            }
            // 무게가 0인 줄에는 예전 내용이 남아 있을 수 있지만, 0을 곱하므로 결과에 들어가지 않는다.
            let row = |k: i32| {
                let slot = (tap.first + k).rem_euclid(TAPS as i32) as usize;
                &self.rows[slot * stride..slot * stride + width]
            };
            let (a, b, c) = (row(0), row(1), row(2));
            let start = ((y - origin.1) * dst_width + out.x - origin.0) as usize;
            let target = &mut dst[start..start + width];
            if tap.first == tap.last {
                for (pixel, &a) in target.iter_mut().zip(a) {
                    *pixel = compact(a);
                }
            } else {
                for (((pixel, &a), &b), &c) in target.iter_mut().zip(a).zip(b).zip(c) {
                    *pixel = compact(mix([a, b, c], tap.weights));
                }
            }
        }
        Rect::new(out.x - origin.0, out.y - origin.1, out.w, out.h)
    }

    /// 원본의 `row`째 줄을 가로로 바꿔 `rows`의 `slot`번 줄에 둔다.
    fn fill_row(&mut self, slot: usize, row: i32, indices: &[u8], src: Rect, out: Rect) {
        let stride = self.xs.len();
        let target = &mut self.rows[slot * stride..slot * stride + out.w as usize];
        if row < src.y || row >= src.bottom() {
            target.fill(0);
            return;
        }
        let w = src.w as usize;
        let from = (row - src.y) as usize * w;
        self.line[..TAPS].fill(0);
        for (wide, &index) in self.line[TAPS..TAPS + w].iter_mut().zip(&indices[from..from + w]) {
            *wide = self.palette[index as usize];
        }
        self.line[TAPS + w..TAPS + w + TAPS].fill(0);
        for (pixel, tap) in target.iter_mut().zip(&self.xs[out.x as usize..]) {
            // `out`은 장을 건드리는 화면 픽셀만 담으므로, 섞는 자리는 여백 안쪽을 벗어나지 않는다.
            let base = (tap.first - src.x + TAPS as i32) as usize;
            let line = &self.line[base..base + TAPS];
            *pixel = mix([line[0], line[1], line[2]], tap.weights);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL: i32 = 16;

    /// 번호가 곧 색인 시험용 팔레트. 0번은 투명, 나머지는 불투명이다.
    fn palette() -> [u32; 256] {
        std::array::from_fn(|i| if i == 0 { 0 } else { 0xFF00_0000 | (i as u32 * 0x01_0203) })
    }

    /// 칸을 다 채운 장을 그린다.
    fn draw_cell(scaler: &mut Scaler, indices: &[u8]) -> (Vec<u32>, i32) {
        let side = scaler.xs.len() as i32;
        let mut dst = vec![0u32; (side * side) as usize];
        scaler.draw(indices, Rect::new(0, 0, CELL, CELL), &palette(), &mut dst, side, (0, 0));
        (dst, side)
    }

    fn scales() -> Vec<Scale> {
        [(75, 96), (100, 96), (125, 96), (75, 144), (100, 144), (125, 144), (100, 192), (125, 192), (100, 168)]
            .map(|(size, dpi)| Scale::new(size, dpi))
            .to_vec()
    }

    #[test]
    fn scale_is_reduced() {
        assert_eq!(Scale::new(100, 96), Scale { num: 1, den: 1 });
        assert!(Scale::new(100, 96).is_one());
        assert_eq!(Scale::new(75, 144), Scale { num: 9, den: 8 });
        assert_eq!(Scale::new(125, 192), Scale { num: 5, den: 2 });
        // 절반보다 작게는 줄이지 않는다.
        assert_eq!(Scale::new(10, 96), Scale { num: 1, den: 2 });
    }

    #[test]
    fn point_rounds_to_nearest() {
        assert_eq!(Scale::new(100, 144).point(128), 192);
        assert_eq!(Scale::new(75, 96).point(232), 174);
        assert_eq!(Scale::new(75, 96).point(1), 1); // 0.75 → 1
        assert_eq!(Scale::new(125, 96).point(1), 1); // 1.25 → 1
    }

    #[test]
    fn weights_add_up_and_taps_move_forward() {
        for scale in scales() {
            let taps = build_axis(256, scale);
            assert_eq!(taps.len() as i32, (256 * scale.num + scale.den - 1) / scale.den);
            for tap in &taps {
                assert_eq!(tap.weights.iter().sum::<u64>(), 256, "{scale}");
                assert!(tap.last - tap.first < TAPS as i32);
            }
            assert!(taps.windows(2).all(|w| w[0].first <= w[1].first && w[0].last <= w[1].last));
            // 원본 픽셀은 하나도 빠지지 않는다.
            assert_eq!(taps[0].first, 0);
            assert!(taps.windows(2).all(|w| w[1].first <= w[0].last + 1));
            assert!(taps[taps.len() - 1].last >= 255);
        }
    }

    #[test]
    fn same_size_copies_pixels() {
        let indices: Vec<u8> = (0..CELL * CELL).map(|i| (i % 251) as u8).collect();
        let (dst, side) = draw_cell(&mut Scaler::new(CELL, Scale::new(100, 96)), &indices);
        assert_eq!(side, CELL);
        let palette = palette();
        assert!(dst.iter().zip(&indices).all(|(&pixel, &index)| pixel == palette[index as usize]));
    }

    #[test]
    fn double_size_repeats_pixels() {
        let indices: Vec<u8> = (0..CELL * CELL).map(|i| (i % 251) as u8).collect();
        let (dst, side) = draw_cell(&mut Scaler::new(CELL, Scale::new(100, 192)), &indices);
        assert_eq!(side, CELL * 2);
        let palette = palette();
        for y in 0..side {
            for x in 0..side {
                let index = indices[((y / 2) * CELL + x / 2) as usize];
                assert_eq!(dst[(y * side + x) as usize], palette[index as usize]);
            }
        }
    }

    #[test]
    fn flat_color_stays_flat() {
        // 한 가지 색으로 채운 칸은 어떤 배율에서도 그 색 그대로여야 한다(반올림으로 색이 달라지지 않는다).
        let indices = vec![200u8; (CELL * CELL) as usize];
        for scale in scales() {
            let (dst, side) = draw_cell(&mut Scaler::new(CELL, scale), &indices);
            // 맨 끝 줄은 칸 밖(투명)과 섞일 수 있으므로 뺀다.
            for y in 0..side - 1 {
                for x in 0..side - 1 {
                    assert_eq!(dst[(y * side + x) as usize], palette()[200], "{scale} ({x}, {y})");
                }
            }
        }
    }

    #[test]
    fn cropped_frame_lands_on_the_same_pixels() {
        // 장을 잘라서 그린 결과는, 같은 그림을 칸 전체에 놓고 그린 결과와 픽셀 하나까지 같아야 한다.
        for scale in scales() {
            for src in [Rect::new(3, 5, 7, 6), Rect::new(0, 0, 4, 9), Rect::new(9, 2, 7, 14), Rect::new(5, 5, 1, 1)] {
                let cropped: Vec<u8> = (0..src.w * src.h).map(|i| (1 + i % 250) as u8).collect();
                let mut full = vec![0u8; (CELL * CELL) as usize];
                for row in 0..src.h {
                    let from = (row * src.w) as usize;
                    let to = ((src.y + row) * CELL + src.x) as usize;
                    full[to..to + src.w as usize].copy_from_slice(&cropped[from..from + src.w as usize]);
                }
                let mut scaler = Scaler::new(CELL, scale);
                let (expected, side) = draw_cell(&mut scaler, &full);
                let mut dst = vec![0u32; (side * side) as usize];
                let drawn = scaler.draw(&cropped, src, &palette(), &mut dst, side, (0, 0));
                assert_eq!(dst, expected, "{scale} {src:?}");
                assert_eq!(drawn, scaler.map(src));
                // 그렸다고 한 범위 밖에는 아무것도 없어야 한다.
                for y in 0..side {
                    for x in 0..side {
                        let inside = x >= drawn.x && x < drawn.right() && y >= drawn.y && y < drawn.bottom();
                        assert!(inside || expected[(y * side + x) as usize] == 0, "{scale} {src:?} ({x}, {y})");
                    }
                }
            }
        }
    }

    #[test]
    fn origin_shifts_the_drawing() {
        let scale = Scale::new(125, 144);
        let src = Rect::new(4, 6, 8, 7);
        let indices: Vec<u8> = (0..src.w * src.h).map(|i| (1 + i % 250) as u8).collect();
        let mut scaler = Scaler::new(CELL, scale);
        let out = scaler.map(src);
        // 장이 꼭 들어가는 크기의 버퍼에 그린다.
        let mut tight = vec![0u32; (out.w * out.h) as usize];
        let drawn = scaler.draw(&indices, src, &palette(), &mut tight, out.w, (out.x, out.y));
        assert_eq!(drawn, Rect::new(0, 0, out.w, out.h));
        let side = scaler.xs.len() as i32;
        let mut whole = vec![0u32; (side * side) as usize];
        scaler.draw(&indices, src, &palette(), &mut whole, side, (0, 0));
        for y in 0..out.h {
            for x in 0..out.w {
                assert_eq!(tight[(y * out.w + x) as usize], whole[((out.y + y) * side + out.x + x) as usize]);
            }
        }
    }

    #[test]
    fn half_covered_pixel_is_half_transparent() {
        // 1.5배에서 가운데 화면 픽셀은 원본 두 픽셀을 반씩 덮는다.
        let mut indices = vec![0u8; (CELL * CELL) as usize];
        indices[0] = 255; // 흰색에 가까운 불투명 픽셀 하나, 나머지는 투명
        let (dst, _) = draw_cell(&mut Scaler::new(CELL, Scale::new(100, 144)), &indices);
        assert_eq!(dst[0], palette()[255]);
        assert_eq!(dst[1] >> 24, 128);
        assert_eq!(dst[2], 0);
    }
}
