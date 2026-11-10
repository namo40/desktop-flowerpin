//! 꽃핀 한 명. 자기 창을 하나 갖고, 정해진 시각마다 다음 장을 그린다.

use std::time::{Duration, Instant};

use dfp_pack::temp::PackError;

use crate::character_window::CharacterWindow;
use crate::frames::Frames;
use crate::geom::Rect;

/// 꽃핀의 발바닥이 놓이는 점(칸 좌표). 이 점을 바닥 위의 서 있을 자리에 맞춘다.
const FOOT: (i32, i32) = (128, 232);

/// 장 하나를 보여 주는 시간. 그림은 1초에 24장으로 만들어졌다.
/// 임시 팩에는 장마다의 시간이 없어서 모든 장에 같은 값을 쓴다.
const FRAME_TIME: Duration = Duration::from_nanos(1_000_000_000 / 24);

/// 이만큼 넘게 밀리면(잠들었다 깬 뒤 등) 밀린 장을 따라잡지 않고 지금부터 다시 센다.
const MAX_LAG: Duration = Duration::from_millis(250);

pub struct Character {
    window: CharacterWindow,
    /// 창 버퍼의 왼쪽 위가 칸 안의 어느 점인지.
    origin: (i32, i32),
    /// 재생하는 동작의 장 번호들.
    motion: Vec<usize>,
    /// `motion` 안에서 지금 보여 주는 장의 순서.
    index: usize,
    /// 지금 창 버퍼에 그려져 있는 범위(창 좌표). 다음 장을 그리기 전에 여기만 지운다.
    drawn: Rect,
    /// 다음 장으로 넘어갈 시각.
    next_due: Instant,
}

impl Character {
    /// 발바닥이 화면의 `floor`에 오도록 창을 만들고, `motion`의 `start`번째 장부터 재생한다.
    pub fn new(
        frames: &mut Frames,
        motion: Vec<usize>,
        start: usize,
        floor: (i32, i32),
        now: Instant,
    ) -> Result<Character, PackError> {
        // 창은 모든 장이 들어가는 크기로 한 번만 만든다. 장마다 크기를 바꾸면 화면에 넘기는 비용이 오히려 는다.
        let bounds = frames.bounds();
        let x = floor.0 - (FOOT.0 - bounds.x);
        let y = floor.1 - (FOOT.1 - bounds.y);
        let window = CharacterWindow::new(bounds.w, bounds.h, x, y);
        let index = start % motion.len();
        let mut character = Character {
            window,
            origin: (bounds.x, bounds.y),
            motion,
            index,
            drawn: Rect::EMPTY,
            next_due: now + FRAME_TIME,
        };
        character.draw(frames)?;
        Ok(character)
    }

    pub fn window(&self) -> &CharacterWindow {
        &self.window
    }

    /// 다음 장으로 넘어갈 시각.
    pub fn due(&self) -> Instant {
        self.next_due
    }

    /// 시각이 됐으면 다음 장을 그려 화면에 넘긴다. 장을 넘겼으면 `true`를 돌려준다.
    pub fn tick(&mut self, frames: &mut Frames, now: Instant) -> Result<bool, PackError> {
        if now < self.next_due {
            return Ok(false);
        }
        self.index = (self.index + 1) % self.motion.len();
        self.draw(frames)?;
        // 장 시간을 그대로 더해서, 조금 늦게 깨어나도 전체 빠르기가 밀리지 않게 한다.
        self.next_due += FRAME_TIME;
        if now > self.next_due + MAX_LAG {
            self.next_due = now + FRAME_TIME;
        }
        Ok(true)
    }

    /// 지금 장을 창 버퍼에 그리고, 바뀐 범위만 화면에 넘긴다.
    fn draw(&mut self, frames: &mut Frames) -> Result<(), PackError> {
        let width = self.window.width();
        let previous = self.drawn;
        clear(self.window.pixels(), width, previous);
        self.drawn = frames.draw(self.motion[self.index], self.window.pixels(), width, self.origin)?;
        self.window.present(previous.union(self.drawn));
        Ok(())
    }
}

/// 창 버퍼의 `rect` 범위를 투명하게 지운다.
fn clear(pixels: &mut [u32], width: i32, rect: Rect) {
    for y in rect.y..rect.bottom() {
        let start = (y * width + rect.x) as usize;
        pixels[start..start + rect.w as usize].fill(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_touches_only_the_rect() {
        let mut pixels = vec![1u32; 4 * 3];
        clear(&mut pixels, 4, Rect::new(1, 1, 2, 2));
        assert_eq!(pixels, [1, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1]);
    }

    #[test]
    fn frame_time_is_24_per_second() {
        assert_eq!((FRAME_TIME * 24).as_millis(), 999);
    }
}
