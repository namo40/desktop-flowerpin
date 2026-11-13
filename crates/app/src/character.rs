//! 꽃핀 한 명. 상태 기계([`Actor`])가 정한 동작과 자리를 자기 창에 그려 보여 준다.

use std::time::{Duration, Instant};

use dfp_pack::PackError;

use crate::actor::{Actor, Mode, Show, Surroundings};
use crate::character_window::CharacterWindow;
use crate::desktop::Desktop;
use crate::frames::Frames;
use crate::geom::Rect;
use crate::motions::{BEAT_MS, Motions};
use crate::perch::{self, Body, Perch};
use crate::scale::Scale;
use crate::shadow::Shadow;
use crate::stage::{Monitor, Stage};

/// 벽에서 이만큼 떨어져 선다(그림 1배 기준 px). 몸이 화면 밖으로 나가지 않게 한다.
const WALL_MARGIN: f64 = 64.0;
/// 들어 올릴 때 화면 위 끝에 맞추는 칸의 줄. 머리 조금 위다.
const HEAD_ROW: f64 = 60.0;
/// 서 있거나 걸을 때 머리 꼭대기의 줄. 꽃핀 그림에서는 머리카락 위 끝이 칸의 y 84~90에 있다.
/// 다른 꽃핀이 이 높이에 올라선다.
const HEAD_TOP_ROW: f64 = 88.0;
/// 다른 꽃핀의 머리 위에 내려앉을 때, 발바닥 가운데가 머리 가운데에서 이만큼(그림 1배 기준 px) 안이면 받아 준다.
/// 올라선 뒤에는 가운데에 선다.
const HEAD_CATCH: f64 = 20.0;
/// 머리로 치는 범위의 아래 끝. 뒷덜미(잡는 점)에서 발바닥까지를 1로 볼 때, 뒷덜미에서 이만큼 내려간 곳까지다.
/// 꽃핀 그림에서는 칸의 y 160쯤이고, 얼굴 아래 끝이다. 이 위를 문질러야 쓰다듬는 것으로 친다.
const HEAD_BELOW_HOLD: f64 = 0.31;
/// 할 일이 없을 때(다 사라진 뒤) 다음에 깨어날 때까지의 시간.
const IDLE_WAKE: Duration = Duration::from_secs(3600);
/// 모든 꽃핀이 함께 맞추는 박자(초). 그림이 1초에 24장이라 장 하나의 길이와 같다.
const BEAT: f64 = BEAT_MS / 1000.0;
/// 박자를 셀 때 봐주는 오차(박자 단위, 약 40나노초). 시각을 나노초로 적으면서 생기는 끝자리 오차를 넘는다.
const BEAT_SLACK: f64 = 1e-6;

/// 반짝이는 시간(초)과 그동안 밝아졌다 돌아오는 횟수, 가장 밝을 때 흰색 쪽으로 가는 정도(0~1).
const BLINK_TIME: f64 = 0.9;
const BLINK_PULSES: f64 = 2.0;
const BLINK_PEAK: f64 = 0.6;

/// 깨어났을 때 이만큼 앞선 시각까지 본다. 곧 올 박자를 지금 함께 밟으려는 것이다.
/// [`Character::tick`]에는 "지금 + 이 값"을 넘긴다.
pub const LOOKAHEAD: Duration = Duration::from_millis(3);

/// `origin`에서 시작한 박자로 `at`이 몇 번째인지(소수).
fn beats_until(origin: Instant, at: Instant) -> f64 {
    at.saturating_duration_since(origin).as_secs_f64() / BEAT
}

fn beat_at(origin: Instant, index: f64) -> Instant {
    origin + Duration::from_secs_f64(index.max(0.0) * BEAT)
}

/// `due`나 그 뒤의 가장 가까운 박자. 박자는 `origin`에서 시작한다.
fn next_beat(origin: Instant, due: Instant) -> Instant {
    beat_at(origin, (beats_until(origin, due) - BEAT_SLACK).ceil())
}

/// `at`이나 그 앞의 가장 가까운 박자.
fn last_beat(origin: Instant, at: Instant) -> Instant {
    beat_at(origin, (beats_until(origin, at) + BEAT_SLACK).floor())
}

/// 상태 기계의 시간을 어디까지 흘릴지 정한다. `last`는 지난번에 흘린 시각이다.
///
/// 박자에 맞춰 사는 동안(`smooth`가 아닐 때)에는 `horizon` 앞의 마지막 박자까지만 흘린다. 그러면 조금
/// 일찍 깨든 늦게 깨든 상태 기계의 시간은 꼭 박자만큼씩 흐르고, 장도 박자마다 꼭 하나씩 넘어간다.
/// 깨어난 시각을 그대로 쓰면, 장이 바뀌는 시각이 깨어난 시각과 거의 겹칠 때 그 작은 차이에 따라
/// 어떤 장은 두 박자 동안 보이고 어떤 장은 건너뛴다.
fn clock_at(origin: Instant, horizon: Instant, last: Instant, smooth: bool) -> Instant {
    if smooth { horizon } else { last_beat(origin, horizon).max(last) }
}

/// `from`까지 시간을 흘린 뒤 다음에 깨어날 시각. `wait`은 상태 기계가 기다려도 된다고 한 시간(초)이다.
fn due_after(origin: Instant, from: Instant, wait: f64, smooth: bool) -> Instant {
    if !wait.is_finite() {
        from + IDLE_WAKE
    } else if smooth {
        from + Duration::from_secs_f64(wait)
    } else {
        // 적어도 반 박자 뒤의 박자를 고른다. 방금 밟은 박자를 다시 고르면 시간이 흐르지 않은 채 맴돈다.
        next_beat(origin, from + Duration::from_secs_f64(wait.max(BEAT / 2.0)))
    }
}

/// 다른 꽃핀이 올라설 수 있는 머리. `under`는 이 꽃핀이 딛고 선 꽃핀들(없으면 0)이다. 그 꽃핀들은 이 머리에
/// 올라서지 않는다. 제 위에 선 꽃핀의 머리로 올라가려는 일이 없게 한다.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Head {
    pub perch: Perch,
    pub under: [isize; 2],
}

/// 꽃핀이 어떻게 와 있는지.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stay {
    /// 기본 꽃핀. 앱을 끝낼 때까지 있다.
    Home,
    /// 더 부른 꽃핀과, 부른 시각. 머무를 시간이 지나거나 보내면 돌아간다.
    Guest(Instant),
}

/// 꽃핀을 어떻게 보여 줄지에 대한 설정.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    /// 사용자가 고른 크기(퍼센트).
    pub size_percent: u32,
    /// 모니터의 DPI 대신 쓸 값. 배율이 큰 모니터가 없어도 그 배율의 그림을 확인하려고 둔다.
    pub dpi_override: Option<u32>,
    /// 다른 창들보다 늘 위에 있는다.
    pub topmost: bool,
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
    /// 발바닥에서 머리의 아래 끝까지(화면 px, 위쪽이라 음수).
    head_bottom: f64,
    /// 그림자가 창 안에 다 들어오는 가장 높은 높이(화면 px). 이보다 높이 뜨면 그림자를 그리지 않는다.
    shadow_reach: f64,
    /// 벽에서 떨어져 설 거리와, 들어 올릴 때 발바닥에서 화면 위 끝까지 남길 거리(화면 px).
    wall_margin: f64,
    headroom: f64,
    /// 발바닥에서 머리 꼭대기까지(화면 px). 다른 꽃핀이 이 높이에 올라선다.
    crown: f64,
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
            head_bottom: (character.hold.1 as f64 - character.foot_y as f64) * (1.0 - HEAD_BELOW_HOLD) * factor,
            shadow_reach: (bounds.bottom() - shadow_bottom).max(1) as f64,
            wall_margin: WALL_MARGIN * factor,
            headroom: (character.foot_y as f64 - HEAD_ROW) * factor,
            crown: (character.foot_y as f64 - HEAD_TOP_ROW) * factor,
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
    /// 반짝여서 밝아진 정도(0~255).
    glow: u8,
}

pub struct Character {
    window: CharacterWindow,
    layout: Layout,
    look: Look,
    actor: Actor,
    stay: Stay,
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
    /// 마지막 걸음에서 걸터앉아 있었는가.
    sitting: bool,
    /// 창이 다른 꽃핀들보다 앞으로 나왔다. 꽃핀끼리의 앞뒤 차례를 다시 맞춰야 한다.
    raised: bool,
    /// 올라설 수 있는 다른 창의 윗변들과, 그것을 계산했을 때의 조건. 조건이 달라졌을 때만 다시 계산한다.
    perches: Vec<Perch>,
    perches_for: Option<(u64, Rect, Body, bool)>,
    /// 반짝임이 끝날 때까지 남은 시간(초). 반짝이지 않으면 0이다.
    blink_left: f64,
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
        let window = CharacterWindow::new(layout.size.0, layout.size.1, x, y, look.topmost);
        let mut character = Character {
            window,
            layout,
            look,
            actor,
            stay: Stay::Home,
            shown: None,
            drawn: Rect::EMPTY,
            last: now,
            due: now,
            beat,
            moved: false,
            sitting: false,
            // 새로 만든 창은 맨 앞에 놓인다.
            raised: true,
            perches: Vec::new(),
            perches_for: None,
            blink_left: 0.0,
        };
        let around = character.surroundings(character.monitor(stage));
        character.render(frames, motions, &around)?;
        character.schedule(motions, now);
        Ok(character)
    }

    pub fn window(&self) -> &CharacterWindow {
        &self.window
    }

    /// 이 꽃핀을 가리키는 값. 창 윗변처럼 이 꽃핀의 머리를 딛고 설 때 쓴다.
    pub fn id(&self) -> isize {
        self.window.hwnd() as isize
    }

    /// 다른 꽃핀이 올라설 수 있는 머리 위. 바닥이나 창 위에서 쉬거나 걷고 있을 때만 내준다.
    /// 다른 행동을 하거나, 잡혔거나, 돌아가는 중이면 위에 선 꽃핀이 떨어진다.
    pub fn head_perch(&self) -> Option<Perch> {
        if self.actor.mode() != Mode::Ground || self.actor.is_leaving() {
            return None;
        }
        let (x, y) = self.actor.position();
        let catch = HEAD_CATCH * self.layout.scale.factor();
        Some(Perch {
            window: self.id(),
            y: y - self.layout.crown,
            left: x - catch,
            right: x + catch,
            origin: x,
            head: true,
        })
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

    pub fn stay(&self) -> Stay {
        self.stay
    }

    /// 더 부른 꽃핀으로 삼는다. `since`는 부른 시각이다.
    pub fn set_guest(&mut self, since: Instant) {
        self.stay = Stay::Guest(since);
    }

    /// 기본 꽃핀으로 삼는다. 기본 꽃핀을 보냈을 때 남은 꽃핀이 그 자리를 잇는다.
    pub fn set_home(&mut self) {
        self.stay = Stay::Home;
    }

    /// 창이 다른 꽃핀들보다 앞으로 나온 적이 있는지 묻고, 물은 것으로 적는다.
    pub fn take_raised(&mut self) -> bool {
        std::mem::take(&mut self.raised)
    }

    /// 다음에 [`Character::tick`]을 불러야 할 시각.
    pub fn due(&self) -> Instant {
        self.due
    }

    /// 마지막 걸음에서 창이 움직였는가. 움직이는 동안에는 화면 확인을 건너뛴다.
    pub fn moved(&self) -> bool {
        self.moved
    }

    /// 멈춰 세워 두었던 꽃핀을 다시 움직이게 한다. 멈춰 있던 동안의 시간은 흐르지 않은 것으로 친다.
    pub fn resume(&mut self, now: Instant) {
        self.last = last_beat(self.beat, now).max(self.last);
        self.due = now;
    }

    /// 창을 숨기거나 다시 보인다. 다시 보일 때는 숨어 있던 동안의 시간이 흐르지 않은 것으로 친다.
    pub fn set_visible(&mut self, visible: bool, now: Instant) {
        self.window.set_visible(visible);
        if visible {
            self.resume(now);
        }
    }

    /// 잠깐 반짝인다. 어디 있는지 알려 주려는 것이다. 하던 동작은 그대로 한다.
    pub fn blink(&mut self, now: Instant) {
        self.blink_left = BLINK_TIME;
        self.wake(now);
    }

    /// 반짝여서 지금 밝아진 정도(0~255).
    fn glow(&self) -> u8 {
        let turns = (1.0 - self.blink_left / BLINK_TIME) * BLINK_PULSES;
        ((turns * std::f64::consts::PI).sin().powi(2) * BLINK_PEAK * 255.0).round() as u8
    }

    /// 상태를 밖에서 바꿨을 때(잡기, 놓기 등) 다음 걸음을 바로 밟게 한다.
    pub fn wake(&mut self, now: Instant) {
        self.due = now;
    }

    /// "항상 위"이면 다른 "항상 위" 창들보다도 앞으로 올린다.
    ///
    /// 작업표시줄도 "항상 위" 창이다. 작업표시줄이 앞으로 나온 뒤에는 꽃핀이 그 뒤에 놓여서, 걸터앉았을 때
    /// 작업표시줄 위로 내려온 다리가 가려진다.
    pub fn keep_on_top(&self) {
        if self.look.topmost {
            self.window.set_topmost(true);
        }
    }

    /// 보여 주는 설정을 바꾼다. 배율은 다음 걸음에서 다시 잡는다.
    pub fn set_look(&mut self, look: Look, now: Instant) {
        if look.topmost != self.look.topmost {
            self.window.set_topmost(look.topmost);
            self.raised = true;
        }
        self.look = look;
        self.wake(now);
    }

    /// 말풍선이 따라갈 자리. 머리 꼭대기의 가운데, 몸 너비의 절반쯤, 말풍선이 벗어나면 안 되는 범위,
    /// 그리고 그 모니터의 DPI다.
    pub fn head(&self, stage: &Stage) -> ((i32, i32), i32, Rect, u32) {
        let monitor = self.monitor(stage);
        let (x, y) = self.actor.position();
        let head = (x.round() as i32, (y - self.layout.headroom).round() as i32);
        // 창은 효과까지 들어가는 너비라 몸보다 넓다. 몸은 그 3분의 2쯤이다.
        (head, self.layout.size.0 / 3, monitor.work, self.look.dpi_override.unwrap_or(monitor.dpi))
    }

    /// 시킨 행동을 바로 보여 준다. 지금 할 수 없으면 아무 일도 하지 않는다.
    pub fn show(&mut self, show: Show, motions: &Motions, stage: &Stage, now: Instant) {
        let around = Surroundings { perches: &self.perches, ..self.surroundings(self.monitor(stage)) };
        if self.actor.show(show, motions, &around) {
            self.wake(now);
        }
    }

    /// 커서가 `cursor`에서 `distance`px 움직였다. 머리 위였으면 문지른 것으로 친다.
    pub fn rub(&mut self, motions: &Motions, cursor: (i32, i32), distance: f64, now: Instant) {
        let before = self.actor.mode();
        if (cursor.1 as f64) < self.actor.position().1 + self.layout.head_bottom {
            self.actor.rub(motions, distance);
        }
        if self.actor.mode() != before {
            self.wake(now);
        }
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

    fn surroundings(&self, monitor: &Monitor) -> Surroundings<'static> {
        monitor.surroundings(self.layout.wall_margin, self.layout.headroom)
    }

    /// 선 몸이 발바닥 가운데에서 차지하는 범위. 창에 그리는 범위 전체로 친다.
    fn body(&self) -> Body {
        Body { left: self.layout.foot.0, right: self.layout.size.0 - self.layout.foot.0, up: self.layout.foot.1 }
    }

    /// 다른 꽃핀들의 머리 가운데 이 꽃핀이 올라설 수 있는 것. 같은 모니터에 있고, 올라서도 머리가 모니터
    /// 위로 나가지 않는 것이다. 제 머리와, 제 위에 선 꽃핀의 머리는 뺀다.
    fn usable_heads<'a>(&self, heads: &'a [Head], monitor: &Monitor) -> impl Iterator<Item = Perch> + Clone + 'a {
        let mine = self.id();
        let (left, right) = (monitor.work.x as f64, monitor.work.right() as f64);
        let top = monitor.area.y as f64 + self.layout.headroom;
        heads
            .iter()
            .filter(move |head| head.perch.window != mine && !head.under.contains(&mine))
            .filter(move |head| (left..right).contains(&head.perch.origin) && head.perch.y >= top)
            .map(|head| head.perch)
    }

    /// 이 꽃핀이 올라설 수 있는 창 윗변을 다시 계산한다. 보이는 창, 모니터, 몸 크기가 그대로면 건너뛴다.
    fn refresh_perches(&mut self, desktop: &Desktop, monitor: &Monitor) {
        let body = self.body();
        // 몸이 벗어나면 안 되는 범위. 옆은 작업 영역, 위는 모니터의 위 끝, 아래는 바닥이다.
        let top = monitor.area.y;
        let stage = Rect::new(monitor.work.x, top, monitor.work.w, monitor.work.bottom() - top);
        let conditions = (desktop.version(), stage, body, self.look.topmost);
        if self.perches_for != Some(conditions) {
            // "항상 위"가 꺼져 있으면 꽃핀이 다른 창에 가려진다. 그때는 창 위에 올라서지 않는다.
            self.perches = if self.look.topmost { perch::find(desktop.panes(), body, stage) } else { Vec::new() };
            self.perches_for = Some(conditions);
        }
    }

    /// 시각이 됐으면 상태 기계의 시간을 흘리고, 달라진 모습을 화면에 넘긴다. 걸음을 밟았으면 `true`다.
    ///
    /// `horizon`은 "지금 + [`LOOKAHEAD`]"다. 시간을 어디까지 흘리는지는 [`clock_at`]이 정한다.
    /// `heads`는 올라설 수 있는 다른 꽃핀들의 머리다(제 머리도 들어 있을 수 있다).
    pub fn tick(
        &mut self,
        frames: &mut Frames,
        motions: &Motions,
        stage: &Stage,
        (desktop, heads): (&Desktop, &[Head]),
        horizon: Instant,
    ) -> Result<bool, PackError> {
        if horizon < self.due {
            return Ok(false);
        }
        let at = clock_at(self.beat, horizon, self.last, self.actor.moves_smoothly());
        let dt = at.saturating_duration_since(self.last).as_secs_f64();
        self.last = at;
        self.blink_left = (self.blink_left - dt).max(0.0);

        let monitor = *self.monitor(stage);
        // 배율이 다른 모니터로 옮겨졌으면 크기를 다시 잡는다. 잡혀 있는 동안에는 창을 바꾸지 않는다.
        // 창을 새로 만들면 누르고 있던 것이 끊기기 때문이다. 놓은 뒤에 바꾼다.
        let scale = self.look.scale_on(&monitor);
        if scale != self.layout.scale && self.actor.mode() != Mode::Drag {
            self.rescale(frames, motions, scale);
        }
        self.refresh_perches(desktop, &monitor);
        // 둘레의 상황이 윗변 목록을 빌려 쓰므로, 걸음을 밟는 동안 잠깐 꺼내 둔다.
        let perches = std::mem::take(&mut self.perches);
        let heads = self.usable_heads(heads, &monitor);
        let joined: Vec<Perch>;
        let all = if heads.clone().next().is_none() {
            &perches[..]
        } else {
            joined = perches.iter().copied().chain(heads).collect();
            &joined[..]
        };
        let around = Surroundings { perches: all, ..self.surroundings(&monitor) };
        self.actor.update(dt, motions, &around);
        let sitting = self.actor.mode() == Mode::Sit;
        if sitting && !self.sitting {
            // 다리가 단 아래로 내려온다. 단(작업표시줄)보다 앞에 있어야 보인다. 이 꽃핀만 먼저 올리면 다른
            // 꽃핀들의 앞으로 잠깐 나와 깜빡여 보이므로, 앱이 꽃핀들을 차례대로 함께 올리게 한다.
            self.raised = true;
        }
        self.sitting = sitting;
        let rendered = self.render(frames, motions, &around);
        self.perches = perches;
        rendered?;
        self.schedule(motions, at);
        Ok(true)
    }

    /// 다음에 깨어날 시각을 잡는다. `from`은 방금 시간을 흘린 시각이다.
    ///
    /// 장을 넘기려고 깨는 것은 모든 꽃핀이 함께 쓰는 박자에 맞춘다. 동작을 시작한 시각이 꽃핀마다 달라도
    /// 같은 순간에 함께 깨어나므로, 꽃핀 수가 늘어도 깨어나는 횟수는 1초에 24번 그대로다. 박자 사이에
    /// 시작한 동작은 장이 조금(한 박자 안에서) 늦게 넘어가지만, 시간은 흐른 만큼 세므로 빠르기는 그대로다.
    ///
    /// 떨어지거나 끌려가는 동안에는 박자보다 자주 움직여야 하므로 박자에 맞추지 않는다.
    fn schedule(&mut self, motions: &Motions, from: Instant) {
        let mut wait = self.actor.wake_in(motions);
        if self.blink_left > 0.0 {
            // 반짝이는 동안에는 장이 그대로여도 박자마다 밝기를 고친다.
            wait = wait.min(BEAT);
        }
        self.due = due_after(self.beat, from, wait, self.actor.moves_smoothly());
    }

    /// 배율을 바꾼다. 창 버퍼의 크기가 달라지므로 창을 새로 만든다.
    fn rescale(&mut self, frames: &mut Frames, motions: &Motions, scale: Scale) {
        self.layout = Layout::new(frames, scale);
        self.actor.set_scale(scale.factor());
        let (x, y) = self.layout.window_position(self.actor.foot(motions));
        self.window = CharacterWindow::new(self.layout.size.0, self.layout.size.1, x, y, self.look.topmost);
        self.shown = None;
        self.drawn = Rect::EMPTY;
        self.raised = true;
    }

    /// 지금 모습이 창에 그려진 것과 다르면 다시 그리고, 자리와 함께 화면에 넘긴다.
    fn render(&mut self, frames: &mut Frames, motions: &Motions, around: &Surroundings) -> Result<(), PackError> {
        // 그림자는 발밑의 바닥(창 위에 있으면 그 윗변)에 진다. 떠오르면 그만큼 발밑에서 멀어지고,
        // 창 밖으로 나가기 전에 옅어져 없어진다.
        let height = (self.actor.ground_below(around) - self.actor.position().1).max(0.0);
        let level = self.actor.shadow() * (1.0 - height / self.layout.shadow_reach).clamp(0.0, 1.0);
        let shadow_level = (level * 255.0).round() as u8;
        let now = Drawn {
            frame: self.actor.frame(motions),
            shadow_level,
            shadow_drop: if shadow_level > 0 { height.round() as i32 } else { 0 },
            glow: self.glow(),
        };

        let mut dirty = Rect::EMPTY;
        if self.shown != Some(now) {
            let (width, rows) = (self.window.width(), self.window.height());
            let previous = self.drawn;
            clear(self.window.pixels(), width, previous);
            let mut drawn = Rect::EMPTY;
            if !self.actor.is_gone() {
                drawn = frames.draw(now.frame, self.layout.scale, self.window.pixels(), width, self.layout.origin)?;
                brighten(self.window.pixels(), width, drawn, now.glow);
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

/// 창 버퍼의 `rect` 범위에 있는 그림을 흰색 쪽으로 밝힌다. `level`이 255면 온통 희다. 투명한 정도는 그대로다.
fn brighten(pixels: &mut [u32], width: i32, rect: Rect, level: u8) {
    if level == 0 {
        return;
    }
    let level = level as u32;
    for y in rect.y..rect.bottom() {
        let start = (y * width + rect.x) as usize;
        for pixel in &mut pixels[start..start + rect.w as usize] {
            let (alpha, color) = (*pixel >> 24, *pixel);
            // 미리 곱한 알파라, 그 픽셀에서 가장 밝은 값은 알파와 같다.
            let lift = |shift: u32| {
                let channel = ((color >> shift) & 0xFF).min(alpha);
                (channel + (alpha - channel) * level / 255) << shift
            };
            *pixel = alpha << 24 | lift(16) | lift(8) | lift(0);
        }
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
        // 박자를 나노초로 적은 시각은 그 박자 그대로다. 앞으로 찾든 뒤로 찾든 같다.
        for beat in 0..2000 {
            let on = beat_at(origin, beat as f64);
            assert_eq!(next_beat(origin, on), on);
            assert_eq!(last_beat(origin, on), on);
            assert_eq!(last_beat(origin, on + Duration::from_millis(41)), on);
            assert_eq!(next_beat(origin, on + Duration::from_millis(1)), beat_at(origin, beat as f64 + 1.0));
        }
    }

    #[test]
    fn waiting_never_picks_the_beat_just_taken() {
        let origin = Instant::now();
        let beat = |index: f64| beat_at(origin, index);
        // 박자 위에서 기다릴 시간이 0이어도 다음 박자다.
        assert_eq!(due_after(origin, beat(7.0), 0.0, false), beat(8.0));
        assert_eq!(due_after(origin, beat(7.0), BEAT, false), beat(8.0));
        assert_eq!(due_after(origin, beat(7.0), BEAT * 6.0, false), beat(13.0));
        // 떨어지거나 끌려가는 동안에는 박자에 맞추지 않는다.
        assert_eq!(due_after(origin, beat(7.0), 0.01, true), beat(7.0) + Duration::from_millis(10));
        // 사라진 뒤에는 깨어날 일이 없다.
        assert_eq!(due_after(origin, beat(7.0), f64::INFINITY, false), beat(7.0) + IDLE_WAKE);
    }

    #[test]
    fn every_beat_shows_exactly_the_next_frame() {
        // 진짜 팩처럼 장 시간이 41ms와 42ms로 적힌 동작을, 박자에 맞춰 깨어나며 재생해 본다. 동작을 시작한
        // 시각이 박자와 어떻게 어긋나 있든(0~41ms), 일찍 깨든 늦게 깨든, 박자마다 꼭 한 장씩 넘어가야 한다.
        let durations: Vec<u16> = (0..48).map(|i| if i % 3 == 0 { 41 } else { 42 }).collect();
        let clip = crate::motions::Clip::new("x", 0, &durations, true);
        let origin = Instant::now();
        let ms = |ms: f64| Duration::from_secs_f64(ms / 1000.0);
        for phase in 0..420 {
            // 깨어나는 시각이 박자에서 어긋난 정도(ms). 음수는 다른 일로 조금 일찍 깬 것이다.
            for late in [-2.9, 0.0, 0.4, 1.5, 20.0, 38.0] {
                // 동작은 열 번째 박자에서 `phase / 10`ms 지난 때 시작했다. 상태 기계처럼 시각을 센다.
                let started = beat_at(origin, 10.0) + ms(phase as f64 / 10.0);
                let (mut last, mut t, mut laps) = (started, 0.0, 0);
                let mut due = due_after(origin, started, clip.until_next_frame(t) / 1000.0, false);
                let mut shown = 0;
                for step in 0..200 {
                    let now = if late < 0.0 { due - ms(-late) } else { due + ms(late) };
                    let horizon = now + LOOKAHEAD;
                    assert!(horizon >= due);
                    let at = clock_at(origin, horizon, last, false);
                    t = clip.settle(t + (at - last).as_secs_f64() * 1000.0);
                    if t >= clip.total_ms {
                        t = clip.settle(t % clip.total_ms);
                        laps += 1;
                    }
                    last = at;
                    let frame = laps * durations.len() + clip.frame_at(t);
                    assert_eq!(frame, shown + 1, "어긋남 {}ms, {late}ms 늦음, {step}번째", phase as f64 / 10.0);
                    shown = frame;
                    let next = due_after(origin, at, clip.until_next_frame(t) / 1000.0, false);
                    assert!(next > due, "같은 박자에 두 번 깨어남");
                    due = next;
                }
            }
        }
    }

    #[test]
    fn brightening_moves_colors_toward_white_and_keeps_alpha() {
        // 불투명한 색, 반투명한 색(미리 곱한 알파), 투명한 픽셀.
        let mut pixels = vec![0xFF20_4060u32, 0x8010_2030, 0x0000_0000, 0xFF20_4060];
        brighten(&mut pixels, 4, Rect::new(0, 0, 3, 1), 255);
        // 가장 밝을 때는 알파만큼 희다. 투명한 픽셀과 범위 밖은 그대로다.
        assert_eq!(pixels, [0xFFFF_FFFF, 0x8080_8080, 0x0000_0000, 0xFF20_4060]);
        let mut half = vec![0xFF00_80FFu32];
        brighten(&mut half, 1, Rect::new(0, 0, 1, 1), 128);
        assert_eq!(half, [0xFF80_BFFF]);
        // 0이면 아무것도 바꾸지 않는다.
        let mut same = vec![0xFF12_3456u32];
        brighten(&mut same, 1, Rect::new(0, 0, 1, 1), 0);
        assert_eq!(same, [0xFF12_3456]);
    }

    #[test]
    fn clear_touches_only_the_rect() {
        let mut pixels = vec![1u32; 4 * 3];
        clear(&mut pixels, 4, Rect::new(1, 1, 2, 2));
        assert_eq!(pixels, [1, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1]);
    }
}
