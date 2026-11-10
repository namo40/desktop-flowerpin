//! 그림 팩에서 장을 꺼내 창 버퍼에 그린다.
//!
//! 그림은 풀어 두지 않는다. 팩은 암호화된 채 exe 안에 그대로 있고, 장이 바뀔 때 그 장만 꺼낸다
//! (암호 풀기 → 압축 풀기 → 팔레트로 펼치기). 꺼내는 도구와 버퍼는 하나만 두고 모든 꽃핀이 함께 쓴다.

use dfp_pack::temp::{FrameDecoder, PackError, TempPack};

use crate::geom::Rect;

/// 그림 한 칸의 한 변 길이. 모든 장은 이 칸 안의 좌표로 자리가 정해져 있다.
pub const CELL: i32 = 256;

/// 임시 팩에서 idle2(웃는 얼굴로 서기)의 동작 번호. 임시 팩에는 동작 이름이 없어서 번호로 고른다.
pub const MOTION_IDLE2: u16 = 8;

#[cfg(dfp_has_pack)]
static PACK_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/pack.bin"));
#[cfg(dfp_has_pack)]
static PACK_KEY: &[u8; 32] = include_bytes!(concat!(env!("OUT_DIR"), "/key.bin"));

pub struct Frames {
    pack: TempPack<'static>,
    decoder: FrameDecoder,
    /// 지금 꺼낸 장의 팔레트 번호. 한 칸을 다 채워도 들어가는 크기로 한 번만 잡는다.
    indices: Vec<u8>,
}

impl Frames {
    /// exe에 들어 있는 팩을 연다. 팩 없이 빌드했으면 `None`이다.
    pub fn load() -> Result<Option<Frames>, PackError> {
        #[cfg(dfp_has_pack)]
        {
            let (pack, decoder) = TempPack::open(PACK_BYTES, PACK_KEY)?;
            Ok(Some(Frames { pack, decoder, indices: vec![0; (CELL * CELL) as usize] }))
        }
        #[cfg(not(dfp_has_pack))]
        {
            Ok(None)
        }
    }

    /// 한 동작의 장 번호들을 재생 순서대로 돌려준다.
    pub fn motion_frames(&self, motion: u16) -> Vec<usize> {
        (0..self.pack.frames().len()).filter(|&i| self.pack.frames()[i].motion == motion).collect()
    }

    /// 장이 칸 안에서 차지하는 범위.
    pub fn rect(&self, frame: usize) -> Rect {
        let f = &self.pack.frames()[frame];
        Rect::new(f.x as i32, f.y as i32, f.w as i32, f.h as i32)
    }

    /// 모든 장을 겹쳤을 때 칸 안에서 차지하는 범위. 꽃핀 창은 이 크기로 만든다.
    pub fn bounds(&self) -> Rect {
        (0..self.pack.frames().len()).fold(Rect::EMPTY, |all, i| all.union(self.rect(i)))
    }

    /// `frame`번 장을 꺼내 `dst`(한 줄이 `dst_width`개인 창 버퍼)에 그리고, 그린 범위를 창 좌표로 돌려준다.
    ///
    /// `origin`은 창 버퍼의 왼쪽 위가 칸 안의 어느 점인지다. 장은 `origin`에서 시작하는 창 안에
    /// 다 들어와야 한다(창을 [`Frames::bounds`] 크기로 만들면 그렇다).
    pub fn draw(
        &mut self,
        frame: usize,
        dst: &mut [u32],
        dst_width: i32,
        origin: (i32, i32),
    ) -> Result<Rect, PackError> {
        self.decoder.decode(&self.pack, frame, &mut self.indices)?;
        let info = &self.pack.frames()[frame];
        let palette = self.pack.palette(info.motion);
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
