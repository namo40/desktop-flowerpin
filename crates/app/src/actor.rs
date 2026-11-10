//! 꽃핀 한 명의 상태 기계. 지금 무엇을 하고 있고, 시간이 흐르면 무엇으로 넘어가는지를 정한다.
//!
//! 창이나 그림을 모른다. 시간(`dt`)과 둘레의 상황([`Surroundings`])을 받아 자리와 동작만 바꾼다.
//! 그래서 화면 없이 단위 테스트로 규칙을 확인할 수 있다.
//!
//! ```text
//! 나타나기: spawn → spawn_to_fall → fall(반복) → land → land_to_idle2 → idle2
//! 쉬기·걷기: idle2 ⇄ walk   (돌아설 때는 idle로 잠깐 선다)
//! 잡기:     hang(반복) → 놓으면 hang_to_fall → fall → land → …
//! 사라지기: idle2가 첫 장으로 돌아올 때 despawn → 없어짐
//! ```
//!
//! 한 번짜리 동작이 끝나면 팩에 적힌 "다음 동작"으로 넘어간다. 어떤 동작이 어떤 상태인지는
//! [`mode_of`]가 정한다. 규칙과 값은 원본 데모에서 가져왔다.
//!
//! 좌표는 화면 픽셀이고 y는 아래로 갈수록 커진다. 꽃핀의 자리는 발바닥 가운데다.
//! 길이와 빠르기 상수는 그림 1배 크기 기준이라, 쓸 때 표시 배율을 곱한다.

use crate::motions::{MotionId, Motions};

/// 떨어지는 가속도(px/s²).
const GRAVITY: f64 = 1500.0;
/// 놓은 뒤 공중에 잠깐 멈춰 있는 시간(ms). 놓치는 그림에서 눈을 번쩍 뜨는 장까지다.
const RELEASE_HOLD_MS: f64 = 360.0;
/// 나타난 뒤 떨어지기 시작하는 시간(ms). 눈을 뜨고 떨어지는 자세가 잡히는 장까지다.
const SPAWN_DROP_MS: f64 = 300.0;
/// 쉬는 시간과 한 번에 걷는 시간(초).
const REST: (f64, f64) = (1.5, 4.0);
const WALK: (f64, f64) = (1.0, 2.6);
/// 착지한 뒤 처음 쉬는 시간(초).
const SETTLE_REST: (f64, f64) = (0.8, 2.0);
/// 돌아서기 전에 idle로 잠깐 서 있는 시간(초)과, 걷기를 마쳤을 때 돌아설 확률.
const TURN_PAUSE: (f64, f64) = (0.6, 1.2);
const TURN_CHANCE: f64 = 0.25;
/// 자리에서 이만큼 넘게 멀어지면 돌아오는 쪽으로 걷는다(px).
const WANDER: f64 = 160.0;
/// 잡은 점이 커서를 따라가는 빠르기(1/s). 클수록 바짝 따라간다.
const DRAG_FOLLOW: f64 = 30.0;
/// 나타날 때 그림자가 진해지는 구간과 사라질 때 옅어지는 구간(ms). 몸이 빛에서 드러나고 부서지는 장에 맞췄다.
const SHADOW_IN_MS: (f64, f64) = (450.0, 900.0);
const SHADOW_OUT_MS: (f64, f64) = (1000.0, 1550.0);
/// 떨어지거나 끌려갈 때 자리를 고치는 간격(초). 이때만 장이 바뀌지 않아도 움직인다.
const SMOOTH_STEP: f64 = 1.0 / 60.0;
/// 한 번에 흐르게 하는 시간의 한계(초). 절전에서 깨어난 뒤처럼 오래 멈췄다 돌아오면 그사이를 건너뛴다.
/// 가장 오래 보여 주는 장(약 0.3초)보다는 길어야 한다. 짧으면 그 장을 보여 주는 동안 시간이 느리게 흐른다.
const MAX_STEP: f64 = 0.5;
/// 동작을 차례로 보여 줄 때([`Actor::play`]) 반복 동작을 도는 횟수.
const SHOWCASE_LOOPS: u32 = 2;

/// 꽃핀이 하고 있는 일.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// 바닥에서 쉬거나 걷는다.
    Ground,
    /// 제자리에서 동작 하나를 한다.
    Act,
    /// 잡혀서 매달려 있다.
    Drag,
    /// 방금 놓였다. 잠깐 떠 있다가 떨어진다.
    Release,
    Fall,
    /// 빛이 모여 나타나는 중이다. 공중에 떠 있다.
    Spawn,
    /// 나타난 뒤 떨어지는 자세를 잡는다.
    Emerge,
    Land,
    /// 착지한 뒤 서는 자세로 돌아온다.
    Settle,
    /// 손을 흔들며 사라지는 중이다.
    Vanish,
    /// 다 사라졌다. 창을 닫아도 된다.
    Gone,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    West,
    East,
}

impl Dir {
    fn opposite(self) -> Dir {
        if self == Dir::West { Dir::East } else { Dir::West }
    }

    fn sign(self) -> f64 {
        if self == Dir::West { -1.0 } else { 1.0 }
    }
}

/// 꽃핀 둘레의 상황. 꽃핀이 있는 모니터에서 나온다.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surroundings {
    /// 바닥선의 y. 발바닥이 여기에 놓인다.
    pub floor: f64,
    /// 걸어갈 수 있는 x의 범위(벽).
    pub left: f64,
    pub right: f64,
    /// 들어 올릴 수 있는 가장 높은 자리(발바닥의 y). 머리가 화면 위로 나가지 않게 한다.
    pub top: f64,
}

/// 작고 빠른 난수. 쉬는 시간과 걷는 방향을 고르는 데 쓴다.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        // 0이면 계속 0만 나오므로 피한다.
        Rng(seed | 1)
    }

    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// 0 이상 1 미만.
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn range(&mut self, (low, high): (f64, f64)) -> f64 {
        low + (high - low) * self.unit()
    }

    fn chance(&mut self, probability: f64) -> bool {
        self.unit() < probability
    }
}

/// 동작이 어떤 상태에 해당하는지. 한 번짜리 동작이 끝나 "다음 동작"으로 넘어갈 때 상태도 이것으로 정한다.
pub fn mode_of(motion: MotionId, motions: &Motions) -> Mode {
    match motion {
        m if m == motions.spawn => Mode::Spawn,
        m if m == motions.spawn_to_fall => Mode::Emerge,
        m if m == motions.fall => Mode::Fall,
        m if m == motions.land => Mode::Land,
        m if m == motions.land_to_idle2 => Mode::Settle,
        m if m == motions.hang => Mode::Drag,
        m if m == motions.hang_to_fall => Mode::Release,
        m if m == motions.despawn => Mode::Vanish,
        m if [motions.idle, motions.idle2, motions.walk_w, motions.walk_e].contains(&m) => Mode::Ground,
        _ => Mode::Act,
    }
}

pub struct Actor {
    mode: Mode,
    motion: MotionId,
    /// 지금 동작을 시작한 뒤 흐른 시간(ms). 반복 동작은 한 바퀴마다 0으로 돌아온다.
    t: f64,
    /// 발바닥 가운데의 자리(화면 px).
    x: f64,
    y: f64,
    /// 아래로 떨어지는 빠르기(px/s). 위로 뜰 때는 음수다.
    vy: f64,
    /// 표시 배율. 길이와 빠르기 상수에 곱한다.
    scale: f64,
    /// 둘레를 돌아다니는 자리의 가운데. 마지막으로 내려앉은 곳이다.
    home: f64,
    /// 걷고 있는 방향.
    walk: Option<Dir>,
    /// idle로 잠깐 선 뒤에 걸을 방향.
    turn_to: Option<Dir>,
    /// 지금 하는 일(쉬기, 걷기, 잠깐 서기)을 마칠 때까지 남은 시간(초).
    wait: f64,
    /// 끌려가는 목표(발바닥의 자리).
    drag: (f64, f64),
    /// 혼자 돌아다닌다. 끄면 제자리에서 쉬기만 한다.
    roam: bool,
    /// 사라지라는 말을 들었다. 바닥에 서면 손을 흔들고 사라진다.
    leaving: bool,
    /// 동작을 차례로 보여 주는 중이다([`Actor::play`]).
    showcase: bool,
    /// 반복 동작을 몇 바퀴 더 돌지.
    loops_left: u32,
    rng: Rng,
}

impl Actor {
    /// 공중의 `(x, y)`에 나타난다.
    pub fn appear(motions: &Motions, x: f64, y: f64, scale: f64, seed: u64) -> Actor {
        let mut actor = Actor::stand(motions, x, y, scale, seed);
        actor.mode = Mode::Spawn;
        actor.motion = motions.spawn;
        actor
    }

    /// 바닥의 `(x, y)`에 선 채로 시작한다.
    pub fn stand(motions: &Motions, x: f64, y: f64, scale: f64, seed: u64) -> Actor {
        let mut rng = Rng::new(seed);
        let wait = rng.range(SETTLE_REST);
        Actor {
            mode: Mode::Ground,
            motion: motions.idle2,
            t: 0.0,
            x,
            y,
            vy: 0.0,
            scale,
            home: x,
            walk: None,
            turn_to: None,
            wait,
            drag: (x, y),
            roam: true,
            leaving: false,
            showcase: false,
            loops_left: 0,
            rng,
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn motion(&self) -> MotionId {
        self.motion
    }

    pub fn is_gone(&self) -> bool {
        self.mode == Mode::Gone
    }

    /// 발바닥 가운데의 자리.
    pub fn position(&self) -> (f64, f64) {
        (self.x, self.y)
    }

    /// 끌려가는 목표. 잡혀 있을 때만 뜻이 있다.
    pub fn drag_target(&self) -> (f64, f64) {
        self.drag
    }

    /// 지금 보여 줄 장의 번호(팩의 장 표에서).
    pub fn frame(&self, motions: &Motions) -> usize {
        let clip = motions.clip(self.motion);
        clip.first_frame + clip.frame_at(self.t)
    }

    /// 그림을 놓을 자리. 발바닥 가운데에 동작의 발 자리 보정을 더하고 픽셀에 맞춘다.
    pub fn foot(&self, motions: &Motions) -> (i32, i32) {
        let shift = (motions.clip(self.motion).foot_shift * self.scale).round();
        (self.x.round() as i32 + shift as i32, self.y.round() as i32)
    }

    /// 그림자의 진하기(0~1). 나타날 때 서서히 진해지고 사라질 때 서서히 옅어진다.
    pub fn shadow(&self) -> f64 {
        let ramp = |(from, to): (f64, f64)| ((self.t - from) / (to - from)).clamp(0.0, 1.0);
        match self.mode {
            Mode::Spawn => ramp(SHADOW_IN_MS),
            Mode::Vanish => 1.0 - ramp(SHADOW_OUT_MS),
            Mode::Gone => 0.0,
            _ => 1.0,
        }
    }

    pub fn set_scale(&mut self, scale: f64) {
        self.scale = scale;
    }

    pub fn set_roam(&mut self, roam: bool) {
        self.roam = roam;
    }

    /// 지금 잡을 수 있는가. 나타나는 중, 착지하는 중, 사라지는 중에는 잡히지 않는다.
    pub fn can_grab(&self) -> bool {
        matches!(self.mode, Mode::Ground | Mode::Act | Mode::Release | Mode::Fall | Mode::Emerge)
    }

    /// 잡는다. 잡을 수 없는 때면 아무 일도 하지 않고 `false`를 돌려준다.
    pub fn grab(&mut self, motions: &Motions, target: (f64, f64)) -> bool {
        if !self.can_grab() {
            return false;
        }
        self.mode = Mode::Drag;
        self.motion = motions.hang;
        self.t = 0.0;
        self.vy = 0.0;
        self.walk = None;
        self.turn_to = None;
        self.showcase = false;
        self.drag = target;
        true
    }

    /// 잡힌 채로 끌려갈 곳을 바꾼다.
    pub fn drag_to(&mut self, target: (f64, f64)) {
        if self.mode == Mode::Drag {
            self.drag = target;
        }
    }

    /// 놓는다.
    pub fn release(&mut self, motions: &Motions) {
        if self.mode == Mode::Drag {
            self.mode = Mode::Release;
            self.motion = motions.hang_to_fall;
            self.t = 0.0;
            self.vy = 0.0;
        }
    }

    /// 사라지라고 한다. 바닥에 서 있지 않으면 내려와 선 뒤에 사라진다.
    pub fn leave(&mut self) {
        self.leaving = true;
    }

    /// 동작 하나를 보여 준다. 끝나면 "다음 동작"을 따라가다 idle2로 돌아온다.
    /// 반복 동작은 몇 바퀴만 돈다. 만드는 동안 동작을 차례로 확인하는 데 쓴다.
    pub fn play(&mut self, motion: MotionId) {
        self.mode = Mode::Act;
        self.motion = motion;
        self.t = 0.0;
        self.vy = 0.0;
        self.walk = None;
        self.turn_to = None;
        self.showcase = true;
        self.loops_left = SHOWCASE_LOOPS;
    }

    /// 시간을 `dt`초 흐르게 한다.
    pub fn update(&mut self, dt: f64, motions: &Motions, around: &Surroundings) {
        let dt = dt.clamp(0.0, MAX_STEP);
        match self.mode {
            Mode::Gone => return,
            Mode::Ground => self.ground_step(dt, motions, around),
            _ => self.air_step(dt, motions, around),
        }
        if around.left <= around.right {
            self.x = self.x.clamp(around.left, around.right);
        }
    }

    /// 장이 바뀌지 않아도 자리를 자주 고쳐야 하는가. 떨어지는 중이거나 끌려가는 중이면 그렇다.
    pub fn moves_smoothly(&self) -> bool {
        match self.mode {
            Mode::Fall => true,
            Mode::Drag => (self.drag.0 - self.x).abs() > 0.5 || (self.drag.1 - self.y).abs() > 0.5,
            // 잠깐 떠 있는 동안은 움직이지 않는다.
            Mode::Release => self.t >= RELEASE_HOLD_MS,
            Mode::Emerge => self.t >= SPAWN_DROP_MS,
            _ => false,
        }
    }

    /// 다음에 [`Actor::update`]를 불러야 할 때까지 남은 시간(초).
    ///
    /// 보통은 다음 장으로 넘어갈 때다. 떨어지거나 끌려가는 동안([`Actor::moves_smoothly`])에는
    /// 장이 바뀌지 않아도 자리를 고쳐야 해서 더 자주 부른다.
    pub fn wake_in(&self, motions: &Motions) -> f64 {
        let frame = motions.clip(self.motion).until_next_frame(self.t) / 1000.0;
        let until = |at_ms: f64| ((at_ms - self.t) / 1000.0).max(0.0);
        match self.mode {
            Mode::Gone => f64::INFINITY,
            _ if self.moves_smoothly() => frame.min(SMOOTH_STEP),
            // 떠 있는 시간이 끝날 때를 놓치지 않는다.
            Mode::Release => frame.min(until(RELEASE_HOLD_MS)),
            Mode::Emerge => frame.min(until(SPAWN_DROP_MS)),
            // 쉬기와 걷기를 마칠 때를 놓치지 않는다. idle은 장 하나가 0.25초라서 장만 기다리면 늦는다.
            Mode::Ground if !self.leaving => frame.min(self.wait.max(0.0)),
            _ => frame,
        }
    }

    fn at_edge(&self, dir: Dir, around: &Surroundings) -> bool {
        match dir {
            Dir::East => self.x >= around.right - 0.5,
            Dir::West => self.x <= around.left + 0.5,
        }
    }

    /// 반복 동작의 시간을 한 바퀴 안으로 되돌린다. 한 바퀴를 돌았으면 `true`다.
    fn wrap(&mut self, motions: &Motions) -> bool {
        let clip = motions.clip(self.motion);
        if clip.looping && self.t >= clip.total_ms {
            self.t %= clip.total_ms;
            return true;
        }
        false
    }

    /// `motion`으로 넘어가고, 상태도 그 동작에 맞춘다. `carried`는 앞 동작에서 넘친 시간(ms)이다.
    fn enter(&mut self, motion: MotionId, carried: f64, motions: &Motions) {
        self.mode = mode_of(motion, motions);
        self.motion = motion;
        self.t = carried.min(motions.clip(motion).total_ms / 2.0);
        if self.mode == Mode::Ground {
            self.rest(motions, SETTLE_REST);
        }
    }

    /// 바닥에서 idle2로 쉬기 시작한다.
    fn rest(&mut self, motions: &Motions, range: (f64, f64)) {
        self.mode = Mode::Ground;
        self.motion = motions.idle2;
        self.walk = None;
        self.turn_to = None;
        self.showcase = false;
        self.wait = self.rng.range(range);
    }

    /// 한 번짜리 동작이 끝났으면 팩에 적힌 다음 동작으로 넘어간다. 다음 동작이 없으면 사라진 것이다.
    fn follow_when_over(&mut self, motions: &Motions) {
        let clip = motions.clip(self.motion);
        if clip.looping {
            self.wrap(motions);
        } else if clip.is_over(self.t) {
            match clip.next {
                Some(next) => self.enter(next, self.t - clip.total_ms, motions),
                None => self.mode = Mode::Gone,
            }
        }
    }

    /// 중력으로 한 걸음 떨어진다. 바닥에 닿았으면 착지하고 `true`를 돌려준다.
    fn fall_step(&mut self, dt: f64, motions: &Motions, around: &Surroundings) -> bool {
        self.vy += GRAVITY * self.scale * dt;
        self.y += self.vy * dt;
        if self.vy > 0.0 && self.y >= around.floor {
            self.touch_down(motions, around);
            return true;
        }
        false
    }

    fn touch_down(&mut self, motions: &Motions, around: &Surroundings) {
        self.y = around.floor;
        self.vy = 0.0;
        // 내려앉은 곳이 새로 둘레를 돌아다닐 자리가 된다.
        self.home = self.x;
        self.mode = Mode::Land;
        self.motion = motions.land;
        self.t = 0.0;
    }

    /// 바닥에 있지 않을 때의 한 걸음.
    fn air_step(&mut self, dt: f64, motions: &Motions, around: &Surroundings) {
        self.t += dt * 1000.0;
        match self.mode {
            Mode::Drag => {
                // 목표와의 거리를 일정한 비율로 줄여 간다. 멀수록 빨리, 가까울수록 천천히 따라간다.
                let k = 1.0 - (-dt * DRAG_FOLLOW).exp();
                self.x += (self.drag.0 - self.x) * k;
                self.y += (self.drag.1 - self.y) * k;
                self.wrap(motions);
            }
            Mode::Release | Mode::Emerge => {
                let hold = if self.mode == Mode::Release { RELEASE_HOLD_MS } else { SPAWN_DROP_MS };
                if self.t >= hold && self.fall_step(dt, motions, around) {
                    return;
                }
                if self.y >= around.floor {
                    self.touch_down(motions, around);
                } else {
                    self.follow_when_over(motions);
                }
            }
            Mode::Fall => {
                if !self.fall_step(dt, motions, around) {
                    self.wrap(motions);
                }
            }
            Mode::Act => self.act_step(motions),
            _ => self.follow_when_over(motions),
        }
    }

    /// 제자리 동작의 한 걸음. 끝나면 다음 동작으로 넘어가거나 쉬러 간다.
    fn act_step(&mut self, motions: &Motions) {
        let clip = motions.clip(self.motion);
        let over = if clip.looping {
            // 반복 동작은 정한 바퀴 수만 돈다.
            self.wrap(motions) && {
                self.loops_left = self.loops_left.saturating_sub(1);
                self.loops_left == 0
            }
        } else {
            clip.is_over(self.t)
        };
        if !over {
            return;
        }
        let carried = if clip.looping { self.t } else { self.t - clip.total_ms };
        match clip.next.filter(|&next| mode_of(next, motions) != Mode::Ground) {
            // 차례로 보여 줄 때는 떨어지거나 잡히는 동작도 제자리에서 그림만 보여 준다.
            Some(next) if self.showcase => {
                self.motion = next;
                self.t = carried.min(motions.clip(next).total_ms / 2.0);
                self.loops_left = SHOWCASE_LOOPS;
            }
            Some(next) => self.enter(next, carried, motions),
            None => self.rest(motions, REST),
        }
    }

    /// 바닥에서의 한 걸음. 쉬다가 걷고, 벽에 닿거나 걷기를 마치면 선다.
    fn ground_step(&mut self, dt: f64, motions: &Motions, around: &Surroundings) {
        // 바닥이 낮아졌으면(다른 모니터로 옮겨졌거나 작업표시줄이 바뀜) 떨어지고, 높아졌으면 올라선다.
        if self.y < around.floor - 0.5 {
            self.mode = Mode::Fall;
            self.motion = motions.fall;
            self.t = 0.0;
            self.vy = 0.0;
            self.walk = None;
            self.turn_to = None;
            return;
        }
        self.y = around.floor;

        if self.leaving {
            self.walk = None;
            self.turn_to = None;
            if self.motion != motions.idle2 {
                self.motion = motions.idle2;
                self.t = 0.0;
            } else {
                self.t += dt * 1000.0;
                self.wrap(motions);
            }
            // 사라지는 그림의 첫 장은 idle2의 첫 장에서 이어지게 그려져 있다. idle2가 첫 장일 때 바꾼다.
            if motions.clip(motions.idle2).frame_at(self.t) == 0 {
                self.mode = Mode::Vanish;
                self.motion = motions.despawn;
                self.t = 0.0;
            }
            return;
        }

        self.wait -= dt;
        if let Some(dir) = self.walk {
            if self.at_edge(dir, around) {
                // 벽에 닿으면 잠깐 섰다가 돌아선다.
                self.walk = None;
                self.turn_to = Some(dir.opposite());
                self.wait = self.rng.range(TURN_PAUSE);
            } else if self.wait <= 0.0 {
                self.walk = None;
                if self.rng.chance(TURN_CHANCE) {
                    self.turn_to = Some(dir.opposite());
                    self.wait = self.rng.range(TURN_PAUSE);
                } else {
                    self.wait = self.rng.range(REST);
                }
            }
        } else if let Some(dir) = self.turn_to {
            if self.wait <= 0.0 {
                self.turn_to = None;
                if self.roam && !self.at_edge(dir, around) {
                    self.walk = Some(dir);
                    self.wait = self.rng.range(WALK);
                } else {
                    self.wait = self.rng.range(REST);
                }
            }
        } else if self.wait <= 0.0 {
            if self.roam {
                let mut dir = if self.rng.chance(0.5) { Dir::West } else { Dir::East };
                // 자리에서 너무 멀어졌으면 돌아오는 쪽으로 걷는다.
                if (self.x - self.home).abs() > WANDER * self.scale {
                    dir = if self.x > self.home { Dir::West } else { Dir::East };
                }
                if self.at_edge(dir, around) {
                    dir = dir.opposite();
                }
                self.walk = Some(dir);
                self.wait = self.rng.range(WALK);
            } else {
                self.wait = self.rng.range(REST);
            }
        }

        let want = match (self.walk, self.turn_to) {
            (Some(Dir::West), _) => motions.walk_w,
            (Some(Dir::East), _) => motions.walk_e,
            (None, Some(_)) => motions.idle,
            (None, None) => motions.idle2,
        };
        if want != self.motion {
            // 동작이 바뀌면 첫 장부터 보여 준다.
            self.motion = want;
            self.t = 0.0;
        } else {
            self.t += dt * 1000.0;
            self.wrap(motions);
        }
        if let Some(dir) = self.walk {
            // 한 바퀴에 정해진 거리만큼 간다. 디딘 발이 뒤로 밀리는 빠르기와 같아야 발이 미끄러지지 않는다.
            let clip = motions.clip(want);
            let speed = clip.stride * self.scale / (clip.total_ms / 1000.0);
            self.x += dir.sign() * speed * dt;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::motions::tests::sample;

    const AROUND: Surroundings = Surroundings { floor: 1000.0, left: 100.0, right: 1900.0, top: 150.0 };
    /// 시험에서 시간을 흘리는 걸음(초). 앱은 1/24초나 1/60초마다 부른다.
    const STEP: f64 = 1.0 / 60.0;

    /// `seconds` 동안 시간을 흘리고, 그사이에 거친 (상태, 동작 이름)을 차례로 돌려준다.
    fn run(actor: &mut Actor, motions: &Motions, around: &Surroundings, seconds: f64) -> Vec<(Mode, String)> {
        let mut seen = vec![(actor.mode(), motions.clip(actor.motion()).name.clone())];
        for _ in 0..(seconds / STEP).round() as usize {
            actor.update(STEP, motions, around);
            let now = (actor.mode(), motions.clip(actor.motion()).name.clone());
            if seen.last() != Some(&now) {
                seen.push(now);
            }
        }
        seen
    }

    /// 거친 동작의 이름. 같은 동작에서 상태만 바뀐 것은 한 번만 센다.
    fn names(seen: &[(Mode, String)]) -> Vec<&str> {
        let mut names: Vec<&str> = seen.iter().map(|(_, name)| name.as_str()).collect();
        names.dedup();
        names
    }

    #[test]
    fn appearing_ends_standing_on_the_floor() {
        let motions = sample();
        let mut actor = Actor::appear(&motions, 500.0, 400.0, 1.0, 1);
        let seen = run(&mut actor, &motions, &AROUND, 4.0);
        assert_eq!(names(&seen[..6]), ["spawn", "spawn_to_fall", "fall", "land", "land_to_idle2", "idle2"]);
        let modes: Vec<Mode> = seen[..6].iter().map(|(mode, _)| *mode).collect();
        assert_eq!(modes, [Mode::Spawn, Mode::Emerge, Mode::Fall, Mode::Land, Mode::Settle, Mode::Ground]);
        assert_eq!(actor.position().1, AROUND.floor);
    }

    #[test]
    fn spawn_hovers_until_the_light_is_done() {
        let motions = sample();
        let mut actor = Actor::appear(&motions, 500.0, 400.0, 1.0, 1);
        // spawn(1초) 동안과, 그 뒤 떨어지기 시작하기 전(0.3초)까지는 그 자리에 떠 있다.
        run(&mut actor, &motions, &AROUND, 1.25);
        assert_eq!(actor.mode(), Mode::Emerge);
        assert_eq!(actor.position(), (500.0, 400.0));
        run(&mut actor, &motions, &AROUND, 0.2);
        assert!(actor.position().1 > 400.0);
    }

    #[test]
    fn fall_follows_gravity() {
        let motions = sample();
        let mut actor = Actor::stand(&motions, 500.0, 400.0, 1.0, 1);
        // 바닥보다 위에 서 있으면 떨어지기 시작한다.
        actor.update(STEP, &motions, &AROUND);
        assert_eq!(actor.mode(), Mode::Fall);
        // 600px을 떨어지는 데 걸리는 시간은 sqrt(2 × 600 / 1500) = 0.894초다.
        let mut seconds = 0.0;
        while actor.mode() == Mode::Fall {
            actor.update(STEP, &motions, &AROUND);
            seconds += STEP;
        }
        assert!((seconds - 0.894).abs() < 0.03, "{seconds}");
        assert_eq!((actor.mode(), actor.position().1), (Mode::Land, AROUND.floor));
    }

    #[test]
    fn bigger_scale_falls_faster_in_pixels() {
        // 2배로 보여 줄 때는 같은 화면 거리를 더 빨리 떨어진다(그림 기준으로는 같은 빠르기).
        let motions = sample();
        let time_to_land = |scale: f64| {
            let mut actor = Actor::stand(&motions, 500.0, 400.0, scale, 1);
            let mut seconds = 0.0;
            while actor.mode() != Mode::Land {
                actor.update(STEP, &motions, &AROUND);
                seconds += STEP;
            }
            seconds
        };
        assert!(time_to_land(2.0) < time_to_land(1.0) * 0.75);
    }

    #[test]
    fn resting_and_walking_alternate_and_stay_in_range() {
        let motions = sample();
        let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 7);
        let (mut low, mut high) = (f64::MAX, f64::MIN);
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..(600.0 / STEP) as usize {
            actor.update(STEP, &motions, &AROUND);
            assert_eq!(actor.mode(), Mode::Ground);
            low = low.min(actor.position().0);
            high = high.max(actor.position().0);
            seen.insert(motions.clip(actor.motion()).name.clone());
        }
        // 쉬기, 양쪽 걷기, 돌아설 때의 idle이 모두 나온다.
        assert_eq!(seen.into_iter().collect::<Vec<_>>(), ["idle", "idle2", "walk_e", "walk_w"]);
        // 자리(1000)에서 160px 넘게 멀어지면 돌아오므로, 한 번 걷는 거리(40px/s × 2.6초)보다 더 멀리는 못 간다.
        let reach = WANDER + 40.0 * WALK.1 + 1.0;
        assert!(low >= 1000.0 - reach && high <= 1000.0 + reach, "{low}..{high}");
        assert!(high - low > 100.0, "거의 움직이지 않음: {low}..{high}");
    }

    #[test]
    fn walking_speed_matches_the_stride() {
        let motions = sample();
        let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.5, 3);
        actor.walk = Some(Dir::East);
        actor.wait = 10.0;
        run(&mut actor, &motions, &AROUND, 1.0);
        // 한 바퀴(1초)에 40px, 1.5배로 보여 주므로 1초에 60px.
        assert!((actor.position().0 - 1060.0).abs() < 1.5, "{}", actor.position().0);
    }

    #[test]
    fn wall_stops_the_walk_and_turns_around() {
        let motions = sample();
        let mut actor = Actor::stand(&motions, AROUND.right - 20.0, AROUND.floor, 1.0, 3);
        actor.walk = Some(Dir::East);
        actor.wait = 10.0;
        let seen = run(&mut actor, &motions, &AROUND, 3.0);
        // 벽까지 걷고, idle로 잠깐 섰다가, 반대쪽으로 걷는다. 맨 앞은 걷기 전에 서 있던 모습이다.
        assert_eq!(names(&seen)[..4], ["idle2", "walk_e", "idle", "walk_w"]);
        assert!(actor.position().0 <= AROUND.right);
    }

    #[test]
    fn standing_still_when_roaming_is_off() {
        let motions = sample();
        let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 7);
        actor.set_roam(false);
        let seen = run(&mut actor, &motions, &AROUND, 60.0);
        assert_eq!(names(&seen), ["idle2"]);
        assert_eq!(actor.position().0, 1000.0);
    }

    #[test]
    fn grab_hangs_and_release_drops() {
        let motions = sample();
        let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 7);
        assert!(actor.grab(&motions, (1200.0, 500.0)));
        run(&mut actor, &motions, &AROUND, 0.5);
        // 잡은 점을 바짝 따라간다.
        assert_eq!(actor.mode(), Mode::Drag);
        let (x, y) = actor.position();
        assert!((x - 1200.0).abs() < 1.0 && (y - 500.0).abs() < 1.0, "({x}, {y})");

        actor.release(&motions);
        // 놓으면 0.36초 동안 그 자리에 떠 있다가 떨어진다.
        run(&mut actor, &motions, &AROUND, 0.3);
        assert_eq!(actor.mode(), Mode::Release);
        assert!((actor.position().1 - 500.0).abs() < 1.0);
        let seen = run(&mut actor, &motions, &AROUND, 3.0);
        assert_eq!(names(&seen), ["hang_to_fall", "fall", "land", "land_to_idle2", "idle2"]);
        // 놓은 자리 아래가 새로 돌아다닐 자리가 된다.
        assert!((actor.home - 1200.0).abs() < 1.0);
    }

    #[test]
    fn can_be_grabbed_again_while_falling() {
        let motions = sample();
        let mut actor = Actor::stand(&motions, 1000.0, 300.0, 1.0, 7);
        run(&mut actor, &motions, &AROUND, 0.3);
        assert_eq!(actor.mode(), Mode::Fall);
        assert!(actor.grab(&motions, (1000.0, 300.0)));
        assert_eq!(actor.mode(), Mode::Drag);
    }

    #[test]
    fn cannot_be_grabbed_while_appearing_landing_or_vanishing() {
        let motions = sample();
        let mut appearing = Actor::appear(&motions, 500.0, 400.0, 1.0, 1);
        assert!(!appearing.grab(&motions, (0.0, 0.0)));
        let mut landing = Actor::stand(&motions, 500.0, AROUND.floor - 5.0, 1.0, 1);
        run(&mut landing, &motions, &AROUND, 0.2);
        assert_eq!(landing.mode(), Mode::Land);
        assert!(!landing.grab(&motions, (0.0, 0.0)));
        let mut vanishing = Actor::stand(&motions, 500.0, AROUND.floor, 1.0, 1);
        vanishing.leave();
        vanishing.update(STEP, &motions, &AROUND);
        assert_eq!(vanishing.mode(), Mode::Vanish);
        assert!(!vanishing.grab(&motions, (0.0, 0.0)));
    }

    #[test]
    fn releasing_on_the_floor_lands_at_once() {
        let motions = sample();
        let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 7);
        actor.grab(&motions, (1000.0, AROUND.floor));
        run(&mut actor, &motions, &AROUND, 0.2);
        actor.release(&motions);
        actor.update(STEP, &motions, &AROUND);
        assert_eq!(actor.mode(), Mode::Land);
    }

    #[test]
    fn leaving_waves_goodbye_then_is_gone() {
        let motions = sample();
        let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 7);
        actor.walk = Some(Dir::East);
        actor.wait = 10.0;
        run(&mut actor, &motions, &AROUND, 0.1);
        actor.leave();
        let seen = run(&mut actor, &motions, &AROUND, 3.0);
        assert_eq!(names(&seen), ["walk_e", "despawn"]);
        assert_eq!(seen.last().map(|(mode, _)| *mode), Some(Mode::Gone));
        assert!(actor.is_gone());
        assert_eq!(actor.shadow(), 0.0);
    }

    #[test]
    fn leaving_in_the_air_waits_for_the_landing() {
        let motions = sample();
        let mut actor = Actor::stand(&motions, 1000.0, 700.0, 1.0, 7);
        actor.leave();
        let seen = run(&mut actor, &motions, &AROUND, 6.0);
        assert_eq!(names(&seen), ["idle2", "fall", "land", "land_to_idle2", "idle2", "despawn"]);
        assert!(actor.is_gone());
    }

    #[test]
    fn shadow_fades_in_and_out() {
        let motions = sample();
        let mut actor = Actor::appear(&motions, 500.0, 400.0, 1.0, 1);
        assert_eq!(actor.shadow(), 0.0);
        run(&mut actor, &motions, &AROUND, 0.675);
        assert!((actor.shadow() - 0.5).abs() < 0.05, "{}", actor.shadow());
        run(&mut actor, &motions, &AROUND, 0.3);
        assert_eq!(actor.shadow(), 1.0);

        let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 7);
        actor.leave();
        run(&mut actor, &motions, &AROUND, 1.0);
        assert_eq!(actor.shadow(), 1.0);
        run(&mut actor, &motions, &AROUND, 0.3);
        assert!(actor.shadow() > 0.2 && actor.shadow() < 0.8, "{}", actor.shadow());
    }

    #[test]
    fn playing_a_motion_follows_next_back_to_idle2() {
        let motions = sample();
        let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 7);
        // 한 번짜리: 끝나면 idle2로.
        actor.play(motions.find("stretch").unwrap());
        assert_eq!(names(&run(&mut actor, &motions, &AROUND, 1.0)), ["stretch", "idle2"]);
        // 이어지는 동작: edge_in → edge_loop(두 바퀴) → edge_out → idle2
        actor.play(motions.find("edge_in").unwrap());
        let seen = run(&mut actor, &motions, &AROUND, 2.0);
        assert_eq!(names(&seen), ["edge_in", "edge_loop", "edge_out", "idle2"]);
        // 떨어지는 동작을 보여 줄 때도 제자리에 있다.
        actor.play(motions.hang_to_fall);
        let seen = run(&mut actor, &motions, &AROUND, 3.0);
        assert_eq!(names(&seen), ["hang_to_fall", "fall", "land", "land_to_idle2", "idle2"]);
        assert_eq!(actor.position(), (1000.0, AROUND.floor));
    }

    #[test]
    fn floor_change_is_followed() {
        let motions = sample();
        let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 7);
        // 바닥이 높아지면 올라서고, 낮아지면 떨어져 내려간다.
        let higher = Surroundings { floor: 900.0, ..AROUND };
        actor.update(STEP, &motions, &higher);
        assert_eq!((actor.mode(), actor.position().1), (Mode::Ground, 900.0));
        let seen = run(&mut actor, &motions, &AROUND, 2.0);
        assert_eq!(names(&seen[..3]), ["idle2", "fall", "land"]);
        assert_eq!(actor.position().1, AROUND.floor);
    }

    #[test]
    fn long_pause_does_not_fast_forward() {
        // 절전에서 깨어난 뒤처럼 한참 만에 불려도 0.5초만 흐른 것으로 친다.
        let motions = sample();
        let mut actor = Actor::stand(&motions, 500.0, 400.0, 1.0, 1);
        actor.update(STEP, &motions, &AROUND);
        actor.update(3600.0, &motions, &AROUND);
        // 0.5초 동안 떨어지는 거리는 1500 × 0.5² ≈ 375px이다. 바닥(600px 아래)까지 가지 못한다.
        assert_eq!(actor.mode(), Mode::Fall);
        assert!(actor.position().1 < 800.0);
    }

    #[test]
    fn long_frames_do_not_slow_time_down() {
        // idle은 장 하나가 0.25초다. 그 간격으로 불려도 쉬는 시간이 제대로 줄어야 한다.
        let motions = sample();
        let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 7);
        actor.turn_to = Some(Dir::East);
        actor.wait = 1.0;
        for _ in 0..4 {
            actor.update(0.25, &motions, &AROUND);
        }
        actor.update(0.01, &motions, &AROUND);
        assert_eq!(actor.walk, Some(Dir::East));
    }

    #[test]
    fn wake_time_depends_on_what_it_is_doing() {
        let motions = sample();
        // 서 있을 때는 다음 장까지 잔다(idle2의 장은 0.1초).
        let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 7);
        actor.wait = 5.0;
        assert!((actor.wake_in(&motions) - 0.1).abs() < 1e-9);
        // 떨어질 때는 1/60초마다 깬다.
        let mut falling = Actor::stand(&motions, 1000.0, 300.0, 1.0, 7);
        falling.update(STEP, &motions, &AROUND);
        assert!((falling.wake_in(&motions) - SMOOTH_STEP).abs() < 1e-9);
        // 잡혀서 목표에 닿아 있으면 장만 넘긴다.
        actor.grab(&motions, (1000.0, AROUND.floor));
        assert!((actor.wake_in(&motions) - 0.1).abs() < 1e-9);
        actor.drag_to((1300.0, 500.0));
        assert!((actor.wake_in(&motions) - SMOOTH_STEP).abs() < 1e-9);
    }

    #[test]
    fn foot_shift_moves_the_picture_not_the_position() {
        let motions = sample();
        let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 2.0, 7);
        assert_eq!(actor.foot(&motions), (1000, 1000));
        actor.motion = motions.walk_w;
        // 걷는 그림은 5px(2배에서는 10px) 옮겨 그린다. 자리 자체는 그대로다.
        assert_eq!(actor.foot(&motions), (1010, 1000));
        assert_eq!(actor.position().0, 1000.0);
    }

    #[test]
    fn random_numbers_stay_in_range() {
        let mut rng = Rng::new(0);
        for _ in 0..1000 {
            let v = rng.range((1.5, 4.0));
            assert!((1.5..4.0).contains(&v));
        }
        assert!((0..1000).filter(|_| rng.chance(0.25)).count().abs_diff(250) < 60);
    }
}
