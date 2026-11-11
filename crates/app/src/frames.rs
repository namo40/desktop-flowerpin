//! 그림 팩에서 장을 꺼내 창 버퍼에 그린다.
//!
//! 그림은 풀어 두지 않는다. 팩은 암호화된 채 exe 안에 그대로 있고, 장이 바뀔 때 그 장만 꺼낸다
//! (암호 풀기 → 압축 풀기 → 팔레트로 펼치기). 꺼내는 도구와 버퍼는 하나만 두고 모든 꽃핀이 함께 쓴다.
//!
//! 다만 되풀이하는 동작(쉬기, 걷기, 자기)의 장은 한 번 푼 것을 들고 있다가 다시 쓴다([`Kept`]). 같은 장들을
//! 끝없이 다시 보여 주는데 그때마다 암호와 압축을 푸는 것이 한 장을 그리는 일의 4분의 1쯤이었다.

use std::ops::Range;
use std::time::Instant;

use dfp_pack::{FrameDecoder, Pack, PackError};
use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAlloc, VirtualFree,
};

use crate::geom::Rect;
use crate::scale::{Scale, Scaler};

/// 빌드할 때 `build.rs`가 골라 넣은 팩. 진짜 그림 팩이거나 대체 그림 팩이다.
static PACK_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/pack.dfpk"));

/// 풀어서 들고 있는 장들이 차지해도 되는 크기(바이트). 되풀이하는 동작은 어느 것이든 통째로 들어가고
/// (가장 큰 것이 자는 동작으로 0.81MiB), 서서 쉬는 동작(0.58MiB)은 다른 되풀이하는 동작 하나(걷기, 걸터앉기,
/// 떨어지기, 붙들리기)와 함께 들어가는 크기다. 꽃핀이 한 명이면 이만큼 찰 일이 없다.
const KEEP_LIMIT: usize = 1024 * 1024;

/// 이만큼(ms) 쓰지 않은 동작의 장들은 버린다. 그 동작을 하는 꽃핀이 이제 없다는 뜻이다.
const KEEP_FOR: u64 = 3000;

/// 쓰지 않는 동작이 있는지 살펴보는 간격(ms).
const SWEEP_EVERY: u64 = 1000;

/// Windows에서 바로 받아 온 메모리 한 덩어리. 버리면 그 자리에서 Windows에 돌아간다.
///
/// 힙에서 잡은 덩어리는 버려도 프로세스가 그 자리를 쥐고 있어서, 쓰는 메모리가 줄지 않았다.
/// 받아 온 자리는 0으로 차 있고, 손대지 않은 쪽은 쓰는 메모리로 세지 않는다.
struct Block {
    at: *mut u8,
    len: usize,
}

impl Block {
    /// `len`바이트를 받아 온다. 받지 못하면 `None`이다.
    fn new(len: usize) -> Option<Block> {
        let at = unsafe { VirtualAlloc(std::ptr::null(), len, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE) };
        (!at.is_null()).then_some(Block { at: at as *mut u8, len })
    }

    fn bytes(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.at, self.len) }
    }

    fn bytes_mut(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.at, self.len) }
    }
}

impl Drop for Block {
    fn drop(&mut self) {
        unsafe { VirtualFree(self.at as _, 0, MEM_RELEASE) };
    }
}

/// 되풀이하는 동작의 장을 풀어서 들고 있는 곳.
///
/// 들고 있는 것은 팔레트 번호(픽셀마다 1바이트)다. 한 번만 보여 주는 동작(기지개, 하품 같은 것)의 장은
/// 들고 있지 않는다. 한 동작의 장들은 한 덩어리([`Block`])에 넣는다.
///
/// 지금 쓰고 있는 동작만 들고 있다. 한동안 쓰지 않은 동작은 버린다. 새 동작이 들어갈 자리가 없으면 그 동작은
/// 들고 있지 않고 전처럼 장마다 푼다. 자리를 내려고 쓰고 있는 동작을 내보내지는 않는다. 그러면 두 동작이
/// 서로를 내보내느라 장마다 덩어리를 새로 받아 오게 된다.
struct Kept {
    limit: usize,
    /// 장마다, 제 동작의 덩어리 안에서 차지하는 자리.
    slots: Vec<Range<usize>>,
    /// 장마다, 풀어서 넣어 두었는가.
    filled: Vec<bool>,
    /// 동작마다 하나씩 있는 선반.
    shelves: Vec<Shelf>,
    /// 들고 있는 덩어리들의 크기.
    total: usize,
    /// 쓰지 않는 동작이 있는지 다음에 살펴볼 시각(ms).
    sweep_at: u64,
}

/// 동작 하나의 장들을 풀어 두는 자리.
struct Shelf {
    /// 이 동작이 차지하는 장의 범위.
    frames: Range<usize>,
    /// 장을 모두 풀었을 때의 크기.
    size: usize,
    /// 풀어 둔 팔레트 번호. 들고 있지 않으면 없다.
    block: Option<Block>,
    /// 마지막으로 쓴 시각(ms).
    used: u64,
}

impl Kept {
    /// `motions`는 동작마다 그 장들의 크기(바이트)를 차례로 준다.
    fn new(limit: usize, motions: impl IntoIterator<Item = Vec<usize>>) -> Kept {
        let (mut slots, mut shelves) = (Vec::new(), Vec::new());
        for sizes in motions {
            let (first, mut size) = (slots.len(), 0);
            for len in sizes {
                slots.push(size..size + len);
                size += len;
            }
            shelves.push(Shelf { frames: first..slots.len(), size, block: None, used: 0 });
        }
        Kept { limit, filled: vec![false; slots.len()], slots, shelves, total: 0, sweep_at: 0 }
    }

    /// 들고 있는 장인가. 들고 있으면 그 동작을 방금(`at`) 쓴 것으로 적는다.
    fn touch(&mut self, frame: usize, motion: usize, at: u64) -> bool {
        let held = self.filled[frame];
        if held {
            self.shelves[motion].used = at;
        }
        if at >= self.sweep_at {
            self.sweep(at);
        }
        held
    }

    /// 들고 있는 장의 팔레트 번호.
    fn held(&self, frame: usize, motion: usize) -> Option<&[u8]> {
        let block = self.shelves[motion].block.as_ref().filter(|_| self.filled[frame])?;
        Some(&block.bytes()[self.slots[frame].clone()])
    }

    /// 방금 푼 장을 넣는다. 그 동작의 덩어리가 아직 없으면 받아 온다. 자리가 없으면 넣지 않는다.
    fn put(&mut self, frame: usize, motion: usize, indices: &[u8], at: u64) {
        if self.shelves[motion].block.is_none() {
            let size = self.shelves[motion].size;
            if self.total + size > self.limit {
                // 쓰지 않는 동작이 자리를 차지하고 있을 수 있다.
                self.sweep(at);
            }
            if size == 0 || self.total + size > self.limit {
                return;
            }
            self.shelves[motion].block = Block::new(size);
            if self.shelves[motion].block.is_some() {
                self.total += size;
            }
        }
        let shelf = &mut self.shelves[motion];
        // 덩어리를 받아 오지 못했으면 들고 있지 않을 뿐이다.
        let Some(block) = &mut shelf.block else { return };
        block.bytes_mut()[self.slots[frame].clone()].copy_from_slice(indices);
        shelf.used = at;
        self.filled[frame] = true;
    }

    /// `KEEP_FOR` 동안 쓰지 않은 동작의 덩어리를 버린다.
    fn sweep(&mut self, at: u64) {
        for motion in 0..self.shelves.len() {
            if self.shelves[motion].block.is_some() && at >= self.shelves[motion].used + KEEP_FOR {
                self.let_go(motion);
            }
        }
        self.sweep_at = at + SWEEP_EVERY;
    }

    /// 한 동작의 덩어리를 버린다.
    fn let_go(&mut self, motion: usize) {
        let shelf = &mut self.shelves[motion];
        if shelf.block.take().is_some() {
            self.filled[shelf.frames.clone()].fill(false);
            self.total -= shelf.size;
        }
    }

    /// 들고 있던 것을 모두 버린다.
    fn clear(&mut self) {
        (0..self.shelves.len()).for_each(|motion| self.let_go(motion));
    }
}

pub struct Frames {
    pack: Pack<'static>,
    decoder: FrameDecoder,
    /// 지금 꺼낸 장의 팔레트 번호. 한 칸을 다 채워도 들어가는 크기로 한 번만 잡는다.
    indices: Vec<u8>,
    /// 크기를 바꿔 그리는 도구. 쓰는 배율마다 하나씩 두고 모든 꽃핀이 함께 쓴다. 1배는 도구 없이 그린다.
    scalers: Vec<Scaler>,
    /// 되풀이하는 동작의 장을 풀어서 들고 있는 곳. 모든 꽃핀이 함께 쓴다.
    kept: Kept,
    /// 팩을 연 때. 풀어 둔 장을 언제 썼는지를 여기서부터 잰다.
    opened: Instant,
}

impl Frames {
    /// exe에 들어 있는 팩을 연다.
    pub fn load() -> Result<Frames, PackError> {
        let (pack, decoder) = crate::key::with_pack_key(|key| Pack::open(PACK_BYTES, key))?;
        let cell = pack.character().cell as usize;
        let sizes = pack.motions().iter().map(|motion| {
            let frames = &pack.frames()[motion.first_frame..motion.first_frame + motion.frame_count];
            frames.iter().map(|frame| frame.w as usize * frame.h as usize).collect()
        });
        let kept = Kept::new(KEEP_LIMIT, sizes);
        Ok(Frames { pack, decoder, indices: vec![0; cell * cell], scalers: Vec::new(), kept, opened: Instant::now() })
    }

    pub fn pack(&self) -> &Pack<'static> {
        &self.pack
    }

    /// 풀어서 들고 있던 장들을 버린다. 꽃핀을 숨겨서 한동안 그릴 일이 없을 때 부른다.
    pub fn forget(&mut self) {
        self.kept.clear();
    }

    /// 풀어서 들고 있는 장들의 크기(바이트).
    pub fn kept_bytes(&self) -> usize {
        self.kept.total
    }

    /// `frame`번 장의 윗부분을 한 변이 `size`인 작은 그림으로 줄인다. 트레이 아이콘으로 쓴다.
    ///
    /// 장의 너비만큼을 위에서부터 정사각형으로 잘라 쓴다. 서 있는 그림이면 얼굴이 들어온다.
    /// 결과는 미리 곱한 알파의 BGRA다.
    pub fn portrait(&mut self, frame: usize, size: usize) -> Result<Vec<u32>, PackError> {
        dfp_pack::icon::portrait(&self.pack, &mut self.decoder, &mut self.indices, frame, size)
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
        let info = &self.pack.frames()[frame];
        let motion = &self.pack.motions()[info.motion as usize];
        let (w, h) = (info.w as usize, info.h as usize);
        // 되풀이하는 동작의 장은 한 번 푼 것을 들고 있다가 다시 쓴다.
        let at = self.opened.elapsed().as_millis() as u64;
        if !self.kept.touch(frame, info.motion as usize, at) {
            self.decoder.decode(&self.pack, frame, &mut self.indices)?;
            if motion.looping {
                self.kept.put(frame, info.motion as usize, &self.indices[..w * h], at);
            }
        }
        let indices: &[u8] = self.kept.held(frame, info.motion as usize).unwrap_or(&self.indices);
        let palette = &motion.palette;
        if !scale.is_one() {
            let src = Rect::new(info.x as i32, info.y as i32, info.w as i32, info.h as i32);
            let scaler = scaler(&mut self.scalers, &self.pack, scale);
            return Ok(scaler.draw(indices, src, palette, dst, dst_width, origin));
        }
        // 1배는 섞을 것이 없으므로 팔레트 색을 그대로 옮긴다.
        let rect = Rect::new(info.x as i32 - origin.0, info.y as i32 - origin.1, w as i32, h as i32);
        for row in 0..h {
            let start = ((rect.y + row as i32) * dst_width + rect.x) as usize;
            let line = &mut dst[start..start + w];
            for (pixel, &index) in line.iter_mut().zip(&indices[row * w..(row + 1) * w]) {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 동작 셋이 10바이트짜리 장을 넷씩 가진다: 0번 동작이 0~3번 장, 1번이 4~7번, 2번이 8~11번.
    /// 그래서 덩어리 하나가 40바이트다.
    fn shelves(limit: usize) -> Kept {
        Kept::new(limit, (0..3).map(|_| vec![10; 4]))
    }

    #[test]
    fn kept_frames_come_back_as_they_went_in() {
        let mut kept = shelves(100);
        assert!(!kept.touch(1, 0, 0));
        assert_eq!(kept.held(1, 0), None);
        let frame: Vec<u8> = (1..=10).collect();
        kept.put(1, 0, &frame, 0);
        assert!(kept.touch(1, 0, 0));
        assert_eq!(kept.held(1, 0), Some(&frame[..]));
        // 그 동작의 장이 모두 들어갈 자리를 한 번에 잡는다.
        assert_eq!(kept.total, 40);
        // 다른 장은 들고 있지 않다.
        assert!(!kept.touch(0, 0, 0) && !kept.touch(4, 1, 0));
        // 옆 장을 넣어도 먼저 넣은 장은 그대로다.
        kept.put(2, 0, &[9; 10], 0);
        assert_eq!((kept.held(1, 0), kept.held(2, 0)), (Some(&frame[..]), Some(&[9u8; 10][..])));
        assert_eq!(kept.total, 40);
        kept.clear();
        assert!(!kept.touch(1, 0, 0));
        assert_eq!((kept.total, kept.held(1, 0)), (0, None));
    }

    #[test]
    fn a_motion_nobody_plays_any_more_is_let_go() {
        let mut kept = shelves(100);
        kept.put(0, 0, &[1; 10], 0);
        kept.put(4, 1, &[2; 10], 0);
        assert_eq!(kept.total, 80);
        // 0번 동작만 계속 쓴다. 1번 동작은 한동안 쓰지 않았으므로 나간다.
        assert!(kept.touch(0, 0, KEEP_FOR - 1));
        assert_eq!(kept.total, 80);
        assert!(kept.touch(0, 0, KEEP_FOR + SWEEP_EVERY));
        assert_eq!(kept.total, 40);
        assert!(!kept.touch(4, 1, KEEP_FOR + SWEEP_EVERY));
        assert_eq!(kept.held(0, 0), Some(&[1u8; 10][..]));
    }

    #[test]
    fn a_motion_waits_for_room_instead_of_pushing_one_in_use_out() {
        // 들고 있을 수 있는 것이 50바이트라 덩어리는 하나만 들어간다.
        let mut kept = shelves(50);
        kept.put(0, 0, &[1; 10], 0);
        // 0번 동작을 쓰고 있는 동안에는 1번 동작이 들어갈 자리가 없다. 0번을 내보내지 않는다.
        kept.put(4, 1, &[2; 10], 10);
        assert!(kept.touch(0, 0, 10) && !kept.touch(4, 1, 10));
        assert_eq!(kept.total, 40);
        // 0번 동작을 쓰지 않게 되면 그 자리에 1번 동작이 들어간다.
        kept.put(4, 1, &[2; 10], 10 + KEEP_FOR);
        assert!(kept.touch(4, 1, 10 + KEEP_FOR) && !kept.touch(0, 0, 10 + KEEP_FOR));
        assert_eq!(kept.total, 40);
        // 덩어리 하나가 들어가지 않는 크기면 아예 들고 있지 않는다.
        let mut small = shelves(30);
        small.put(0, 0, &[1; 10], 0);
        assert_eq!((small.total, small.touch(0, 0, 0)), (0, false));
    }

    #[test]
    fn a_kept_frame_draws_the_same_picture() {
        let mut frames = Frames::load().expect("그림 팩");
        let cell = frames.pack().character().cell as usize;
        let draw = |frames: &mut Frames, frame: usize| {
            let mut pixels = vec![0u32; cell * cell];
            let rect = frames.draw(frame, Scale::new(100, 96), &mut pixels, cell as i32, (0, 0)).expect("그리기");
            (rect, pixels)
        };
        let motions = frames.pack().motions().to_vec();
        // 되풀이하는 동작의 장은 처음 그릴 때 들어가고, 다시 그려도 같은 그림이 나온다.
        let looping = motions.iter().find(|motion| motion.looping).expect("되풀이하는 동작");
        assert_eq!(frames.kept_bytes(), 0);
        let first = draw(&mut frames, looping.first_frame);
        // 그 동작의 장이 모두 들어갈 자리를 잡는다.
        let held = frames.kept_bytes();
        let of_motion = &frames.pack().frames()[looping.first_frame..looping.first_frame + looping.frame_count];
        assert_eq!(held, of_motion.iter().map(|frame| frame.w as usize * frame.h as usize).sum::<usize>());
        assert_eq!(draw(&mut frames, looping.first_frame), first);
        assert_eq!(frames.kept_bytes(), held);
        // 한 번만 보여 주는 동작의 장은 들고 있지 않는다. 그린 뒤에도 앞의 장은 그대로 나온다.
        let once = motions.iter().find(|motion| !motion.looping).expect("한 번만 보여 주는 동작");
        draw(&mut frames, once.first_frame);
        assert_eq!(frames.kept_bytes(), held);
        assert_eq!(draw(&mut frames, looping.first_frame), first);
        // 버리고 나서 다시 그려도 같다.
        frames.forget();
        assert_eq!(frames.kept_bytes(), 0);
        assert_eq!(draw(&mut frames, looping.first_frame), first);
    }
}
