//! 그림 팩에서 장을 꺼내 창 버퍼에 그린다.
//!
//! 그림은 풀어 두지 않는다. 팩은 암호화된 채 exe 안에 그대로 있고, 장이 바뀔 때 그 장만 꺼낸다
//! (암호 풀기 → 압축 풀기 → 팔레트로 펼치기). 꺼내는 도구와 버퍼는 하나만 두고 모든 꽃핀이 함께 쓴다.

use std::ops::Range;
use std::time::Duration;

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

    /// 발바닥이 놓이는 점(칸 좌표). 이 점을 바닥 위의 서 있을 자리에 맞춘다.
    pub fn foot(&self) -> (i32, i32) {
        let (x, y) = self.pack.character().foot;
        (x as i32, y as i32)
    }

    /// 이름으로 동작을 찾아, 그 동작의 장 번호들을 재생 순서대로 돌려준다.
    pub fn motion_frames(&self, name: &str) -> Result<Range<usize>, PackError> {
        let id = self.pack.motion_id(name).ok_or_else(|| PackError::NoSuchMotion(name.to_string()))?;
        let motion = &self.pack.motions()[id as usize];
        Ok(motion.first_frame..motion.first_frame + motion.frame_count)
    }

    /// 장을 보여 줄 시간.
    pub fn duration(&self, frame: usize) -> Duration {
        Duration::from_millis(self.pack.frames()[frame].duration_ms as u64)
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
