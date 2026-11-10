//! 꽃핀 한 명. 상태 기계([`Actor`])가 정한 동작과 자리를 자기 창에 그려 보여 준다.

use std::time::{Duration, Instant};

use dfp_pack::PackError;

use crate::actor::{Actor, Mode, Surroundings};
use crate::character_window::CharacterWindow;
use crate::frames::Frames;
use crate::geom::Rect;
use crate::motions::Motions;
use crate::scale::Scale;
use crate::shadow::Shadow;
use crate::stage::{Monitor, Stage};

/// 벽에서 이만큼 떨어져 선다(그림 1배 기준 px). 몸이 화면 밖으로 나가지 않게 한다.
const WALL_MARGIN: f64 = 64.0;
/// 들어 올릴 때 화면 위 끝에 맞추는 칸의 줄. 머리 조금 위다.
const HEAD_ROW: f64 = 60.0;
/// 할 일이 없을 때(다 사라진 뒤) 다음에 깨어날 때까지의 시간.
const IDLE_WAKE: Duration = Duration::from_secs(3600);
/// 모든 꽃핀이 함께 맞추는 박자(초). 그림이 1초에 24장이라 장 하나의 길이와 같다.
const BEAT: f64 = 1.0 / 24.0;

/// `due`를 그 뒤의 가장 가까운 박자로 미룬다. 박자는 `origin`에서 시작한다.
fn next_beat(origin: Instant, due: Instant) -> Instant {
    let beats = (due.saturating_duration_since(origin).as_secs_f64() / BEAT - 1e-6).ceil().max(0.0);
    origin + Duration::from_secs_f64(beats * BEAT)
}

/// 꽃핀을 어떻게 보여 줄지에 대한 설정.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    /// 사용자가 고른 크기(퍼센트).
    pub size_percent: u32,
    /// 모니터의 DPI 대신 쓸 값. 배율이 큰 모니터가 없어도 그 배율의 그림을 확인하려고 둔다.
    pub dpi_override: Option<u32>,
}

impl Look {
    fn scale_on(&self, monitor: &Monitor) -> Scale {
        Scale::new(self.size_percent, self.dpi_override.unwrap_or(monitor.dpi))
    }
}

/// 창 버퍼 안에서 그림이 놓이는 자리. 배율이 바뀌면 다시 잡는다.
struct Layout {
    scale: Scale,
    /// 창 버퍼의 왼쪽 위가 화면 칸(칸을 배율만큼 줄이거나 키운 것)의 어느 점인지.
    origin: (i32, i32),
    size: (i32, i32),
    /// 발바닥 가운데(창 좌표). 이 점이 꽃핀의 자리에 온다.
    foot: (i32, i32),
    /// 서 있을 때 그림자의 가운데(창 좌표).
    shadow_center: (i32, i32),
    /// 발바닥에서 잡는 점(뒷덜미)까지(화면 px).
    hold: (f64, f64),
    /// 그림자가 창 안에 다 들어오는 가장 높은 높이(화면 px). 이보다 높이 뜨면 그림자를 그리지 않는다.
    shadow_reach: f64,
    /// 벽에서 떨어져 설 거리와, 들어 올릴 때 발바닥에서 화면 위 끝까지 남길 거리(화면 px).
    wall_margin: f64,
    headroom: f64,
    shadow: Shadow,
}

impl Layout {
    fn new(frames: &mut Frames, scale: Scale) -> Layout {
        let character = frames.pack().character();
        let factor = scale.factor();
        let shadow = Shadow::new(factor);
        let pivot = (scale.point(character.pivot.0 as i32), scale.point(character.pivot.1 as i32));
        let foot_y = scale.point(character.foot_y as i32);
        // 창은 모든 장과 그림자가 들어가는 크기로 한 번만 만든다. 장마다 크기를 바꾸면 화면에 넘기는 비용이 오히려 는다.
        let bounds = frames.bounds(scale).union(shadow.rect(pivot));
        let origin = (bounds.x, bounds.y);
        let shadow_bottom = shadow.rect(pivot).bottom();
        Layout {
            scale,
            origin,
            size: (bounds.w, bounds.h),
            foot: (pivot.0 - origin.0, foot_y - origin.1),
            shadow_center: (pivot.0 - origin.0, pivot.1 - origin.1),
            hold: (
                (character.hold.0 as f64 - character.pivot.0 as f64) * factor,
                (character.hold.1 as f64 - character.foot_y as f64) * factor,
            ),
            shadow_reach: (bounds.bottom() - shadow_bottom).max(1) as f64,
            wall_margin: WALL_MARGIN * factor,
            headroom: (character.foot_y as f64 - HEAD_ROW) * factor,
            shadow,
        }
    }

    /// 발바닥이 화면의 `foot`에 올 때 창의 왼쪽 위.
    fn window_position(&self, foot: (i32, i32)) -> (i32, i32) {
        (foot.0 - self.foot.0, foot.1 - self.foot.1)
    }
}

/// 창에 그려져 있는 모습. 이것이 달라질 때만 다시 그린다.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Drawn {
    frame: usize,
    /// 그림자의 진하기(0~255)와, 발밑에서 아래로 내려간 거리(화면 px).
    shadow_level: u8,
    shadow_drop: i32,
}

pub struct Character {
    window: CharacterWindow,
    layout: Layout,
    look: Look,
    actor: Actor,
    shown: Option<Drawn>,
    /// 지금 창 버퍼에 그려져 있는 범위(창 좌표). 다음에 그리기 전에 여기만 지운다.
    drawn: Rect,
    /// 상태 기계의 시간을 마지막으로 흘린 시각과, 다음에 흘릴 시각.
    last: Instant,
    due: Instant,
    /// 박자가 시작한 시각. 모든 꽃핀이 같은 값을 쓴다.
    beat: Instant,
    /// 마지막 걸음에서 창이 움직였는가.
    moved: bool,
}

impl Character {
    /// 화면의 `(x, y)`(발바닥 자리) 공중에 나타난다.
    pub fn appear(
        frames: &mut Frames,
        motions: &Motions,
        stage: &Stage,
        look: Look,
        (x, y): (f64, f64),
        (now, beat): (Instant, Instant),
        seed: u64,
    ) -> Result<Character, PackError> {
        let scale = look.scale_on(stage.nearest(x.round() as i32, y.round() as i32));
        let actor = Actor::appear(motions, x, y, scale.factor(), seed);
        Character::new(frames, motions, stage, look, actor, scale, (now, beat))
    }

    /// 화면의 `x`에서 바닥에 선 채로 시작한다. 동작을 차례로 보여 줄 때 쓴다.
    pub fn stand(
        frames: &mut Frames,
        motions: &Motions,
        stage: &Stage,
        look: Look,
        x: f64,
        (now, beat): (Instant, Instant),
        seed: u64,
    ) -> Result<Character, PackError> {
        let monitor = stage.nearest(x.round() as i32, stage.primary().work.y);
        let scale = look.scale_on(monitor);
        let actor = Actor::stand(motions, x, monitor.work.bottom() as f64, scale.factor(), seed);
        Character::new(frames, motions, stage, look, actor, scale, (now, beat))
    }

    fn new(
        frames: &mut Frames,
        motions: &Motions,
        stage: &Stage,
        look: Look,
        actor: Actor,
        scale: Scale,
        (now, beat): (Instant, Instant),
    ) -> Result<Character, PackError> {
        let layout = Layout::new(frames, scale);
        let (x, y) = layout.window_position(actor.foot(motions));
        let window = CharacterWindow::new(layout.size.0, layout.size.1, x, y);
        let mut character = Character {
            window,
            layout,
            look,
            actor,
            shown: None,
            drawn: Rect::EMPTY,
            last: now,
            due: now,
            beat,
            moved: false,
        };
        let around = character.surroundings(character.monitor(stage));
        character.render(frames, motions, &around)?;
        character.schedule(motions, now);
        Ok(character)
    }

    pub fn window(&self) -> &CharacterWindow {
        &self.window
    }

    pub fn actor(&self) -> &Actor {
        &self.actor
    }

    pub fn actor_mut(&mut self) -> &mut Actor {
        &mut self.actor
    }

    pub fn scale(&self) -> Scale {
        self.layout.scale
    }

    /// 다음에 [`Character::tick`]을 불러야 할 시각.
    pub fn due(&self) -> Instant {
        self.due
    }

    /// 마지막 걸음에서 창이 움직였는가. 움직이는 동안에는 화면 확인을 건너뛴다.
    pub fn moved(&self) -> bool {
        self.moved
    }

    /// 상태를 밖에서 바꿨을 때(잡기, 놓기 등) 다음 걸음을 바로 밟게 한다.
    pub fn wake(&mut self, now: Instant) {
        self.due = now;
    }

    /// 보여 주는 설정을 바꾼다. 배율은 다음 걸음에서 다시 잡는다.
    pub fn set_look(&mut self, look: Look, now: Instant) {
        self.look = look;
        self.wake(now);
    }

    /// 커서가 `cursor`에 있을 때, 잡는 점(뒷덜미)이 커서에 오게 하는 발바닥 자리.
    /// 그 모니터의 바닥 아래나 화면 위로는 나가지 않는다.
    pub fn drag_target(&self, cursor: (i32, i32), stage: &Stage) -> (f64, f64) {
        self.drag_to_foot((cursor.0 as f64 - self.layout.hold.0, cursor.1 as f64 - self.layout.hold.1), stage)
    }

    /// 발바닥을 `foot`으로 끌고 가려 할 때 실제로 갈 수 있는 자리.
    pub fn drag_to_foot(&self, foot: (f64, f64), stage: &Stage) -> (f64, f64) {
        let around = self.surroundings(stage.nearest(foot.0.round() as i32, foot.1.round() as i32 - 1));
        (foot.0, foot.1.clamp(around.top, around.floor))
    }

    /// 꽃핀이 있는 모니터. 잡혀 있을 때는 끌려가는 곳의 모니터다. 그래야 다른 모니터로 옮길 수 있다.
    fn monitor<'s>(&self, stage: &'s Stage) -> &'s Monitor {
        let (x, y) = if self.actor.mode() == Mode::Drag { self.actor.drag_target() } else { self.actor.position() };
        // 발바닥은 바닥선 위에 있고, 바닥선은 모니터의 아래 끝과 같을 수 있다. 그래서 한 픽셀 위의 점으로 찾는다.
        stage.nearest(x.round() as i32, y.round() as i32 - 1)
    }

    fn surroundings(&self, monitor: &Monitor) -> Surroundings {
        monitor.surroundings(self.layout.wall_margin, self.layout.headroom)
    }

    /// 시각이 됐으면 상태 기계의 시간을 `horizon`까지 흘리고, 달라진 모습을 화면에 넘긴다.
    /// 걸음을 밟았으면 `true`를 돌려준다.
    pub fn tick(
        &mut self,
        frames: &mut Frames,
        motions: &Motions,
        stage: &Stage,
        horizon: Instant,
    ) -> Result<bool, PackError> {
        if horizon < self.due {
            return Ok(false);
        }
        let dt = horizon.saturating_duration_since(self.last).as_secs_f64();
        self.last = horizon;

        let monitor = *self.monitor(stage);
        // 배율이 다른 모니터로 옮겨졌으면 크기를 다시 잡는다. 잡혀 있는 동안에는 창을 바꾸지 않는다.
        // 창을 새로 만들면 누르고 있던 것이 끊기기 때문이다. 놓은 뒤에 바꾼다.
        let scale = self.look.scale_on(&monitor);
        if scale != self.layout.scale && self.actor.mode() != Mode::Drag {
            self.rescale(frames, motions, scale);
        }
        let around = self.surroundings(&monitor);
        self.actor.update(dt, motions, &around);
        self.render(frames, motions, &around)?;
        self.schedule(motions, horizon);
        Ok(true)
    }

    /// 다음에 깨어날 시각을 잡는다.
    ///
    /// 장을 넘기려고 깨는 것은 모든 꽃핀이 함께 쓰는 박자에 맞춘다. 동작을 시작한 시각이 꽃핀마다 달라도
    /// 같은 순간에 함께 깨어나므로, 꽃핀 수가 늘어도 깨어나는 횟수는 1초에 24번 그대로다. 장이 조금
    /// (한 박자 안에서) 늦게 넘어가지만, 시간은 실제로 흐른 만큼 세므로 전체 빠르기는 그대로다.
    ///
    /// 떨어지거나 끌려가는 동안에는 박자보다 자주 움직여야 하므로 박자에 맞추지 않는다.
    fn schedule(&mut self, motions: &Motions, from: Instant) {
        let wake = self.actor.wake_in(motions);
        self.due = if !wake.is_finite() {
            from + IDLE_WAKE
        } else if self.actor.moves_smoothly() {
            from + Duration::from_secs_f64(wake)
        } else {
            next_beat(self.beat, from + Duration::from_secs_f64(wake))
        };
    }

    /// 배율을 바꾼다. 창 버퍼의 크기가 달라지므로 창을 새로 만든다.
    fn rescale(&mut self, frames: &mut Frames, motions: &Motions, scale: Scale) {
        self.layout = Layout::new(frames, scale);
        self.actor.set_scale(scale.factor());
        let (x, y) = self.layout.window_position(self.actor.foot(motions));
        self.window = CharacterWindow::new(self.layout.size.0, self.layout.size.1, x, y);
        self.shown = None;
        self.drawn = Rect::EMPTY;
    }

    /// 지금 모습이 창에 그려진 것과 다르면 다시 그리고, 자리와 함께 화면에 넘긴다.
    fn render(&mut self, frames: &mut Frames, motions: &Motions, around: &Surroundings) -> Result<(), PackError> {
        // 그림자는 바닥에 진다. 떠오르면 그만큼 발밑에서 멀어지고, 창 밖으로 나가기 전에 옅어져 없어진다.
        let height = (around.floor - self.actor.position().1).max(0.0);
        let level = self.actor.shadow() * (1.0 - height / self.layout.shadow_reach).clamp(0.0, 1.0);
        let shadow_level = (level * 255.0).round() as u8;
        let now = Drawn {
            frame: self.actor.frame(motions),
            shadow_level,
            shadow_drop: if shadow_level > 0 { height.round() as i32 } else { 0 },
        };

        let mut dirty = Rect::EMPTY;
        if self.shown != Some(now) {
            let (width, rows) = (self.window.width(), self.window.height());
            let previous = self.drawn;
            clear(self.window.pixels(), width, previous);
            let mut drawn = Rect::EMPTY;
            if !self.actor.is_gone() {
                drawn = frames.draw(now.frame, self.layout.scale, self.window.pixels(), width, self.layout.origin)?;
            }
            let center = (self.layout.shadow_center.0, self.layout.shadow_center.1 + now.shadow_drop);
            let shadow = self.layout.shadow.draw_under(self.window.pixels(), width, rows, center, now.shadow_level);
            self.drawn = drawn.union(shadow);
            dirty = previous.union(self.drawn);
            self.shown = Some(now);
        }

        let (x, y) = self.layout.window_position(self.actor.foot(motions));
        let before = self.window.screen_rect();
        self.moved = (before.x, before.y) != (x, y);
        self.window.move_to(x, y);
        self.window.present(dirty);
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
    fn wake_times_snap_to_the_shared_beat() {
        let origin = Instant::now();
        let at = |ms: f64| origin + Duration::from_secs_f64(ms / 1000.0);
        let beat_of = |ms: f64| (next_beat(origin, at(ms)) - origin).as_secs_f64() * 24.0;
        // 박자는 41.67ms마다다. 그 사이의 시각은 다음 박자로 미룬다.
        assert!((beat_of(1.0) - 1.0).abs() < 1e-6);
        assert!((beat_of(41.0) - 1.0).abs() < 1e-6);
        assert!((beat_of(42.0) - 2.0).abs() < 1e-6);
        // 박자 위에 있는 시각은 그대로 둔다.
        assert!((beat_of(1000.0 / 24.0 * 5.0) - 5.0).abs() < 1e-6);
        assert!((beat_of(0.0) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn clear_touches_only_the_rect() {
        let mut pixels = vec![1u32; 4 * 3];
        clear(&mut pixels, 4, Rect::new(1, 1, 2, 2));
        assert_eq!(pixels, [1, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1]);
    }
}
