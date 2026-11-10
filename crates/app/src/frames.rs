//! 그림 팩에서 장을 꺼내 창 버퍼에 그린다.
//!
//! 그림은 풀어 두지 않는다. 팩은 암호화된 채 exe 안에 그대로 있고, 장이 바뀔 때 그 장만 꺼낸다
//! (암호 풀기 → 압축 풀기 → 팔레트로 펼치기). 꺼내는 도구와 버퍼는 하나만 두고 모든 꽃핀이 함께 쓴다.

use dfp_pack::{FrameDecoder, Pack, PackError};

use crate::geom::Rect;
use crate::scale::{Scale, Scaler};

/// 빌드할 때 `build.rs`가 골라 넣은 팩. 진짜 그림 팩이거나 대체 그림 팩이다.
static PACK_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/pack.dfpk"));

pub struct Frames {
    pack: Pack<'static>,
    decoder: FrameDecoder,
    /// 지금 꺼낸 장의 팔레트 번호. 한 칸을 다 채워도 들어가는 크기로 한 번만 잡는다.
    indices: Vec<u8>,
    /// 크기를 바꿔 그리는 도구. 쓰는 배율마다 하나씩 두고 모든 꽃핀이 함께 쓴다. 1배는 도구 없이 그린다.
    scalers: Vec<Scaler>,
}

impl Frames {
    /// exe에 들어 있는 팩을 연다.
    pub fn load() -> Result<Frames, PackError> {
        let (pack, decoder) = crate::key::with_pack_key(|key| Pack::open(PACK_BYTES, key))?;
        let cell = pack.character().cell as usize;
        Ok(Frames { pack, decoder, indices: vec![0; cell * cell], scalers: Vec::new() })
    }

    pub fn pack(&self) -> &Pack<'static> {
        &self.pack
    }

    /// `frame`번 장의 윗부분을 한 변이 `size`인 작은 그림으로 줄인다. 트레이 아이콘으로 쓴다.
    ///
    /// 장의 너비만큼을 위에서부터 정사각형으로 잘라 쓴다. 서 있는 그림이면 얼굴이 들어온다.
    /// 결과는 미리 곱한 알파의 BGRA다.
    pub fn portrait(&mut self, frame: usize, size: usize) -> Result<Vec<u32>, PackError> {
        self.decoder.decode(&self.pack, frame, &mut self.indices)?;
        let info = &self.pack.frames()[frame];
        let palette = &self.pack.motions()[info.motion as usize].palette;
        let (w, h) = (info.w as usize, info.h as usize);
        let side = w.min(h);
        let left = (w - side) / 2;
        let mut out = vec![0u32; size * size];
        for (i, pixel) in out.iter_mut().enumerate() {
            // 작은 그림의 픽셀 하나가 덮는 원본 범위를 평균한다.
            let (ox, oy) = (i % size, i / size);
            let (x0, x1) = (left + ox * side / size, left + ((ox + 1) * side).div_ceil(size));
            let (y0, y1) = (oy * side / size, ((oy + 1) * side).div_ceil(size));
            let mut sum = [0u32; 4];
            for y in y0..y1 {
                for &index in &self.indices[y * w + x0..y * w + x1] {
                    let color = palette[index as usize];
                    for (channel, total) in sum.iter_mut().enumerate() {
                        *total += (color >> (channel * 8)) & 255;
                    }
                }
            }
            let count = ((x1 - x0) * (y1 - y0)) as u32;
            let mean = sum.map(|total| (total + count / 2) / count);
            *pixel = mean[3] << 24 | mean[2] << 16 | mean[1] << 8 | mean[0];
        }
        Ok(out)
    }

    /// 장이 칸 안에서 차지하는 범위.
    fn rect(&self, frame: usize) -> Rect {
        let f = &self.pack.frames()[frame];
        Rect::new(f.x as i32, f.y as i32, f.w as i32, f.h as i32)
    }

    /// 모든 장을 겹쳤을 때 화면 칸(칸을 `scale`배 한 것)에서 차지하는 범위. 꽃핀 창은 이 크기로 만든다.
    pub fn bounds(&mut self, scale: Scale) -> Rect {
        let bounds = (0..self.pack.frames().len()).fold(Rect::EMPTY, |all, i| all.union(self.rect(i)));
        if scale.is_one() { bounds } else { scaler(&mut self.scalers, &self.pack, scale).map(bounds) }
    }

    /// `frame`번 장을 꺼내 `scale`배로 `dst`(한 줄이 `dst_width`개인 창 버퍼)에 그리고, 그린 범위를
    /// 창 좌표로 돌려준다.
    ///
    /// `origin`은 창 버퍼의 왼쪽 위가 화면 칸의 어느 점인지다. 장은 `origin`에서 시작하는 창 안에
    /// 다 들어와야 한다(창을 [`Frames::bounds`] 크기로 만들면 그렇다).
    pub fn draw(
        &mut self,
        frame: usize,
        scale: Scale,
        dst: &mut [u32],
        dst_width: i32,
        origin: (i32, i32),
    ) -> Result<Rect, PackError> {
        self.decoder.decode(&self.pack, frame, &mut self.indices)?;
        let info = &self.pack.frames()[frame];
        let palette = &self.pack.motions()[info.motion as usize].palette;
        if !scale.is_one() {
            let src = Rect::new(info.x as i32, info.y as i32, info.w as i32, info.h as i32);
            let scaler = scaler(&mut self.scalers, &self.pack, scale);
            return Ok(scaler.draw(&self.indices, src, palette, dst, dst_width, origin));
        }
        // 1배는 섞을 것이 없으므로 팔레트 색을 그대로 옮긴다.
        let (w, h) = (info.w as usize, info.h as usize);
        let rect = Rect::new(info.x as i32 - origin.0, info.y as i32 - origin.1, w as i32, h as i32);
        for row in 0..h {
            let start = ((rect.y + row as i32) * dst_width + rect.x) as usize;
            let line = &mut dst[start..start + w];
            for (pixel, &index) in line.iter_mut().zip(&self.indices[row * w..(row + 1) * w]) {
                *pixel = palette[index as usize];
            }
        }
        Ok(rect)
    }
}

/// `scale`배로 그리는 도구를 찾아 돌려준다. 없으면 만든다.
fn scaler<'s>(scalers: &'s mut Vec<Scaler>, pack: &Pack<'_>, scale: Scale) -> &'s mut Scaler {
    let at = scalers.iter().position(|s| s.scale() == scale).unwrap_or_else(|| {
        scalers.push(Scaler::new(pack.character().cell as i32, scale));
        scalers.len() - 1
    });
    &mut scalers[at]
}
