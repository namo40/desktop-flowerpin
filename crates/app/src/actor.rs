//! 꽃핀 한 명의 상태 기계. 지금 무엇을 하고 있고, 시간이 흐르면 무엇으로 넘어가는지를 정한다.
//!
//! 창이나 그림을 모른다. 시간(`dt`)과 둘레의 상황([`Surroundings`])을 받아 자리와 동작만 바꾼다.
//! 그래서 화면 없이 단위 테스트로 규칙을 확인할 수 있다.
//!
//! ```text
//! 나타나기: spawn → spawn_to_fall → fall(반복) → land → land_to_idle2 → idle2
//! 쉬기·걷기: idle2 ⇄ walk   (돌아설 때는 idle로 잠깐 선다)
//! 잡기:     hang(반복) → 놓으면 hang_to_fall → fall → land → …
//! 클릭:     poke_surprise | poke_pout | poke_shy (바로 전과 다른 것) → idle2
//! 쓰다듬기: 머리를 문지른 거리가 쌓이면 pat → idle2
//! 가끔 혼자: stretch | yawn → idle2,  jump → (떠 있으면 fall) → land → …
//! 잠:       sleep_in → sleep_loop(6~12초, 자리를 비웠으면 돌아올 때까지) → sleep_out → idle2
//! 걸터앉기: 단 위에서 edge_in → edge_loop(3~6바퀴) → edge_out → idle2
//! 창 위로:  가까운 창 윗변으로 jump(높이에 맞춰 한 번에) → land → 그 위에서 쉬고 걷는다
//! 창에서:   jump로 뛰어내려 fall → 바닥에 land
//! 사라지기: idle2가 첫 장으로 돌아올 때 despawn → 없어짐
//! ```
//!
//! 다른 창의 윗변([`Perch`])은 바닥보다 높이 있는 또 하나의 바닥이다. 올라서면 그 윗변의 양 끝이 벽이 되고,
//! 창이 움직이면 따라간다. 창이 없어지거나 선 자리가 가려지면 바닥까지 떨어진다.
//!
//! 다른 꽃핀의 머리도 [`Perch`]로 받는다. 머리 위에서는 걷거나 앉거나 자지 않고, 쉬다가 가끔 기지개나
//! 하품을 하고 옆으로 뛰어내린다. 머리에 누가 서 있는 꽃핀은 쉬고 걷기만 한다([`Actor::set_carrying`]).
//!
//! 한 번짜리 동작이 끝나면 팩에 적힌 "다음 동작"으로 넘어간다. 어떤 동작이 어떤 상태인지는
//! [`mode_of`]가 정한다. 규칙과 값은 원본 데모에서 가져왔다.
//!
//! 좌표는 화면 픽셀이고 y는 아래로 갈수록 커진다. 꽃핀의 자리는 발바닥 가운데다.
//! 길이와 빠르기 상수는 그림 1배 크기 기준이라, 쓸 때 표시 배율을 곱한다.

use crate::motions::{MotionId, Motions};
use crate::perch::Perch;

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
/// 점프: 박차는 장부터 이 빠르기(px/s)로 띄우고 이 중력(px/s²)으로 내려온다. 점프 그림이 끝날 즈음 바닥에 닿는다.
const JUMP_SPEED: f64 = 480.0;
const JUMP_GRAVITY: f64 = 720.0;
/// 팩에 박차는 시각이 적혀 있지 않을 때 쓰는 값(ms).
const JUMP_LAUNCH_MS: f64 = 750.0;
/// 한 번 잠들면 자는 시간(초).
const SLEEP_FOR: (f64, f64) = (6.0, 12.0);
/// 제자리 동작을 마친 뒤 쉬는 시간(초).
const AFTER_ACT: (f64, f64) = (1.5, 3.5);
/// 쉬기를 마쳤을 때 가끔 하는 행동과 그 확률. 나머지는 걷는다.
const WHIMS: [(Whim, f64); 4] = [(Whim::Stretch, 0.10), (Whim::Yawn, 0.10), (Whim::Jump, 0.07), (Whim::Sleep, 0.05)];
/// 단 위에 있을 때 걸터앉을 확률. 위의 행동들보다 먼저 따진다.
const SIT_CHANCE: f64 = 0.10;
/// 걸터앉아 다리를 흔드는 횟수.
const SIT_LOOPS: (u32, u32) = (3, 6);
/// 창 윗변으로 뛰어오를 때 떠 있는 시간(초). 제자리 점프와 같게 해서 점프 그림과 맞춘다.
/// 높이 뛸수록 더 세게 박차고 더 빨리 내려온다.
const LEAP_AIR: f64 = 2.0 * JUMP_SPEED / JUMP_GRAVITY;
/// 뛰어오를 때 내려앉을 곳보다 이만큼(px) 더 높이 떴다가 내려앉는다.
const LEAP_CLEAR: f64 = 28.0;
/// 가로로 이만큼(px) 안에 있는 윗변으로만 뛰어오른다.
const LEAP_REACH: f64 = 320.0;
/// 쉬기를 마쳤을 때 가까운 창 윗변으로 뛰어오를 확률과, 창 위에서 뛰어내릴 확률.
const CLIMB_CHANCE: f64 = 0.10;
const HOP_CHANCE: f64 = 0.08;
/// 창 윗변의 끝까지 걸어갔을 때 돌아서지 않고 뛰어내릴 확률.
const HOP_AT_END: f64 = 0.35;
/// 끝에서 뛰어내릴 때 바깥쪽으로 가는 빠르기(px/s).
const HOP_SPEED: f64 = 110.0;
/// 놓았을 때 발바닥이 윗변에서 이만큼(px) 안에 있으면 그 윗변에 올려놓은 것으로 친다.
const PERCH_SNAP: f64 = 24.0;
/// 올라선 창이 움직이면 이만큼(초) 동안 자리를 자주 고친다. 창을 끌고 다닐 때 뒤처져 보이지 않게 한다.
const RIDE_SMOOTH: f64 = 0.3;
/// 올라선 창을 흔들면 떨어진다. 창이 한 방향으로 이만큼(px) 넘게 움직였다가 방향을 바꾸는 것을, 바꿀 때마다
/// 이만큼(초) 안에 이어서 이만큼(번) 하면 흔든 것으로 친다. 창을 끌어 옮기다 조금 되돌리는 정도로는 떨어지지 않는다.
const SHAKE_LEG: f64 = 24.0;
const SHAKE_GAP: f64 = 0.6;
const SHAKE_TURNS: u32 = 3;
/// 커서 아래로 걸어갈 때, 그 자리에서 이만큼(px) 안에 오면 멈춘다.
const ARRIVE: f64 = 24.0;
/// 다른 꽃핀의 머리 위에서 쉬기를 마쳤을 때 뛰어내릴 확률.
const HEAD_HOP_CHANCE: f64 = 0.25;
/// 머리를 문지른 거리가 이만큼(px) 쌓이면 쓰다듬은 것으로 친다. 이 시간(초) 동안 문지르지 않으면 쌓인 거리를 버린다.
const PAT_DISTANCE: f64 = 200.0;
const PAT_KEEP: f64 = 0.7;

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
    /// 제자리에서 뛰어오른다.
    Jump,
    /// 졸다가 잠들고, 자고, 깬다.
    Sleep,
    /// 단의 앞 가장자리에 걸터앉아 다리를 흔든다.
    Sit,
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

/// 쉬는 동안 가끔 하는 행동.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Whim {
    Stretch,
    Yawn,
    Jump,
    Sleep,
    /// 단의 앞 가장자리에 걸터앉기. 단 위에 있을 때만 고른다.
    Sit,
    /// 가까운 창의 윗변으로 뛰어오르기. 뛰어오를 곳이 있을 때만 고른다.
    Climb,
    /// 올라서 있던 창에서 뛰어내리기. 창 위에 있을 때만 고른다.
    Hop,
}

/// 사용자가 메뉴에서 골라 볼 수 있는 행동.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Show {
    Stretch,
    Yawn,
    Jump,
    Sleep,
    Sit,
    Climb,
    Hop,
    Surprise,
    Pout,
    Shy,
    Pat,
}

impl Show {
    /// 번호로 고를 때의 차례.
    pub const ALL: [Show; 11] = [
        Show::Stretch,
        Show::Yawn,
        Show::Jump,
        Show::Sleep,
        Show::Sit,
        Show::Climb,
        Show::Hop,
        Show::Surprise,
        Show::Pout,
        Show::Shy,
        Show::Pat,
    ];

    /// 행동의 이름. 확인용 기록에 적는다.
    pub fn label(self) -> &'static str {
        match self {
            Show::Stretch => "기지개",
            Show::Yawn => "하품",
            Show::Jump => "점프",
            Show::Sleep => "잠자기",
            Show::Sit => "걸터앉기",
            Show::Climb => "창 위로 올라가기",
            Show::Hop => "뛰어내리기",
            Show::Surprise => "놀라기",
            Show::Pout => "삐치기",
            Show::Shy => "수줍어하기",
            Show::Pat => "쓰다듬 받기",
        }
    }
}

/// 창 윗변으로 뛰어오르는 길.
///
/// 떠 있는 시간([`LEAP_AIR`])이 정해져 있어서, 박찬 뒤 흐른 시간만으로 자리가 정해진다. 걸음마다 조금씩
/// 더해 가지 않으므로, 걸음이 크든 작든 정한 자리에 꼭 내려앉는다.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Leap {
    /// 내려앉을 창.
    window: isize,
    /// 박찬 자리. 박찰 때 적는다.
    from: (f64, f64),
    /// 옆으로 가는 빠르기와 박차는 빠르기(px/s), 중력(px/s²). 화면 px 기준이다.
    vx: f64,
    speed: f64,
    gravity: f64,
    /// 박찬 뒤 흐른 시간(초).
    t: f64,
}

/// 올라선 창을 흔드는지 알아본다. 창이 움직인 만큼을 걸음마다 받는다.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Shake {
    /// 가로와 세로마다, 지금 움직이는 방향(-1, 0, 1)과 그 방향으로 움직인 거리(px).
    axes: [(f64, f64); 2],
    /// 이어서 방향을 바꾼 횟수와, 마지막으로 바꾼 뒤 흐른 시간(초).
    turns: u32,
    quiet: f64,
}

impl Shake {
    /// 창이 `dt`초 동안 `moved`만큼 움직였다. `leg`는 방향을 바꾼 것으로 치려면 그 전에 움직였어야 하는 거리다.
    /// 흔든 것으로 치면 `true`를 돌려준다.
    fn feed(&mut self, dt: f64, moved: (f64, f64), leg: f64) -> bool {
        self.quiet += dt;
        if self.quiet > SHAKE_GAP {
            self.turns = 0;
        }
        for (axis, d) in self.axes.iter_mut().zip([moved.0, moved.1]) {
            if d.abs() < 0.01 {
                continue;
            }
            let dir = d.signum();
            if dir == axis.0 {
                axis.1 += d.abs();
                continue;
            }
            if axis.0 != 0.0 && axis.1 >= leg {
                self.turns += 1;
                self.quiet = 0.0;
            }
            *axis = (dir, d.abs());
        }
        self.turns >= SHAKE_TURNS
    }
}

/// 꽃핀의 버릇.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Habits {
    /// 혼자 돌아다닌다. 늘 켜 둔다. 만드는 동안 가만히 세워 두고 볼 때만 끈다.
    pub roam: bool,
    /// 가끔 하는 행동의 빈도. 확률에 곱한다. 1이 보통이고, 0이면 하지 않는다. 설정에서 바꾼다.
    pub whim_rate: f64,
}

impl Default for Habits {
    fn default() -> Habits {
        Habits { roam: true, whim_rate: 1.0 }
    }
}

/// 꽃핀 둘레의 상황. 꽃핀이 있는 모니터와, 그 모니터에 떠 있는 다른 창들에서 나온다.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surroundings<'a> {
    /// 바닥선의 y. 발바닥이 여기에 놓인다.
    pub floor: f64,
    /// 걸어갈 수 있는 x의 범위(벽). 꽃핀은 이 사이를 끝에서 끝까지 돌아다닌다.
    pub left: f64,
    pub right: f64,
    /// 들어 올릴 수 있는 가장 높은 자리(발바닥의 y). 머리가 화면 위로 나가지 않게 한다.
    pub top: f64,
    /// 바닥이 단의 윗변이다(작업표시줄 위에 서 있다). 그러면 앞 가장자리에 걸터앉을 수 있다.
    /// 바닥이 화면의 아래 끝이면 걸터앉은 다리가 화면 밖으로 나가므로 앉지 않는다.
    pub ledge: bool,
    /// 올라설 수 있는 다른 창의 윗변들.
    pub perches: &'a [Perch],
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

    pub fn range(&mut self, (low, high): (f64, f64)) -> f64 {
        low + (high - low) * self.unit()
    }

    pub fn chance(&mut self, probability: f64) -> bool {
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
        m if m == motions.jump => Mode::Jump,
        m if [motions.sleep_in, motions.sleep_loop, motions.sleep_out].contains(&m) => Mode::Sleep,
        m if [motions.edge_in, motions.edge_loop, motions.edge_out].contains(&m) => Mode::Sit,
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
    /// 떠 있는 동안 옆으로 가는 빠르기(px/s). 창으로 뛰어오르거나 창 끝에서 뛰어내릴 때만 0이 아니다.
    vx: f64,
    /// 올라서 있는 창과, 마지막으로 봤을 때 그 창의 왼쪽 끝. 바닥에 있거나 떠 있으면 `None`이다.
    on: Option<(isize, f64)>,
    /// 올라서 있는 곳이 다른 꽃핀의 머리다. `on`이 있을 때만 뜻이 있다.
    head: bool,
    /// 머리에 다른 꽃핀이 서 있다. 그동안에는 쉬고 걷기만 한다.
    carrying: bool,
    /// 떨어지는 동안 창 윗변을 지나쳐 바닥까지 간다. 스스로 뛰어내렸거나 딛고 있던 윗변이 없어졌을 때다.
    through: bool,
    /// 창 윗변으로 뛰어오르는 길. 뛰어오르는 동안에만 있다.
    leap: Option<Leap>,
    /// 뛰어내릴 방향. 없으면 제자리에서 뛰어 앞으로 내려온다.
    hop_dir: Option<Dir>,
    /// 올라선 창이 방금 움직였다. 이 시간(초) 동안 자리를 자주 고친다.
    riding: f64,
    /// 올라선 창을 흔드는지 본다.
    shake: Shake,
    /// 표시 배율. 길이와 빠르기 상수에 곱한다.
    scale: f64,
    /// 마지막으로 내려앉은 곳.
    home: f64,
    /// 걷고 있는 방향.
    walk: Option<Dir>,
    /// 걸어가서 멈출 자리(x). 커서 아래로 걸어갈 때만 있다.
    goal: Option<f64>,
    /// idle로 잠깐 선 뒤에 걸을 방향.
    turn_to: Option<Dir>,
    /// 지금 하는 일(쉬기, 걷기, 잠깐 서기)을 마칠 때까지 남은 시간(초).
    wait: f64,
    /// 끌려가는 목표(발바닥의 자리).
    drag: (f64, f64),
    habits: Habits,
    /// 사라지라는 말을 들었다. 바닥에 서면 손을 흔들고 사라진다.
    leaving: bool,
    /// 바로 전에 한 클릭 반응. 같은 반응이 연달아 나오지 않게 한다.
    last_poke: Option<MotionId>,
    /// 머리를 문지른 거리(px)와, 마지막으로 문지른 뒤 흐른 시간(초).
    pat_distance: f64,
    pat_quiet: f64,
    /// 점프에서 몸을 띄웠는가.
    launched: bool,
    /// 잠에서 깰 때까지 남은 시간(초). 자리를 비운 동안의 잠은 끝이 없다.
    sleep_left: f64,
    /// 걸터앉아 다리를 몇 번 더 흔들지.
    sit_left: u32,
    /// 사용자가 자리를 비웠다. 쉬게 되면 잠들고, 돌아올 때까지 잔다.
    away: bool,
    /// idle2가 첫 장으로 돌아오면 시작할 행동.
    pending: Option<Whim>,
    /// 동작을 차례로 보여 주는 중이다([`Actor::play`]).
    showcase: bool,
    /// 반복 동작을 몇 바퀴 더 돌지.
    loops_left: u32,
    /// 혼자 시작한 가끔 하는 행동. 앱이 꺼내 가서 옆 꽃핀이 따라 하게 한다.
    started: Option<Whim>,
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
            vx: 0.0,
            on: None,
            head: false,
            carrying: false,
            through: false,
            leap: None,
            hop_dir: None,
            riding: 0.0,
            shake: Shake::default(),
            scale,
            home: x,
            walk: None,
            goal: None,
            turn_to: None,
            wait,
            drag: (x, y),
            habits: Habits::default(),
            leaving: false,
            last_poke: None,
            pat_distance: 0.0,
            pat_quiet: 0.0,
            launched: false,
            sleep_left: 0.0,
            sit_left: 0,
            away: false,
            pending: None,
            showcase: false,
            loops_left: 0,
            started: None,
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

    /// 마지막으로 내려앉은 곳. 다시 켰을 때 나타날 자리로 쓴다.
    pub fn home(&self) -> f64 {
        self.home
    }

    /// 올라서 있는 창. 바닥에 있거나 떠 있으면 `None`이다.
    pub fn support(&self) -> Option<isize> {
        self.on.map(|(window, _)| window)
    }

    /// 가로 자리 `x`로 걸어가서 멈춘다. 바닥이나 창 위에서 쉬거나 걷고 있을 때만 하고, 그러면 `true`다.
    /// 이미 그 자리 가까이에 있거나, 다른 꽃핀의 머리 위에 있거나, 할 일을 고른 뒤면 하지 않는다.
    /// 디딘 곳의 끝까지 가도 닿지 않으면 끝에서 멈춘다.
    pub fn come_to(&mut self, x: f64) -> bool {
        let arrive = ARRIVE * self.scale;
        if self.mode != Mode::Ground || self.leaving || self.on_head() || self.pending.is_some() {
            return false;
        }
        if (x - self.x).abs() <= arrive {
            return false;
        }
        self.goal = Some(x);
        self.walk = Some(if x < self.x { Dir::West } else { Dir::East });
        self.turn_to = None;
        // 닿을 때까지 걷는다. 걷는 시간으로는 멈추지 않는다.
        self.wait = f64::INFINITY;
        true
    }

    /// 다른 꽃핀의 머리 위에 서 있는가.
    pub fn on_head(&self) -> bool {
        self.on.is_some() && self.head
    }

    /// 머리에 다른 꽃핀이 서 있는지 알려 준다. 서 있는 동안에는 쉬고 걷기만 하고, 자리를 비워도 잠들지 않는다.
    pub fn set_carrying(&mut self, carrying: bool) {
        self.carrying = carrying;
    }

    /// 공중에 있어서 어디에 내려앉을지 알아야 하는가.
    pub fn is_airborne(&self) -> bool {
        matches!(self.mode, Mode::Release | Mode::Fall | Mode::Emerge | Mode::Spawn)
            || (self.mode == Mode::Jump && self.launched)
    }

    /// 말풍선을 띄워도 되는가. 무엇인가를 딛고 서 있을 때다. 떠 있거나 잡혀 있을 때는 내려앉은 뒤에 띄운다.
    pub fn can_speak(&self) -> bool {
        matches!(self.mode, Mode::Ground | Mode::Act | Mode::Sleep | Mode::Sit | Mode::Land | Mode::Settle)
    }

    /// 발밑에 있는 바닥의 y. 창 위에 있거나 창 윗변으로 내려앉는 중이면 그 윗변이다. 그림자가 여기에 진다.
    pub fn ground_below(&self, around: &Surroundings) -> f64 {
        if self.through {
            return around.floor;
        }
        let below = around.perches.iter().filter(|perch| perch.y >= self.y - 0.5 && perch.holds(self.x));
        below.map(|perch| perch.y).fold(around.floor, f64::min)
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

    pub fn habits(&self) -> Habits {
        self.habits
    }

    pub fn set_habits(&mut self, habits: Habits) {
        self.habits = habits;
    }

    /// 짧게 클릭했다. 자고 있으면 깨우고, 걸터앉아 있으면 일어나고, 서 있으면 반응 하나를 한다.
    /// 반응은 바로 전과 다른 것을 고른다.
    pub fn poke(&mut self, motions: &Motions) {
        match self.mode {
            Mode::Sleep => self.sleep_left = 0.0,
            Mode::Sit => self.stand_up(motions),
            Mode::Ground => {
                let kinds = [motions.poke_surprise, motions.poke_pout, motions.poke_shy];
                let choices: Vec<MotionId> = kinds.into_iter().filter(|&kind| Some(kind) != self.last_poke).collect();
                let pick = choices[(self.rng.unit() * choices.len() as f64) as usize % choices.len()];
                self.last_poke = Some(pick);
                self.act(pick);
            }
            _ => {}
        }
    }

    /// 머리 위에서 마우스를 `distance`px 문질렀다. 잠깐 사이에 충분히 문지르면 쓰다듬기 반응을 한다.
    /// 지나가기만 해서는 거리가 모자라 반응하지 않는다.
    pub fn rub(&mut self, motions: &Motions, distance: f64) {
        if self.mode != Mode::Ground {
            return;
        }
        if self.pat_quiet > PAT_KEEP {
            self.pat_distance = 0.0;
        }
        self.pat_quiet = 0.0;
        self.pat_distance += distance;
        if self.pat_distance >= PAT_DISTANCE * self.scale {
            self.pat_distance = 0.0;
            self.act(motions.pat);
        }
    }

    /// 가끔 하는 행동을 바로 시작한다.
    pub fn start(&mut self, whim: Whim, motions: &Motions) {
        match whim {
            Whim::Stretch => self.act(motions.stretch),
            Whim::Yawn => self.act(motions.yawn),
            Whim::Jump => {
                self.act(motions.jump);
                self.mode = Mode::Jump;
                self.vy = 0.0;
                self.launched = false;
            }
            Whim::Sleep => {
                self.act(motions.sleep_in);
                self.mode = Mode::Sleep;
                self.sleep_left = if self.away { f64::INFINITY } else { self.rng.range(SLEEP_FOR) };
            }
            Whim::Sit => {
                self.act(motions.edge_in);
                self.mode = Mode::Sit;
                let (least, most) = SIT_LOOPS;
                self.sit_left = least + (self.rng.unit() * (most - least + 1) as f64) as u32;
            }
            // 둘레를 봐야 시작할 수 있다. 다음에 바닥에서 걸음을 밟을 때 시작한다.
            Whim::Climb | Whim::Hop => {
                if self.mode == Mode::Ground {
                    self.walk = None;
                    self.turn_to = None;
                    self.hop_dir = None;
                    self.pending = Some(whim);
                }
            }
        }
    }

    /// 뛰어오를 창 윗변을 고른다. 가로로 가장 가까운 것이고, 너무 멀면 고르지 않는다.
    fn climb_target(&self, around: &Surroundings) -> Option<Perch> {
        let reach = LEAP_REACH * self.scale;
        // 지금 딛고 있는 토막은 뺀다.
        let here = |perch: &Perch| self.on.is_some() && (perch.y - self.y).abs() < 0.5 && perch.holds(self.x);
        let near = around.perches.iter().filter(|perch| !here(perch) && perch.gap(self.x) <= reach);
        near.min_by(|a, b| {
            let sideways = a.gap(self.x).total_cmp(&b.gap(self.x));
            sideways.then((a.y - self.y).abs().total_cmp(&(b.y - self.y).abs()))
        })
        .copied()
    }

    /// 가까운 창 윗변으로 뛰어오르기 시작한다. 뛰어오를 곳이 없으면 `false`다.
    ///
    /// 떠 있는 시간은 늘 같다. 그 시간에 꼭 맞게 올라갔다가 윗변에 내려앉도록 중력과 박차는 빠르기를 정한다.
    /// 그래서 아무리 높은 곳도 한 번에 뛰어오르고, 점프 그림과도 맞는다.
    fn start_climb(&mut self, motions: &Motions, around: &Surroundings) -> bool {
        let Some(target) = self.climb_target(around) else { return false };
        let land_x = self.x.clamp(target.left, target.right);
        let peak = self.y.min(target.y) - LEAP_CLEAR * self.scale;
        let (up, down) = (self.y - peak, target.y - peak);
        let gravity = 2.0 * (up.sqrt() + down.sqrt()).powi(2) / (LEAP_AIR * LEAP_AIR);
        let speed = (2.0 * gravity * up).sqrt();
        self.act(motions.jump);
        self.mode = Mode::Jump;
        self.vy = 0.0;
        self.launched = false;
        self.through = false;
        self.vx = 0.0;
        let vx = (land_x - self.x) / LEAP_AIR;
        self.leap = Some(Leap { window: target.window, from: (self.x, self.y), vx, speed, gravity, t: 0.0 });
        true
    }

    /// 뛰어오르는 길을 따라 `dt`초만큼 간다. 윗변에 내려앉았으면 `true`를 돌려준다.
    ///
    /// 다 왔는데 그 창의 윗변이 그 자리에 없으면(그사이에 닫혔거나 옮겨졌으면) 그대로 떨어진다.
    fn leap_step(&mut self, dt: f64, motions: &Motions, around: &Surroundings) -> bool {
        let Some(mut leap) = self.leap else { return false };
        leap.t += dt;
        let t = leap.t.min(LEAP_AIR);
        self.x = leap.from.0 + leap.vx * t;
        self.y = leap.from.1 - leap.speed * t + 0.5 * leap.gravity * t * t;
        self.vy = leap.gravity * t - leap.speed;
        if leap.t < LEAP_AIR {
            self.leap = Some(leap);
            return false;
        }
        self.leap = None;
        let snap = PERCH_SNAP * self.scale;
        let waiting =
            |perch: &&Perch| perch.window == leap.window && perch.holds(self.x) && (perch.y - self.y).abs() <= snap;
        if let Some(perch) = around.perches.iter().find(waiting).copied() {
            self.touch_down(motions, perch.y, Some(perch));
            return true;
        }
        false
    }

    /// 올라서 있던 창에서 뛰어내리기 시작한다. `dir`이 있으면 그쪽 바깥으로, 없으면 제자리에서 뛴다.
    /// 다른 창 윗변을 지나쳐 바닥까지 내려간다.
    ///
    /// 다른 꽃핀의 머리 위에서는 늘 옆으로 뛰고, 아래에 창이 있으면 그 위에 내려선다.
    fn start_hop(&mut self, motions: &Motions, dir: Option<Dir>) {
        let from_head = self.on_head();
        let dir = if from_head { Some(dir.unwrap_or_else(|| self.any_dir())) } else { dir };
        self.act(motions.jump);
        self.mode = Mode::Jump;
        self.vy = 0.0;
        self.launched = false;
        self.leap = None;
        self.through = !from_head;
        self.vx = dir.map_or(0.0, |dir| dir.sign() * HOP_SPEED * self.scale);
    }

    /// 아무 쪽이나 고른다.
    fn any_dir(&mut self) -> Dir {
        if self.rng.chance(0.5) { Dir::West } else { Dir::East }
    }

    /// 쉬다가 고른 행동을 시작한다. 뛰어오르거나 뛰어내릴 수 없게 됐으면 그만두고 다시 쉰다.
    fn begin(&mut self, whim: Whim, motions: &Motions, around: &Surroundings) {
        match whim {
            Whim::Climb if self.start_climb(motions, around) => {}
            Whim::Hop if self.on.is_some() => {
                let dir = self.hop_dir.take();
                self.start_hop(motions, dir);
            }
            Whim::Climb | Whim::Hop => self.rest(motions, REST),
            other => {
                self.start(other, motions);
                self.started = Some(other);
            }
        }
    }

    /// 쉬다가 혼자 시작한 가끔 하는 행동이 있으면 꺼낸다. 사용자나 앱이 시킨 행동은 들어 있지 않다.
    pub fn take_started(&mut self) -> Option<Whim> {
        self.started.take()
    }

    /// 지금 이 행동을 골라서 볼 수 있는가.
    ///
    /// 바닥이나 창 위에 서 있을 때 된다. 쉬거나 걷는 중이 아니라 다른 행동을 하고 있어도(기지개, 잠,
    /// 걸터앉기 등) 된다. 그때는 하던 것을 그만두고 고른 행동을 한다. 떠 있거나 잡혀 있을 때는 안 된다.
    /// 걸터앉기는 단 위에서만, 창 위로 올라가기는 가까이에 올라설 윗변이 있을 때만, 뛰어내리기는 창 위에
    /// 있을 때만 된다.
    pub fn can_show(&self, show: Show, around: &Surroundings) -> bool {
        let standing = matches!(self.mode, Mode::Ground | Mode::Act | Mode::Sleep | Mode::Sit);
        if !standing || self.leaving {
            return false;
        }
        match show {
            // 다른 꽃핀의 머리 위에서는 앉거나 자거나 뛰지 않는다. 내려가는 것만 된다.
            Show::Sit | Show::Sleep | Show::Jump | Show::Climb if self.on_head() => false,
            Show::Sit => self.on.is_some() || around.ledge,
            Show::Climb => self.climb_target(around).is_some(),
            Show::Hop => self.on.is_some(),
            _ => true,
        }
    }

    /// 사용자가 고른 행동을 바로 시작한다. 지금 할 수 없는 행동이면 아무 일도 하지 않고 `false`를 돌려준다.
    ///
    /// 혼자 시작할 때와 달리 쉬는 그림의 첫 장을 기다리지 않는다. 고르자마자 보여야 하기 때문이다.
    pub fn show(&mut self, show: Show, motions: &Motions, around: &Surroundings) -> bool {
        if !self.can_show(show, around) {
            return false;
        }
        match show {
            Show::Stretch => self.start(Whim::Stretch, motions),
            Show::Yawn => self.start(Whim::Yawn, motions),
            Show::Jump => self.start(Whim::Jump, motions),
            Show::Sleep => self.start(Whim::Sleep, motions),
            Show::Sit => self.start(Whim::Sit, motions),
            Show::Climb => return self.start_climb(motions, around),
            Show::Hop => self.start_hop(motions, None),
            Show::Surprise | Show::Pout | Show::Shy => {
                let kind = match show {
                    Show::Surprise => motions.poke_surprise,
                    Show::Pout => motions.poke_pout,
                    _ => motions.poke_shy,
                };
                // 다음에 클릭했을 때 같은 반응이 연달아 나오지 않게 적어 둔다.
                self.last_poke = Some(kind);
                self.act(kind);
            }
            Show::Pat => self.act(motions.pat),
        }
        true
    }

    /// 걸터앉아 있다가 일어난다. 다리를 흔드는 중이면 바로 일어나고, 앉는 중이면 앉자마자 일어난다.
    fn stand_up(&mut self, motions: &Motions) {
        self.sit_left = 0;
        if self.motion == motions.edge_loop {
            self.enter(motions.edge_out, 0.0, motions);
        }
    }

    /// 사용자가 자리를 비웠는지 알려 준다. 비웠으면 쉬게 될 때 잠들고, 돌아오면 깬다.
    pub fn set_away(&mut self, away: bool) {
        let sleeping = self.mode == Mode::Sleep && self.sleep_left > 0.0;
        if away && sleeping {
            // 이미 자고 있었으면 돌아올 때까지 계속 잔다.
            self.sleep_left = f64::INFINITY;
        } else if !away && self.away {
            if sleeping && self.sleep_left.is_infinite() {
                self.sleep_left = 0.0;
            }
            if self.pending == Some(Whim::Sleep) {
                self.pending = None;
            }
        }
        self.away = away;
    }

    /// 제자리 동작을 시작한다.
    fn act(&mut self, motion: MotionId) {
        self.mode = Mode::Act;
        self.motion = motion;
        self.t = 0.0;
        self.walk = None;
        self.turn_to = None;
        self.pending = None;
        self.showcase = false;
        self.loops_left = 0;
    }

    /// 지금 잡을 수 있는가. 나타나는 중, 착지하는 중, 사라지는 중에는 잡히지 않는다.
    pub fn can_grab(&self) -> bool {
        matches!(
            self.mode,
            Mode::Ground | Mode::Act | Mode::Jump | Mode::Sleep | Mode::Sit | Mode::Release | Mode::Fall | Mode::Emerge
        )
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
        self.vx = 0.0;
        self.on = None;
        self.carrying = false;
        self.through = false;
        self.leap = None;
        self.riding = 0.0;
        self.walk = None;
        self.turn_to = None;
        self.pending = None;
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

    /// 하던 일을 멈추고 바닥의 `(x, y)`에 바로 선다. 떨어지는 모습 없이 쉬기(idle2)부터 한다. 방송 촬영용 창의
    /// 크기가 바뀌어 꽃핀들을 새 바닥에 다시 세울 때 쓴다. 손을 흔들며 사라지는 중이면 그 자리에서 이어 간다.
    /// 돌아가라는 말을 들었으면 선 뒤에 손을 흔들고 사라진다.
    pub fn settle_at(&mut self, motions: &Motions, (x, y): (f64, f64)) {
        if !matches!(self.mode, Mode::Vanish | Mode::Gone) {
            (self.mode, self.motion, self.t) = (Mode::Ground, motions.idle2, 0.0);
            (self.vx, self.vy, self.on, self.head, self.through) = (0.0, 0.0, None, false, false);
            (self.leap, self.hop_dir, self.riding, self.shake) = (None, None, 0.0, Shake::default());
            (self.walk, self.goal, self.turn_to) = (None, None, None);
            self.wait = self.rng.range(SETTLE_REST);
            (self.launched, self.sleep_left, self.sit_left) = (false, 0.0, 0);
            (self.pending, self.showcase, self.loops_left, self.started) = (None, false, 0, None);
            (self.pat_distance, self.pat_quiet) = (0.0, 0.0);
        }
        (self.x, self.y, self.home, self.drag) = (x, y, x, (x, y));
    }

    /// 무대가 통째로 옮겨 갔다(방송 촬영용 창을 옮김). 같은 만큼 함께 옮겨 가고, 하던 일은 그대로 이어 간다.
    pub fn shift(&mut self, (dx, dy): (f64, f64)) {
        (self.x, self.y, self.home) = (self.x + dx, self.y + dy, self.home + dx);
        self.drag = (self.drag.0 + dx, self.drag.1 + dy);
        // 딛고 있는 머리도 같이 옮겨 갔다. 머리가 움직인 것으로 보고 따라가지 않게 한다.
        if let Some((_, left)) = &mut self.on {
            *left += dx;
        }
        if let Some(goal) = &mut self.goal {
            *goal += dx;
        }
        if let Some(leap) = &mut self.leap {
            leap.from = (leap.from.0 + dx, leap.from.1 + dy);
        }
    }

    /// 사라지라고 한다. 바닥에 서 있지 않으면 내려와 선 뒤에 사라진다. 자고 있으면 깨고, 앉아 있으면 일어난다.
    pub fn leave(&mut self, motions: &Motions) {
        self.leaving = true;
        self.pending = None;
        match self.mode {
            Mode::Sleep => self.sleep_left = 0.0,
            Mode::Sit => self.stand_up(motions),
            _ => {}
        }
    }

    /// 사라지라는 말을 들었는가. 아직 손을 흔들기 전일 수도 있다.
    pub fn is_leaving(&self) -> bool {
        self.leaving
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
        self.pat_quiet += dt;
        self.riding = (self.riding - dt).max(0.0);
        if self.mode == Mode::Gone {
            return;
        }
        // 창 위에 서 있으면 그 윗변이 바닥이고 양 끝이 벽이다.
        let here = self.footing(dt, motions, around);
        match self.mode {
            Mode::Ground => self.ground_step(dt, motions, &here),
            _ => self.air_step(dt, motions, &here),
        }
        if here.left <= here.right {
            self.x = self.x.clamp(here.left, here.right);
        }
    }

    /// 무엇인가를 딛고 서 있는가. 떠 있거나 잡혀 있으면 아니다.
    fn is_standing(&self) -> bool {
        match self.mode {
            Mode::Ground | Mode::Act | Mode::Sleep | Mode::Sit | Mode::Land | Mode::Settle | Mode::Vanish => true,
            // 박차기 전에는 아직 딛고 있다.
            Mode::Jump => !self.launched,
            _ => false,
        }
    }

    /// 딛고 있는 곳의 상황을 돌려준다. 창 위에 서 있으면 그 윗변이 바닥이고 양 끝이 벽이다.
    ///
    /// 창이 움직였으면 따라 움직인다. 딛고 있던 윗변이 없어졌으면(창을 닫았거나, 최소화했거나, 선 자리가
    /// 다른 창에 가려졌으면) 그 자리에서 바닥까지 떨어지기 시작한다. 창을 흔들면 놓친 것처럼 떨어진다.
    fn footing<'a>(&mut self, dt: f64, motions: &Motions, around: &Surroundings<'a>) -> Surroundings<'a> {
        let Some((window, origin)) = self.on else {
            self.shake = Shake::default();
            return *around;
        };
        if !self.is_standing() {
            return *around;
        }
        // 그 창의 윗변 토막 가운데, 창이 옆으로 움직인 만큼 옮긴 자리를 받치는 것을 찾는다.
        let under = |perch: &&Perch| perch.window == window && perch.holds(self.x + perch.origin - origin);
        let Some(perch) = around.perches.iter().find(under) else {
            self.on = None;
            // 창이 없어졌으면 다른 창들을 지나쳐 바닥까지 간다. 받치던 꽃핀이 움직였으면 바로 아래에 내려선다.
            self.through = !self.head;
            self.mode = Mode::Fall;
            self.motion = motions.fall;
            self.t = 0.0;
            self.vy = 0.0;
            self.vx = 0.0;
            self.leap = None;
            self.walk = None;
            self.turn_to = None;
            self.pending = None;
            self.showcase = false;
            return *around;
        };
        let (dx, dy) = (perch.origin - origin, perch.y - self.y);
        // 창은 끄는 대로 움직이므로 자리를 자주 고친다. 다른 꽃핀은 장마다 움직이므로 장에 맞춰 따라가면 된다.
        if (dx.abs() > 0.01 || dy.abs() > 0.01) && !perch.head {
            self.riding = RIDE_SMOOTH;
        }
        self.x += dx;
        self.y = perch.y;
        self.on = Some((window, perch.origin));
        if self.shake.feed(dt, (dx, dy), SHAKE_LEG * self.scale) && self.mode != Mode::Vanish {
            self.shaken_off(motions);
            return *around;
        }
        Surroundings { floor: perch.y, left: perch.left, right: perch.right, ledge: true, ..*around }
    }

    /// 흔들리는 창에서 놓친다. 그 자리에 잠깐 떠 있다가(hang_to_fall) 창들을 지나쳐 바닥까지 떨어진다.
    fn shaken_off(&mut self, motions: &Motions) {
        self.on = None;
        self.through = true;
        self.mode = Mode::Release;
        self.motion = motions.hang_to_fall;
        self.t = 0.0;
        self.vy = 0.0;
        self.vx = 0.0;
        self.leap = None;
        self.riding = 0.0;
        self.shake = Shake::default();
        self.walk = None;
        self.turn_to = None;
        self.pending = None;
        self.showcase = false;
        self.sit_left = 0;
    }

    /// 장이 바뀌지 않아도 자리를 자주 고쳐야 하는가. 떨어지는 중이거나 끌려가는 중이면 그렇다.
    /// 올라선 창이 움직이는 동안에도 그렇다.
    pub fn moves_smoothly(&self) -> bool {
        if self.riding > 0.0 && self.mode != Mode::Gone {
            return true;
        }
        match self.mode {
            Mode::Fall => true,
            Mode::Drag => (self.drag.0 - self.x).abs() > 0.5 || (self.drag.1 - self.y).abs() > 0.5,
            // 잠깐 떠 있는 동안은 움직이지 않는다.
            Mode::Release => self.t >= RELEASE_HOLD_MS,
            Mode::Emerge => self.t >= SPAWN_DROP_MS,
            // 박차고 오른 뒤부터 움직인다.
            Mode::Jump => self.launched,
            _ => false,
        }
    }

    /// 점프에서 박차는 시각(ms).
    fn launch_ms(&self, motions: &Motions) -> f64 {
        motions.clip(self.motion).launch_ms.unwrap_or(JUMP_LAUNCH_MS)
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
            Mode::Jump => frame.min(until(self.launch_ms(motions))),
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

    /// 지금 동작의 시각을 `dt`초만큼 흘린다.
    ///
    /// 장이 바뀌는 시각에 아주 가까워지면 꼭 그 시각으로 맞춘다([`Clip::settle`]). 박자마다 시간을 흘릴 때
    /// 끝자리 오차 때문에 장이 한 박자 늦게 넘어가는 일을 막는다.
    ///
    /// [`Clip::settle`]: crate::motions::Clip::settle
    fn pass(&mut self, dt: f64, motions: &Motions) {
        self.t = motions.clip(self.motion).settle(self.t + dt * 1000.0);
    }

    /// 반복 동작의 시간을 한 바퀴 안으로 되돌린다. 한 바퀴를 돌았으면 `true`다.
    fn wrap(&mut self, motions: &Motions) -> bool {
        let clip = motions.clip(self.motion);
        if clip.looping && self.t >= clip.total_ms {
            self.t = clip.settle(self.t % clip.total_ms);
            return true;
        }
        false
    }

    /// 방금 넘어온 동작의 시각을 맞춘다. `carried`는 앞 동작에서 넘친 시간(ms)이다.
    /// 넘친 만큼 지난 데서 시작해야 동작이 이어져도 전체 빠르기가 그대로다.
    fn carry(&mut self, carried: f64, motions: &Motions) {
        let clip = motions.clip(self.motion);
        self.t = clip.settle(carried.min(clip.total_ms / 2.0));
    }

    /// `motion`으로 넘어가고, 상태도 그 동작에 맞춘다. `carried`는 앞 동작에서 넘친 시간(ms)이다.
    fn enter(&mut self, motion: MotionId, carried: f64, motions: &Motions) {
        let landed = self.motion == motions.land_to_idle2;
        self.mode = mode_of(motion, motions);
        self.motion = motion;
        self.carry(carried, motions);
        if self.mode == Mode::Ground {
            self.rest(motions, if landed { SETTLE_REST } else { AFTER_ACT });
        }
    }

    /// 바닥에서 idle2로 쉬기 시작한다.
    fn rest(&mut self, motions: &Motions, range: (f64, f64)) {
        self.mode = Mode::Ground;
        self.motion = motions.idle2;
        self.walk = None;
        self.turn_to = None;
        self.pending = None;
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

    /// 중력 `gravity`로 한 걸음 떨어진다. 바닥이나 창 윗변에 닿았으면 착지하고 `true`를 돌려준다.
    fn fall_step(&mut self, dt: f64, gravity: f64, motions: &Motions, around: &Surroundings) -> bool {
        let before = self.y;
        self.vy += gravity * self.scale * dt;
        self.y += self.vy * dt;
        self.x += self.vx * dt;
        if self.vy <= 0.0 {
            return false;
        }
        if !self.through {
            // 이번 걸음에 위에서 아래로 지나친 윗변 가운데 가장 높은 것에 내려앉는다.
            let crossed = |perch: &&Perch| before <= perch.y && self.y >= perch.y && perch.holds(self.x);
            if let Some(perch) = around.perches.iter().filter(crossed).min_by(|a, b| a.y.total_cmp(&b.y)).copied() {
                self.touch_down(motions, perch.y, Some(perch));
                return true;
            }
        }
        if self.y >= around.floor {
            self.touch_down(motions, around.floor, None);
            return true;
        }
        false
    }

    /// 높이 `y`에 내려앉는다. `perch`가 있으면 그 창 윗변에 올라선 것이다.
    fn touch_down(&mut self, motions: &Motions, y: f64, perch: Option<Perch>) {
        self.y = y;
        self.vy = 0.0;
        self.vx = 0.0;
        self.leap = None;
        self.through = false;
        self.on = perch.map(|perch| (perch.window, perch.origin));
        self.head = perch.is_some_and(|perch| perch.head);
        if let Some(perch) = perch.filter(|perch| perch.head) {
            // 다른 꽃핀의 머리 위에는 가운데에 선다.
            self.x = perch.origin;
        }
        self.home = self.x;
        self.mode = Mode::Land;
        self.motion = motions.land;
        self.t = 0.0;
    }

    /// 놓은 자리의 발밑 가까이에 있는 창 윗변. 그 위에 올려놓은 것으로 친다.
    fn perch_at_feet(&self, around: &Surroundings) -> Option<Perch> {
        let snap = PERCH_SNAP * self.scale;
        let near = around.perches.iter().filter(|perch| perch.holds(self.x) && (perch.y - self.y).abs() <= snap);
        near.min_by(|a, b| (a.y - self.y).abs().total_cmp(&(b.y - self.y).abs())).copied()
    }

    /// 바닥에 있지 않을 때의 한 걸음.
    fn air_step(&mut self, dt: f64, motions: &Motions, around: &Surroundings) {
        self.pass(dt, motions);
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
                if self.t >= hold && self.fall_step(dt, GRAVITY, motions, around) {
                    return;
                }
                if self.y >= around.floor {
                    self.touch_down(motions, around.floor, None);
                } else if let (Mode::Release, false, Some(perch)) =
                    (self.mode, self.through, self.perch_at_feet(around))
                {
                    // 창 윗변 가까이에 놓았다. 떨어뜨리지 않고 그 위에 올려놓는다. 흔들려 놓친 것이면 그대로 떨어진다.
                    self.touch_down(motions, perch.y, Some(perch));
                } else {
                    self.follow_when_over(motions);
                }
            }
            Mode::Fall => {
                // 창으로 뛰어오르다 점프 그림이 먼저 끝났으면, 가던 길을 마저 간다.
                let landed = if self.leap.is_some() {
                    self.leap_step(dt, motions, around)
                } else {
                    self.fall_step(dt, GRAVITY, motions, around)
                };
                if !landed {
                    self.wrap(motions);
                }
            }
            Mode::Jump => {
                if self.t >= self.launch_ms(motions) {
                    if !self.launched {
                        self.launched = true;
                        self.on = None;
                        match &mut self.leap {
                            Some(leap) => leap.from = (self.x, self.y),
                            None => self.vy = -JUMP_SPEED * self.scale,
                        }
                    }
                    let landed = if self.leap.is_some() {
                        self.leap_step(dt, motions, around)
                    } else {
                        self.fall_step(dt, JUMP_GRAVITY, motions, around)
                    };
                    if landed {
                        return;
                    }
                }
                // 점프 그림이 끝났는데 아직 떠 있으면 떨어지는 그림으로 잇는다.
                self.follow_when_over(motions);
            }
            Mode::Sleep => {
                let clip = motions.clip(self.motion);
                if self.motion == motions.sleep_loop {
                    self.wrap(motions);
                    self.sleep_left -= dt;
                    if self.sleep_left <= 0.0 {
                        self.enter(motions.sleep_out, 0.0, motions);
                    }
                } else if self.motion == motions.sleep_in && clip.is_over(self.t) && self.sleep_left <= 0.0 {
                    // 잠들기 전에 깨웠으면 자지 않고 바로 일어난다.
                    self.enter(motions.sleep_out, 0.0, motions);
                } else {
                    self.follow_when_over(motions);
                }
            }
            Mode::Sit => {
                if self.motion != motions.edge_loop {
                    // 앉는 그림과 일어나는 그림은 끝나면 다음으로 넘어간다. 앉는 사이에 일어나라고 했으면 바로 일어난다.
                    let sat_down = self.motion == motions.edge_in && motions.clip(self.motion).is_over(self.t);
                    if sat_down && self.sit_left == 0 {
                        self.enter(motions.edge_out, 0.0, motions);
                    } else {
                        self.follow_when_over(motions);
                    }
                } else if self.wrap(motions) {
                    // 다리를 한 번 흔들 때마다 센다. 다 흔들었으면 일어난다.
                    self.sit_left = self.sit_left.saturating_sub(1);
                    if self.sit_left == 0 {
                        self.enter(motions.edge_out, self.t, motions);
                    }
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
                self.carry(carried, motions);
                self.loops_left = SHOWCASE_LOOPS;
            }
            Some(next) => self.enter(next, carried, motions),
            None => {
                // 쉬는 그림도 첫 장부터 보여 준다. 시각을 그대로 두면 쉬는 그림이 중간 장에서 시작한다.
                self.rest(motions, REST);
                self.carry(carried, motions);
            }
        }
    }

    /// 쉬기를 마쳤을 때 가끔 하는 행동 가운데 하나를 고른다. 고르지 않으면 `None`이다.
    ///
    /// `ledge`는 단 위에 서 있는지다. 단 위에서만 걸터앉기를 고른다. `can_climb`은 뛰어오를 창 윗변이
    /// 가까이에 있는지다. 창 위에 서 있을 때만 뛰어내리기를 고른다.
    fn pick_whim(&mut self, ledge: bool, can_climb: bool) -> Option<Whim> {
        let chance_if = |yes: bool, chance: f64| if yes { chance } else { 0.0 };
        let first = [
            (Whim::Sit, chance_if(ledge, SIT_CHANCE)),
            (Whim::Climb, chance_if(can_climb, CLIMB_CHANCE)),
            (Whim::Hop, chance_if(self.on.is_some(), HOP_CHANCE)),
        ];
        let mut r = self.rng.unit();
        for &(whim, chance) in first.iter().chain(&WHIMS) {
            r -= chance * self.habits.whim_rate;
            if r < 0.0 {
                return Some(whim);
            }
        }
        None
    }

    /// 다른 꽃핀의 머리 위에서 쉬기를 마쳤을 때 할 일을 고른다. 뛰어내리거나, 기지개나 하품을 한다.
    fn pick_on_head(&mut self) -> Option<Whim> {
        let mut r = self.rng.unit();
        let choices = [(Whim::Hop, HEAD_HOP_CHANCE), WHIMS[0], WHIMS[1]];
        for (whim, chance) in choices {
            r -= chance * self.habits.whim_rate;
            if r < 0.0 {
                return Some(whim);
            }
        }
        None
    }

    /// idle2로 서서 시간을 흘리고, 지금 idle2의 첫 장을 보여 주고 있는지 돌려준다.
    /// 다른 동작을 하고 있었으면 idle2로 바꾼다(첫 장부터 시작한다).
    fn idle2_at_first_frame(&mut self, dt: f64, motions: &Motions) -> bool {
        if self.motion != motions.idle2 {
            self.motion = motions.idle2;
            self.t = 0.0;
        } else {
            self.pass(dt, motions);
            self.wrap(motions);
        }
        motions.clip(motions.idle2).frame_at(self.t) == 0
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
            // 사라지는 그림의 첫 장은 idle2의 첫 장에서 이어지게 그려져 있다. idle2가 첫 장일 때 바꾼다.
            if self.idle2_at_first_frame(dt, motions) {
                self.mode = Mode::Vanish;
                self.motion = motions.despawn;
                self.t = 0.0;
            }
            return;
        }

        if let Some(whim) = self.pending {
            // 가끔 하는 행동도 idle2의 첫 장에서 이어지게 그려져 있다. 혼자 시작하는 것이라 서두를 까닭이 없으므로
            // idle2가 첫 장으로 돌아올 때까지 기다렸다 시작한다.
            if self.idle2_at_first_frame(dt, motions) {
                self.begin(whim, motions, around);
            }
            return;
        }

        self.wait -= dt;
        if let Some(dir) = self.walk {
            if self.goal.is_some_and(|goal| (goal - self.x) * dir.sign() <= ARRIVE * self.scale) {
                // 가려던 자리에 왔다. 멈춰 쉰다.
                self.walk = None;
                self.goal = None;
                self.wait = self.rng.range(REST);
            } else if self.at_edge(dir, around) {
                self.walk = None;
                self.goal = None;
                if self.on.is_some() && self.habits.whim_rate > 0.0 && self.rng.chance(HOP_AT_END) {
                    // 창 윗변의 끝이다. 돌아서지 않고 바깥쪽으로 뛰어내리기도 한다.
                    self.hop_dir = Some(dir);
                    self.pending = Some(Whim::Hop);
                } else {
                    // 벽에 닿으면 잠깐 섰다가 돌아선다.
                    self.turn_to = Some(dir.opposite());
                    self.wait = self.rng.range(TURN_PAUSE);
                }
            } else if self.wait <= 0.0 {
                self.walk = None;
                self.goal = None;
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
                if self.habits.roam && !self.at_edge(dir, around) {
                    self.walk = Some(dir);
                    self.wait = self.rng.range(WALK);
                } else {
                    self.wait = self.rng.range(REST);
                }
            }
        } else if self.on_head() {
            // 다른 꽃핀의 머리 위에서는 걷지 않는다. 자리를 비웠으면 내려가서 잔다.
            if self.away {
                self.hop_dir = None;
                self.pending = Some(Whim::Hop);
            } else if self.wait <= 0.0 {
                match self.pick_on_head() {
                    Some(whim) => {
                        self.hop_dir = None;
                        self.pending = Some(whim);
                    }
                    None => self.wait = self.rng.range(REST),
                }
            }
        } else if self.away && !self.carrying {
            // 사용자가 자리를 비웠으면 쉬는 김에 잠든다. 머리에 누가 서 있으면 내려간 뒤에 잠든다.
            self.pending = Some(Whim::Sleep);
        } else if self.wait <= 0.0 {
            let can_climb = self.climb_target(around).is_some();
            // 머리에 누가 서 있으면 가끔 하는 행동을 하지 않고 쉬고 걷기만 한다.
            let whim = if self.carrying { None } else { self.pick_whim(around.ledge, can_climb) };
            if let Some(whim) = whim {
                self.hop_dir = None;
                self.pending = Some(whim);
            } else if self.habits.roam {
                // 어느 쪽으로든 걷는다. 그래서 벽에서 벽까지 어디든 간다.
                let mut dir = if self.rng.chance(0.5) { Dir::West } else { Dir::East };
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
            self.pass(dt, motions);
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
    use crate::motions::tests::{sample, sample_24fps};

    /// 화면의 아래 끝이 바닥인 곳. 걸터앉을 단이 없다.
    const AROUND: Surroundings =
        Surroundings { floor: 1000.0, left: 100.0, right: 1900.0, top: 150.0, ledge: false, perches: &[] };
    /// 작업표시줄 위. 바닥이 단의 윗변이다.
    const ON_LEDGE: Surroundings = Surroundings { ledge: true, ..AROUND };
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
    fn on_the_beat_every_frame_shows_exactly_once() {
        // 진짜 팩처럼 1초에 24장인 표로, 박자마다 시간을 흘린다. 무엇을 하든 장은 박자마다 꼭 하나씩 넘어가야
        // 한다. 같은 장이 두 박자 동안 보이거나 장을 건너뛰면 안 된다.
        let motions = sample_24fps();
        // 흐르는 시간은 앱에서처럼 나노초로 적은 박자 시각의 차이다. 그래서 박자와 끝자리가 조금 다르다.
        let beat_ns = |beat: u64| (beat as f64 * 1e9 / 24.0).round() as u64;
        for (seed, around) in [(1, AROUND), (2, ON_LEDGE), (3, AROUND), (4, ON_LEDGE)] {
            let mut actor = Actor::appear(&motions, 500.0, 400.0, 1.0, seed);
            let mut before = (actor.motion(), actor.frame(&motions));
            // 10분.
            for beat in 1..=24 * 600 {
                if beat == 24 * 300 {
                    actor.set_away(true);
                }
                if beat == 24 * 420 {
                    actor.set_away(false);
                }
                let dt = (beat_ns(beat) - beat_ns(beat - 1)) as f64 / 1e9;
                actor.update(dt, &motions, &around);
                let now = (actor.motion(), actor.frame(&motions));
                let clip = motions.clip(now.0);
                let first = clip.first_frame;
                let last = first + clip.frame_at(clip.total_ms - 1.0);
                let ok = if now.0 != before.0 {
                    // 다른 동작으로 넘어가면 첫 장부터다.
                    now.1 == first
                } else {
                    // 같은 동작에서는 다음 장이다. 반복 동작의 마지막 장 다음은 첫 장이다.
                    now.1 == before.1 + 1 || (clip.looping && before.1 == last && now.1 == first)
                };
                assert!(ok, "씨앗 {seed}, {beat}번째 박자: {} {} -> {} {}", before.0, before.1, clip.name, now.1);
                before = now;
            }
        }
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
    fn resting_and_walking_alternate_from_wall_to_wall() {
        let motions = sample();
        let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 7);
        // 가끔 하는 행동은 끄고 쉬기와 걷기만 본다.
        actor.set_habits(Habits { whim_rate: 0.0, ..Habits::default() });
        let (mut low, mut high) = (f64::MAX, f64::MIN);
        let mut seen = std::collections::BTreeSet::new();
        // 한 시간.
        for _ in 0..(3600.0 / STEP) as usize {
            actor.update(STEP, &motions, &AROUND);
            assert_eq!(actor.mode(), Mode::Ground);
            low = low.min(actor.position().0);
            high = high.max(actor.position().0);
            seen.insert(motions.clip(actor.motion()).name.clone());
        }
        // 쉬기, 양쪽 걷기, 돌아설 때의 idle이 모두 나온다.
        assert_eq!(seen.into_iter().collect::<Vec<_>>(), ["idle", "idle2", "walk_e", "walk_w"]);
        // 벽 밖으로는 나가지 않는다. 처음 선 자리 둘레에 머물지 않고 바닥의 대부분을 돌아다닌다.
        assert!(low >= AROUND.left && high <= AROUND.right, "{low}..{high}");
        assert!(high - low > (AROUND.right - AROUND.left) * 0.6, "멀리 가지 않음: {low}..{high}");
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
        // 만드는 동안 가만히 세워 두고 볼 때 쓴다.
        let motions = sample();
        let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 7);
        actor.set_habits(Habits { roam: false, whim_rate: 0.0 });
        let seen = run(&mut actor, &motions, &AROUND, 60.0);
        assert_eq!(names(&seen), ["idle2"]);
        assert_eq!(actor.position().0, 1000.0);
    }

    #[test]
    fn staying_put_still_does_its_whims() {
        // 돌아다니기를 꺼도 가끔 하는 행동은 한다. 걷지는 않는다.
        let motions = sample();
        let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 7);
        actor.set_habits(Habits { roam: false, whim_rate: 1.0 });
        let seen = run(&mut actor, &motions, &AROUND, 1200.0);
        let names: std::collections::BTreeSet<&str> = names(&seen).into_iter().collect();
        assert!(!names.contains("walk_w") && !names.contains("walk_e") && !names.contains("idle"), "{names:?}");
        for whim in ["stretch", "yawn", "jump", "sleep_loop"] {
            assert!(names.contains(whim), "{whim}이 나오지 않음: {names:?}");
        }
        assert_eq!(actor.position().0, 1000.0);
    }

    /// 가끔 하는 행동이 끝없이 이어지지 않게, 바닥에서 쉬는 상태로 돌아올 때까지 시간을 흘린다.
    fn settle(actor: &mut Actor, motions: &Motions) {
        for _ in 0..(30.0 / STEP) as usize {
            if actor.mode() == Mode::Ground {
                return;
            }
            actor.update(STEP, motions, &AROUND);
        }
        panic!("30초가 지나도 바닥으로 돌아오지 않음: {:?}", actor.mode());
    }

    #[test]
    fn click_reacts_and_never_repeats_the_last_reaction() {
        let motions = sample();
        let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 7);
        actor.set_habits(Habits { roam: false, whim_rate: 0.0 });
        let mut seen = Vec::new();
        for _ in 0..60 {
            actor.poke(&motions);
            assert_eq!(actor.mode(), Mode::Act);
            seen.push(motions.clip(actor.motion()).name.clone());
            settle(&mut actor, &motions);
        }
        assert!(seen.windows(2).all(|pair| pair[0] != pair[1]), "같은 반응이 연달아 나옴");
        for kind in ["poke_surprise", "poke_pout", "poke_shy"] {
            assert!(seen.iter().filter(|name| *name == kind).count() >= 10, "{kind}이 너무 드물다");
        }
    }

    #[test]
    fn click_does_nothing_in_the_air() {
        let motions = sample();
        let mut actor = Actor::stand(&motions, 1000.0, 300.0, 1.0, 7);
        actor.update(STEP, &motions, &AROUND);
        actor.poke(&motions);
        assert_eq!(actor.mode(), Mode::Fall);
    }

    #[test]
    fn rubbing_the_head_gets_a_pat_but_passing_by_does_not() {
        let motions = sample();
        let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 7);
        actor.set_habits(Habits { roam: false, whim_rate: 0.0 });
        // 머리 너비(약 80px)를 한 번 지나가는 것으로는 모자란다.
        actor.rub(&motions, 80.0);
        assert_eq!(actor.mode(), Mode::Ground);
        // 한참 뒤에 또 지나가도 앞의 것과 이어지지 않는다.
        for _ in 0..3 {
            run(&mut actor, &motions, &AROUND, 1.0);
            actor.rub(&motions, 80.0);
            assert_eq!(actor.mode(), Mode::Ground);
        }
        // 잠깐 사이에 왔다 갔다 문지르면 쓰다듬은 것이다.
        run(&mut actor, &motions, &AROUND, 1.0);
        for _ in 0..3 {
            actor.rub(&motions, 70.0);
            run(&mut actor, &motions, &AROUND, 0.2);
        }
        assert_eq!((actor.mode(), motions.clip(actor.motion()).name.as_str()), (Mode::Act, "pat"));
        settle(&mut actor, &motions);
        assert_eq!(motions.clip(actor.motion()).name, "idle2");
    }

    #[test]
    fn rubbing_needs_more_distance_when_shown_bigger() {
        let motions = sample();
        let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 2.0, 7);
        actor.rub(&motions, 250.0);
        assert_eq!(actor.mode(), Mode::Ground);
        actor.rub(&motions, 160.0);
        assert_eq!(actor.mode(), Mode::Act);
    }

    #[test]
    fn whims_come_at_their_rates() {
        // 쉬기를 마칠 때마다 기지개 10%, 하품 10%, 점프 7%, 잠 5%, 나머지는 걷기.
        let motions = sample();
        let count = |rate: f64| {
            let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 11);
            actor.set_habits(Habits { whim_rate: rate, ..Habits::default() });
            let mut picks = std::collections::BTreeMap::new();
            for _ in 0..20000 {
                actor.wait = 0.0;
                actor.walk = None;
                actor.turn_to = None;
                actor.pending = None;
                actor.mode = Mode::Ground;
                actor.motion = motions.idle2;
                actor.ground_step(0.0, &motions, &AROUND);
                let name = match actor.pending {
                    Some(whim) => format!("{whim:?}"),
                    None => "Walk".to_string(),
                };
                *picks.entry(name).or_insert(0u32) += 1;
            }
            picks
        };
        let normal = count(1.0);
        for (name, percent) in [("Stretch", 10.0), ("Yawn", 10.0), ("Jump", 7.0), ("Sleep", 5.0), ("Walk", 68.0)] {
            let got = normal[name] as f64 / 200.0;
            assert!((got - percent).abs() < 1.0, "{name}: {got}%");
        }
        // 빈도를 두 배로 하면 두 배로 자주, 0으로 하면 하지 않는다.
        assert!((count(2.0)["Jump"] as f64 / 200.0 - 14.0).abs() < 1.2);
        assert_eq!(count(0.0).keys().collect::<Vec<_>>(), ["Walk"]);
    }

    #[test]
    fn sitting_is_only_picked_on_a_ledge() {
        let motions = sample();
        let sits = |around: &Surroundings| {
            let mut actor = Actor::stand(&motions, 1000.0, around.floor, 1.0, 11);
            (0..20000).filter(|_| actor.pick_whim(around.ledge, false) == Some(Whim::Sit)).count() as f64 / 200.0
        };
        // 단 위에서는 쉬기를 마칠 때 10%.
        assert!((sits(&ON_LEDGE) - 10.0).abs() < 1.0, "{}", sits(&ON_LEDGE));
        // 바닥이 화면의 아래 끝이면 앉지 않는다.
        assert_eq!(sits(&AROUND), 0.0);
    }

    #[test]
    fn sits_swings_the_legs_a_few_times_and_stands_up() {
        let motions = sample();
        for seed in 1..40 {
            let mut actor = Actor::stand(&motions, 1000.0, ON_LEDGE.floor, 1.0, seed);
            actor.start(Whim::Sit, &motions);
            let mut swings = 0.0;
            let mut seen = vec![motions.clip(actor.motion()).name.clone()];
            while actor.mode() == Mode::Sit {
                actor.update(STEP, &motions, &ON_LEDGE);
                if actor.motion() == motions.edge_loop {
                    swings += STEP;
                }
                let name = motions.clip(actor.motion()).name.clone();
                if seen.last() != Some(&name) {
                    seen.push(name);
                }
            }
            assert_eq!(seen, ["edge_in", "edge_loop", "edge_out", "idle2"]);
            // 다리 흔들기 한 번이 0.2초다. 3~6번 흔든다.
            let loops = (swings / 0.2).round() as u32;
            assert!((3..=6).contains(&loops), "{loops}번 (seed {seed})");
            // 앉았다 일어나는 동안 자리는 그대로다.
            assert_eq!(actor.position(), (1000.0, ON_LEDGE.floor));
        }
    }

    #[test]
    fn click_makes_a_sitter_stand_up() {
        let motions = sample();
        let mut actor = Actor::stand(&motions, 1000.0, ON_LEDGE.floor, 1.0, 7);
        actor.start(Whim::Sit, &motions);
        run(&mut actor, &motions, &ON_LEDGE, 0.3);
        assert_eq!(motions.clip(actor.motion()).name, "edge_loop");
        actor.poke(&motions);
        assert_eq!(motions.clip(actor.motion()).name, "edge_out");
        // 앉는 중에 클릭하면 앉자마자 일어난다.
        settle(&mut actor, &motions);
        actor.start(Whim::Sit, &motions);
        actor.poke(&motions);
        assert_eq!(names(&run(&mut actor, &motions, &ON_LEDGE, 1.0)), ["edge_in", "edge_out", "idle2"]);
    }

    #[test]
    fn sitter_stands_up_to_leave() {
        let motions = sample();
        let mut actor = Actor::stand(&motions, 1000.0, ON_LEDGE.floor, 1.0, 7);
        actor.start(Whim::Sit, &motions);
        run(&mut actor, &motions, &ON_LEDGE, 0.3);
        assert!(actor.can_grab());
        // 사라지라고 하면 바로 일어나서, 인사하고 사라진다.
        actor.leave(&motions);
        let seen = run(&mut actor, &motions, &ON_LEDGE, 6.0);
        assert_eq!(names(&seen), ["edge_out", "idle2", "despawn"]);
        assert!(actor.is_gone());
    }

    #[test]
    fn whim_waits_for_the_first_frame_of_idle2() {
        let motions = sample();
        let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 7);
        // idle2의 둘째 장(100~200ms)을 보여 주는 중에 기지개를 켜기로 했다.
        actor.t = 150.0;
        actor.pending = Some(Whim::Stretch);
        run(&mut actor, &motions, &AROUND, 0.2);
        assert_eq!(motions.clip(actor.motion()).name, "idle2");
        // 한 바퀴(400ms)가 끝나 첫 장으로 돌아오면 시작한다.
        run(&mut actor, &motions, &AROUND, 0.1);
        assert_eq!(motions.clip(actor.motion()).name, "stretch");
    }

    #[test]
    fn jump_goes_up_and_comes_back_down() {
        let motions = sample();
        let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 7);
        actor.start(Whim::Jump, &motions);
        // 박차는 시각(0.3초)까지는 바닥에 있다.
        run(&mut actor, &motions, &AROUND, 0.28);
        assert_eq!((actor.mode(), actor.position().1), (Mode::Jump, AROUND.floor));
        // 가장 높이 뜨는 높이는 480² ÷ (2 × 720) = 160px이다.
        let mut highest = AROUND.floor;
        let mut seen = vec![motions.clip(actor.motion()).name.clone()];
        for _ in 0..(6.0 / STEP) as usize {
            actor.update(STEP, &motions, &AROUND);
            highest = highest.min(actor.position().1);
            let name = motions.clip(actor.motion()).name.clone();
            if seen.last() != Some(&name) {
                seen.push(name);
            }
        }
        assert!((AROUND.floor - highest - 160.0).abs() < 6.0, "{}", AROUND.floor - highest);
        // 점프 그림이 끝나기 전에 내려와 착지로 이어진다.
        assert_eq!(seen[..4], ["jump", "land", "land_to_idle2", "idle2"]);
        assert_eq!(actor.position().1, AROUND.floor);
    }

    #[test]
    fn jump_that_outlasts_its_picture_keeps_falling() {
        // 점프 그림이 끝났는데 아직 떠 있으면(바닥이 낮아진 경우 등) 떨어지는 그림으로 잇는다.
        let motions = sample();
        let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 7);
        actor.start(Whim::Jump, &motions);
        run(&mut actor, &motions, &AROUND, 1.0);
        let lower = Surroundings { floor: AROUND.floor + 800.0, ..AROUND };
        let seen = run(&mut actor, &motions, &lower, 4.0);
        assert_eq!(names(&seen)[..3], ["jump", "fall", "land"]);
        assert_eq!(actor.position().1, lower.floor);
    }

    #[test]
    fn sleep_lasts_a_while_then_wakes() {
        let motions = sample();
        let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 7);
        actor.start(Whim::Sleep, &motions);
        let mut asleep = 0.0;
        let mut seen = vec![motions.clip(actor.motion()).name.clone()];
        while actor.mode() == Mode::Sleep {
            actor.update(STEP, &motions, &AROUND);
            if actor.motion() == motions.sleep_loop {
                asleep += STEP;
            }
            let name = motions.clip(actor.motion()).name.clone();
            if seen.last() != Some(&name) {
                seen.push(name);
            }
        }
        assert_eq!(seen, ["sleep_in", "sleep_loop", "sleep_out", "idle2"]);
        assert!((6.0..=12.1).contains(&asleep), "{asleep}");
    }

    #[test]
    fn click_wakes_a_sleeper() {
        let motions = sample();
        let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 7);
        actor.start(Whim::Sleep, &motions);
        run(&mut actor, &motions, &AROUND, 2.0);
        assert_eq!(motions.clip(actor.motion()).name, "sleep_loop");
        actor.poke(&motions);
        actor.update(STEP, &motions, &AROUND);
        assert_eq!(motions.clip(actor.motion()).name, "sleep_out");
        // 잠들기도 전에 깨우면 자지 않고 바로 일어난다.
        settle(&mut actor, &motions);
        actor.start(Whim::Sleep, &motions);
        actor.poke(&motions);
        assert_eq!(names(&run(&mut actor, &motions, &AROUND, 1.2)), ["sleep_in", "sleep_out", "idle2"]);
    }

    #[test]
    fn dozes_while_the_user_is_away_and_wakes_on_return() {
        let motions = sample();
        let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 7);
        actor.walk = Some(Dir::East);
        actor.wait = 1.0;
        actor.set_away(true);
        // 걷던 것을 마치고 쉬게 되면 잠든다. 정해진 시간(6~12초)이 지나도 깨지 않는다.
        let seen = run(&mut actor, &motions, &AROUND, 40.0);
        assert_eq!(names(&seen).last(), Some(&"sleep_loop"));
        assert!(names(&seen).contains(&"sleep_in"));
        // 돌아오면 깬다.
        actor.set_away(false);
        let seen = run(&mut actor, &motions, &AROUND, 1.0);
        assert_eq!(names(&seen), ["sleep_loop", "sleep_out", "idle2"]);
    }

    #[test]
    fn a_nap_continues_if_the_user_leaves_meanwhile() {
        let motions = sample();
        let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 7);
        actor.start(Whim::Sleep, &motions);
        run(&mut actor, &motions, &AROUND, 2.0);
        actor.set_away(true);
        run(&mut actor, &motions, &AROUND, 30.0);
        assert_eq!(motions.clip(actor.motion()).name, "sleep_loop");
    }

    #[test]
    fn returning_before_falling_asleep_cancels_the_nap() {
        let motions = sample();
        let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 7);
        actor.set_habits(Habits { roam: false, whim_rate: 0.0 });
        actor.t = 150.0;
        actor.set_away(true);
        actor.update(STEP, &motions, &AROUND);
        assert_eq!(actor.pending, Some(Whim::Sleep));
        actor.set_away(false);
        let seen = run(&mut actor, &motions, &AROUND, 5.0);
        assert_eq!(names(&seen), ["idle2"]);
    }

    #[test]
    fn sleeper_can_be_picked_up_and_asked_to_leave() {
        let motions = sample();
        let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 7);
        actor.start(Whim::Sleep, &motions);
        run(&mut actor, &motions, &AROUND, 2.0);
        assert!(actor.can_grab());
        // 사라지라고 하면 깨어나서 인사하고 사라진다.
        actor.leave(&motions);
        let seen = run(&mut actor, &motions, &AROUND, 6.0);
        assert_eq!(names(&seen), ["sleep_loop", "sleep_out", "idle2", "despawn"]);
        assert!(actor.is_gone());
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
        vanishing.leave(&motions);
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
        actor.leave(&motions);
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
        actor.leave(&motions);
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
        actor.leave(&motions);
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

    /// 바닥(y 1000)보다 400px 높이 떠 있는 창의 윗변. 발바닥이 설 수 있는 범위는 x 800~1200이다.
    const WINDOW: Perch = Perch { window: 7, y: 600.0, left: 800.0, right: 1200.0, origin: 730.0, head: false };
    /// 그 아래에 있는 더 넓은 창의 윗변.
    const LOWER: Perch = Perch { window: 8, y: 800.0, left: 600.0, right: 1400.0, origin: 530.0, head: false };

    /// 작업표시줄 위이고, 창들이 떠 있는 곳.
    fn among(perches: &[Perch]) -> Surroundings<'_> {
        Surroundings { perches, ..ON_LEDGE }
    }

    /// 창 윗변 `perch`의 `x`에 이미 올라서 있는 꽃핀.
    fn standing_on(motions: &Motions, perch: Perch, x: f64, seed: u64) -> Actor {
        let mut actor = Actor::stand(motions, x, perch.y, 1.0, seed);
        actor.on = Some((perch.window, perch.origin));
        actor
    }

    /// 잡아서 `to`로 옮긴 뒤 놓는다.
    fn carry_and_drop(actor: &mut Actor, motions: &Motions, around: &Surroundings, to: (f64, f64)) {
        assert!(actor.grab(motions, to));
        run(actor, motions, around, 1.0);
        actor.release(motions);
    }

    #[test]
    fn dropped_above_a_window_lands_on_its_top_edge_and_stays_there() {
        let motions = sample();
        let perches = [WINDOW];
        let around = among(&perches);
        let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 5);
        // 가끔 하는 행동은 끄고 쉬기와 걷기만 본다. 그러면 윗변 끝에서도 뛰어내리지 않고 돌아선다.
        actor.set_habits(Habits { whim_rate: 0.0, ..Habits::default() });
        carry_and_drop(&mut actor, &motions, &around, (1000.0, 300.0));
        let seen = run(&mut actor, &motions, &around, 5.0);
        assert_eq!(names(&seen)[..5], ["hang_to_fall", "fall", "land", "land_to_idle2", "idle2"]);
        // 놓은 자리의 바로 아래 윗변에 내려앉았다.
        assert_eq!((actor.support(), actor.position().1, actor.home()), (Some(7), 600.0, 1000.0));

        // 그 위에서 쉬고 걷는다. 윗변의 양 끝이 벽이다.
        let (mut low, mut high) = (f64::MAX, f64::MIN);
        for _ in 0..(1800.0 / STEP) as usize {
            actor.update(STEP, &motions, &around);
            let (x, y) = actor.position();
            assert_eq!((actor.mode(), actor.support(), y), (Mode::Ground, Some(7), 600.0));
            low = low.min(x);
            high = high.max(x);
        }
        assert!(low >= WINDOW.left && high <= WINDOW.right, "{low}..{high}");
        assert!(high - low > 300.0, "윗변을 돌아다니지 않음: {low}..{high}");
    }

    #[test]
    fn dropped_beside_or_below_the_edge_falls_to_the_floor() {
        let motions = sample();
        let perches = [WINDOW];
        let around = among(&perches);
        // 윗변의 범위 밖, 그리고 윗변보다 아래(창의 앞)에서 놓는다.
        for to in [(700.0, 300.0), (1000.0, 700.0)] {
            let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 5);
            actor.set_habits(Habits { roam: false, whim_rate: 0.0 });
            carry_and_drop(&mut actor, &motions, &around, to);
            run(&mut actor, &motions, &around, 5.0);
            let (x, y) = actor.position();
            assert_eq!((actor.support(), y), (None, AROUND.floor), "{to:?}");
            assert!((x - to.0).abs() < 1e-6, "{to:?}: {x}");
        }
    }

    #[test]
    fn put_down_right_at_the_edge_stands_on_it_at_once() {
        let motions = sample();
        let perches = [WINDOW];
        let around = among(&perches);
        // 윗변보다 조금 아래에 놓아도, 조금 위에 놓아도 그 위에 올려놓은 것이다.
        for y in [612.0, 590.0] {
            let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 5);
            carry_and_drop(&mut actor, &motions, &around, (1000.0, y));
            actor.update(STEP, &motions, &around);
            assert_eq!((actor.mode(), actor.support(), actor.position().1), (Mode::Land, Some(7), 600.0), "{y}");
        }
    }

    #[test]
    fn rides_along_when_the_window_moves() {
        let motions = sample();
        let mut actor = standing_on(&motions, WINDOW, 1000.0, 5);
        actor.set_habits(Habits { roam: false, whim_rate: 0.0 });
        run(&mut actor, &motions, &among(&[WINDOW]), 1.0);
        assert!(!actor.moves_smoothly());
        // 창이 오른쪽으로 100, 위로 50 움직였다.
        let moved = [Perch { y: 550.0, left: 900.0, right: 1300.0, origin: 830.0, ..WINDOW }];
        actor.update(STEP, &motions, &among(&moved));
        assert_eq!((actor.support(), actor.position()), (Some(7), (1100.0, 550.0)));
        // 움직이는 동안에는 자리를 자주 고친다. 멈추면 다시 장에 맞춰 산다.
        assert!(actor.moves_smoothly());
        assert!((actor.wake_in(&motions) - SMOOTH_STEP).abs() < 1e-9);
        run(&mut actor, &motions, &among(&moved), 1.0);
        assert!(!actor.moves_smoothly());
        assert_eq!((actor.mode(), actor.position()), (Mode::Ground, (1100.0, 550.0)));
    }

    /// `WINDOW`를 `(dx, dy)`만큼 옮긴 것.
    fn shifted(dx: f64, dy: f64) -> Perch {
        Perch {
            y: WINDOW.y + dy,
            left: WINDOW.left + dx,
            right: WINDOW.right + dx,
            origin: WINDOW.origin + dx,
            ..WINDOW
        }
    }

    /// 창을 `seconds`초 동안 걸음마다 `at(흐른 시간)`의 자리로 옮기며 시간을 흘린다. 아래에는 `LOWER`가 있다.
    /// 그동안 거친 동작을 돌려준다.
    fn move_window(
        actor: &mut Actor,
        motions: &Motions,
        seconds: f64,
        at: impl Fn(f64) -> (f64, f64),
    ) -> Vec<MotionId> {
        let steps = (seconds / STEP).round() as usize;
        let mut seen = vec![actor.motion()];
        for i in 1..=steps {
            let (dx, dy) = at(i as f64 * STEP);
            let perches = [shifted(dx, dy), LOWER];
            actor.update(STEP, motions, &among(&perches));
            if seen.last() != Some(&actor.motion()) {
                seen.push(actor.motion());
            }
        }
        seen
    }

    #[test]
    fn shaking_the_window_shakes_the_character_off() {
        let motions = sample();
        for (name, start, wave) in [
            ("쉬는 중에 좌우로", None, (60.0, 0.0)),
            ("걸터앉은 중에 좌우로", Some(Whim::Sit), (60.0, 0.0)),
            ("쉬는 중에 위아래로", None, (0.0, 40.0)),
        ] {
            let mut actor = standing_on(&motions, WINDOW, 1000.0, 5);
            actor.set_habits(Habits { roam: false, whim_rate: 0.0 });
            if let Some(whim) = start {
                actor.start(whim, &motions);
            }
            run(&mut actor, &motions, &among(&[WINDOW, LOWER]), 1.0);
            assert_eq!(actor.support(), Some(7), "{name}");
            // 0.4초에 한 번 왔다 갔다 한다. 1.2초면 방향을 여섯 번 바꾼다.
            let swing = |t: f64| (t * std::f64::consts::TAU / 0.4).sin();
            let shaken = move_window(&mut actor, &motions, 1.2, |t| (wave.0 * swing(t), wave.1 * swing(t)));
            assert!(shaken.contains(&motions.hang_to_fall), "{name}: {shaken:?}");
            assert_eq!(actor.support(), None, "{name}");
            // 아래에 다른 창이 있어도 지나쳐 바닥까지 떨어진다.
            let seen = run(&mut actor, &motions, &among(&[WINDOW, LOWER]), 5.0);
            assert!(names(&seen).contains(&"fall") && names(&seen).contains(&"land"), "{name}: {seen:?}");
            assert_eq!(
                (actor.mode(), actor.support(), actor.position().1),
                (Mode::Ground, None, AROUND.floor),
                "{name}"
            );
        }
    }

    #[test]
    fn moving_the_window_around_is_not_shaking() {
        let motions = sample();
        let mut actor = standing_on(&motions, WINDOW, 1000.0, 5);
        actor.set_habits(Habits { roam: false, whim_rate: 0.0 });
        run(&mut actor, &motions, &among(&[WINDOW, LOWER]), 1.0);
        // 오른쪽으로 300 옮겼다가 다시 왼쪽으로 되돌린다. 방향을 한 번 바꾼다.
        move_window(&mut actor, &motions, 2.0, |t| (300.0 * (1.0 - (t - 1.0).abs()), 0.0));
        // 끌고 가다 손이 떨려 조금씩 오간다. 한 번에 움직인 거리가 짧다.
        move_window(&mut actor, &motions, 1.0, |t| (8.0 * (t * std::f64::consts::TAU / 0.2).sin(), 0.0));
        // 천천히 왔다 갔다 한다. 방향을 바꾸는 사이가 길다.
        move_window(&mut actor, &motions, 6.0, |t| (200.0 * (t * std::f64::consts::TAU / 3.0).sin(), 0.0));
        assert_eq!((actor.mode(), actor.support()), (Mode::Ground, Some(7)));
    }

    /// `LOWER` 창 위에 선 다른 꽃핀의 머리. 그 꽃핀은 x 1000에 서 있고, 머리 꼭대기는 윗변보다 145px 높다.
    const HEAD: Perch = Perch { window: 20, y: 655.0, left: 980.0, right: 1020.0, origin: 1000.0, head: true };

    /// `HEAD`를 가로로 `dx`만큼 옮긴 것. 아래 꽃핀이 걸어간 것이다.
    fn head_at(dx: f64) -> Perch {
        Perch { left: HEAD.left + dx, right: HEAD.right + dx, origin: HEAD.origin + dx, ..HEAD }
    }

    /// 다른 꽃핀의 머리 위에 이미 서 있는 꽃핀.
    fn standing_on_head(motions: &Motions, seed: u64) -> Actor {
        let mut actor = standing_on(motions, HEAD, HEAD.origin, seed);
        actor.head = true;
        actor
    }

    #[test]
    fn dropped_near_a_head_stands_in_the_middle_of_it_and_rides_along() {
        let motions = sample();
        let perches = [HEAD, LOWER];
        let mut actor = Actor::stand(&motions, 700.0, LOWER.y, 1.0, 5);
        actor.set_habits(Habits { whim_rate: 0.0, ..Habits::default() });
        // 머리 가운데에서 15px 비켜 놓아도 머리 위에 올라서고, 가운데에 선다.
        carry_and_drop(&mut actor, &motions, &among(&perches), (1015.0, 640.0));
        run(&mut actor, &motions, &among(&perches), 3.0);
        assert_eq!((actor.mode(), actor.support(), actor.on_head()), (Mode::Ground, Some(20), true));
        assert_eq!(actor.position(), (1000.0, 655.0));
        // 아래 꽃핀이 걸어가면 따라간다. 머리 위에서는 스스로 걷지 않는다.
        for step in 1..=600 {
            let walked = [head_at(step as f64 * 0.5), LOWER];
            actor.update(STEP, &motions, &among(&walked));
            assert_eq!(actor.position().0, 1000.0 + step as f64 * 0.5);
        }
        assert_eq!(actor.support(), Some(20));
    }

    #[test]
    fn hops_off_a_head_sideways_and_lands_on_the_window_below() {
        let motions = sample();
        let perches = [HEAD, LOWER];
        for seed in 1..=6 {
            let mut actor = standing_on_head(&motions, seed);
            // 머리 위에 있는 동안 거친 동작. 머리 위에서는 걷거나 앉거나 자지 않는다.
            let mut on_top = Vec::new();
            for _ in 0..(60.0 / STEP) as usize {
                actor.update(STEP, &motions, &among(&perches));
                if !actor.on_head() {
                    break;
                }
                on_top.push(motions.clip(actor.motion()).name.clone());
            }
            assert!(
                on_top.iter().all(|name| ["idle2", "stretch", "yawn", "jump"].contains(&name.as_str())),
                "{on_top:?}"
            );
            assert_eq!(on_top.last().map(String::as_str), Some("jump"), "씨앗 {seed}");
            // 옆으로 뛰어내려, 지나치지 않고 아래에 있는 창 위에 내려선다.
            let seen = run(&mut actor, &motions, &among(&[LOWER]), 3.0);
            assert!(names(&seen).contains(&"land"), "씨앗 {seed}: {seen:?}");
            assert_eq!((actor.support(), actor.position().1), (Some(8), LOWER.y), "씨앗 {seed}");
            assert!((actor.position().0 - HEAD.origin).abs() > HEAD.right - HEAD.origin, "씨앗 {seed}");
        }
        // 자리를 비우면 내려가서 잔다.
        let mut actor = standing_on_head(&motions, 3);
        actor.set_away(true);
        let seen = run(&mut actor, &motions, &among(&perches), 10.0);
        assert!(names(&seen).contains(&"jump") && names(&seen).contains(&"sleep_loop"), "{seen:?}");
        assert_eq!((actor.mode(), actor.on_head(), actor.position().1), (Mode::Sleep, false, LOWER.y));
    }

    #[test]
    fn when_the_one_below_moves_on_steps_down_onto_the_window_below() {
        let motions = sample();
        let mut actor = standing_on_head(&motions, 5);
        actor.set_habits(Habits { roam: false, whim_rate: 0.0 });
        run(&mut actor, &motions, &among(&[HEAD, LOWER]), 1.0);
        // 아래 꽃핀이 잡혀 가거나, 다른 행동을 하거나, 돌아갔다. 창을 지나치지 않고 바로 아래 창에 내려선다.
        let seen = run(&mut actor, &motions, &among(&[LOWER]), 5.0);
        assert!(names(&seen).contains(&"fall") && names(&seen).contains(&"land"), "{seen:?}");
        assert_eq!((actor.mode(), actor.support(), actor.position()), (Mode::Ground, Some(8), (1000.0, LOWER.y)));
    }

    #[test]
    fn carrying_someone_only_rests_and_walks() {
        let motions = sample();
        for seed in 1..=4 {
            let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, seed);
            // 가끔 하는 행동을 아주 자주 하게 해도, 머리에 누가 서 있는 동안에는 하지 않는다. 자리를 비워도 자지 않는다.
            actor.set_habits(Habits { whim_rate: 5.0, ..Habits::default() });
            actor.set_carrying(true);
            actor.set_away(seed % 2 == 0);
            let seen = run(&mut actor, &motions, &among(&[WINDOW]), 60.0);
            assert!(names(&seen).iter().all(|name| ["idle", "idle2", "walk_w", "walk_e"].contains(name)), "{seen:?}");
            // 내려가면 다시 한다.
            actor.set_carrying(false);
            let seen = run(&mut actor, &motions, &among(&[WINDOW]), 30.0);
            assert!(names(&seen).iter().any(|name| !["idle", "idle2", "walk_w", "walk_e"].contains(name)), "{seen:?}");
        }
    }

    #[test]
    fn walks_over_to_a_spot_and_stops_there() {
        let motions = sample();
        let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 5);
        actor.set_habits(Habits { roam: false, whim_rate: 0.0 });
        assert!(actor.come_to(1300.0));
        let seen = run(&mut actor, &motions, &AROUND, 10.0);
        assert_eq!(names(&seen)[..3], ["idle2", "walk_e", "idle2"]);
        assert!((actor.position().0 - 1300.0).abs() <= ARRIVE + 2.0, "{:?}", actor.position());
        // 이미 가까이에 있으면 가지 않는다.
        assert!(!actor.come_to(actor.position().0 + 10.0));
        // 왼쪽으로도 간다. 돌아다니는 꽃핀도 걷던 것을 그만두고 간다.
        actor.set_habits(Habits::default());
        assert!(actor.come_to(700.0));
        run(&mut actor, &motions, &AROUND, 0.1);
        assert_eq!(actor.motion(), motions.walk_w);
        for _ in 0..(30.0 / STEP) as usize {
            actor.update(STEP, &motions, &AROUND);
            if actor.motion() != motions.walk_w {
                break;
            }
        }
        assert!((actor.position().0 - 700.0).abs() <= ARRIVE + 2.0, "{:?}", actor.position());
        // 디딘 창의 끝까지 가도 닿지 않으면 끝에서 멈춘다. 머리 위에 있거나 다른 행동을 하면 가지 않는다.
        let mut on_window = standing_on(&motions, WINDOW, 1000.0, 5);
        on_window.set_habits(Habits { roam: false, whim_rate: 0.0 });
        assert!(on_window.come_to(1500.0));
        run(&mut on_window, &motions, &among(&[WINDOW]), 20.0);
        assert_eq!(
            (on_window.mode(), on_window.support(), on_window.position().0),
            (Mode::Ground, Some(7), WINDOW.right)
        );
        assert!(!standing_on_head(&motions, 5).come_to(1500.0));
        let mut busy = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 5);
        busy.start(Whim::Stretch, &motions);
        assert!(!busy.come_to(1500.0));
    }

    #[test]
    fn falls_to_the_floor_when_the_window_goes_away() {
        let motions = sample();
        for (name, start) in [("쉬는 중", None), ("자는 중", Some(Whim::Sleep)), ("걸터앉은 중", Some(Whim::Sit))]
        {
            let mut actor = standing_on(&motions, WINDOW, 1000.0, 5);
            actor.set_habits(Habits { roam: false, whim_rate: 0.0 });
            if let Some(whim) = start {
                actor.start(whim, &motions);
            }
            run(&mut actor, &motions, &among(&[WINDOW, LOWER]), 1.0);
            assert_eq!(actor.support(), Some(7), "{name}");
            // 창을 닫았다. 아래에 다른 창이 있어도 그 위에 서지 않고 바닥까지 떨어진다.
            let left = [LOWER];
            let seen = run(&mut actor, &motions, &among(&left), 5.0);
            assert!(names(&seen).contains(&"fall") && names(&seen).contains(&"land"), "{name}: {seen:?}");
            assert_eq!((actor.support(), actor.position()), (None, (1000.0, AROUND.floor)), "{name}");
            assert_eq!(actor.mode(), Mode::Ground, "{name}");
        }
    }

    #[test]
    fn falls_when_the_spot_gets_covered() {
        let motions = sample();
        let mut actor = standing_on(&motions, WINDOW, 1000.0, 5);
        actor.set_habits(Habits { roam: false, whim_rate: 0.0 });
        run(&mut actor, &motions, &among(&[WINDOW]), 1.0);
        // 다른 창이 앞으로 나와 선 자리를 가렸다. 그 창의 윗변은 양쪽 토막만 남았다.
        let covered = [Perch { right: 900.0, ..WINDOW }, Perch { left: 1100.0, ..WINDOW }];
        run(&mut actor, &motions, &among(&covered), 5.0);
        assert_eq!((actor.support(), actor.position()), (None, (1000.0, AROUND.floor)));
    }

    #[test]
    fn picking_up_from_a_window_lets_go_of_it() {
        let motions = sample();
        let mut actor = standing_on(&motions, WINDOW, 1000.0, 5);
        let perches = [WINDOW];
        run(&mut actor, &motions, &among(&perches), 0.5);
        assert!(actor.grab(&motions, (300.0, 300.0)));
        assert_eq!(actor.support(), None);
        run(&mut actor, &motions, &among(&perches), 1.0);
        actor.release(&motions);
        run(&mut actor, &motions, &among(&perches), 5.0);
        assert_eq!((actor.support(), actor.position().1), (None, AROUND.floor));
    }

    #[test]
    fn leaps_onto_a_window_however_high_in_one_jump() {
        let motions = sample();
        // (윗변의 높이, 처음 선 x, 내려앉을 x, 배율). 윗변 아래에서도, 옆에서도, 아주 높아도 한 번에 올라간다.
        let cases = [
            (100.0, 1000.0, 1000.0, 1.0),
            (400.0, 1350.0, 1200.0, 1.0),
            (840.0, 500.0, 800.0, 1.0),
            (840.0, 1700.0, 1200.0, 2.0),
        ];
        for (height, from, to, scale) in cases {
            let perches = [Perch { y: AROUND.floor - height, ..WINDOW }];
            let around = among(&perches);
            let mut actor = Actor::stand(&motions, from, AROUND.floor, scale, 3);
            actor.set_habits(Habits { whim_rate: 0.0, ..Habits::default() });
            actor.start(Whim::Climb, &motions);
            let (mut airborne, mut highest) = (0.0, f64::MAX);
            let mut seen = Vec::new();
            for _ in 0..(4.0 / STEP) as usize {
                actor.update(STEP, &motions, &around);
                if actor.is_airborne() {
                    airborne += STEP;
                }
                highest = highest.min(actor.position().1);
                seen.push(motions.clip(actor.motion()).name.clone());
                if actor.mode() == Mode::Land {
                    break;
                }
            }
            seen.dedup();
            let case = format!("높이 {height}, {from} → {to}, {scale}배");
            assert_eq!((actor.mode(), actor.support()), (Mode::Land, Some(7)), "{case}: {seen:?}");
            assert_eq!(actor.position(), (to, AROUND.floor - height), "{case}");
            // 떠 있는 시간은 높이와 상관없이 같고, 내려앉을 곳보다 조금 더 높이 떴다가 내려앉는다.
            assert!((airborne - LEAP_AIR).abs() < 2.5 * STEP, "{case}: {airborne}초");
            let cleared = AROUND.floor - height - highest;
            assert!((cleared - LEAP_CLEAR * scale).abs() < 2.0, "{case}: {cleared}px 더 뜸");
            assert!(seen.contains(&"jump".to_string()), "{case}: {seen:?}");
        }
    }

    #[test]
    fn does_not_leap_at_a_window_too_far_to_the_side() {
        let motions = sample();
        let perches = [WINDOW];
        let around = among(&perches);
        // 윗변의 오른쪽 끝(1200)에서 321px 떨어져 있다. 320px까지만 뛰어오른다.
        let mut actor = Actor::stand(&motions, 1521.0, AROUND.floor, 1.0, 3);
        actor.set_habits(Habits { roam: false, whim_rate: 0.0 });
        assert!(actor.climb_target(&around).is_none());
        actor.start(Whim::Climb, &motions);
        let seen = run(&mut actor, &motions, &around, 4.0);
        assert_eq!(names(&seen), ["idle2"]);
        actor.x = 1520.0;
        assert_eq!(actor.climb_target(&around), Some(WINDOW));
        // 가까운 윗변이 여럿이면 옆으로 가장 가까운 것을 고른다.
        let two = [WINDOW, Perch { window: 9, y: 300.0, left: 1400.0, right: 1800.0, origin: 1330.0, head: false }];
        assert_eq!(actor.climb_target(&among(&two)).map(|perch| perch.window), Some(9));
    }

    #[test]
    fn a_leap_whose_window_moved_away_falls_to_the_floor() {
        let motions = sample();
        let perches = [WINDOW];
        let mut actor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 3);
        actor.set_habits(Habits { roam: false, whim_rate: 0.0 });
        actor.start(Whim::Climb, &motions);
        // 떠 있는 동안 창이 옆으로 치워졌다.
        let mut gone = false;
        for _ in 0..(6.0 / STEP) as usize {
            if actor.is_airborne() {
                gone = true;
            }
            let moved = [Perch { left: 1500.0, right: 1900.0, origin: 1430.0, ..WINDOW }];
            actor.update(STEP, &motions, &among(if gone { &moved } else { &perches }));
        }
        assert!(gone);
        assert_eq!((actor.mode(), actor.support(), actor.position()), (Mode::Ground, None, (1000.0, AROUND.floor)));
    }

    #[test]
    fn hopping_off_goes_all_the_way_down_to_the_floor() {
        let motions = sample();
        let perches = [WINDOW, LOWER];
        let around = among(&perches);
        let mut actor = standing_on(&motions, WINDOW, 1000.0, 5);
        actor.set_habits(Habits { roam: false, whim_rate: 0.0 });
        actor.start(Whim::Hop, &motions);
        let seen = run(&mut actor, &motions, &around, 6.0);
        assert_eq!(names(&seen), ["idle2", "jump", "fall", "land", "land_to_idle2", "idle2"]);
        // 아래에 있는 창을 지나쳐 바닥에 내려앉는다.
        assert_eq!((actor.support(), actor.position()), (None, (1000.0, AROUND.floor)));
        // 바닥에서는 뛰어내릴 것이 없다. 시켜도 그냥 쉰다.
        actor.start(Whim::Hop, &motions);
        assert_eq!(names(&run(&mut actor, &motions, &around, 3.0)), ["idle2"]);
    }

    #[test]
    fn at_the_end_of_the_edge_turns_around_or_hops_off() {
        let motions = sample();
        let perches = [WINDOW];
        let around = among(&perches);
        let (mut hopped, mut turned) = (0, 0);
        for seed in 1..=200 {
            let mut actor = standing_on(&motions, WINDOW, WINDOW.right - 20.0, seed);
            // 가끔 하는 행동을 아주 드물게 해서, 끝에 닿았을 때 하는 일만 본다.
            actor.set_habits(Habits { roam: true, whim_rate: 1e-9 });
            actor.walk = Some(Dir::East);
            actor.wait = 10.0;
            let seen = run(&mut actor, &motions, &around, 2.0);
            if names(&seen).contains(&"jump") {
                hopped += 1;
                run(&mut actor, &motions, &around, 5.0);
                // 바깥쪽으로 뛰어서 창의 오른쪽 바닥에 내려앉는다.
                assert_eq!((actor.support(), actor.position().1), (None, AROUND.floor), "씨앗 {seed}");
                assert!(actor.position().0 > WINDOW.right + 50.0, "씨앗 {seed}: {}", actor.position().0);
            } else {
                turned += 1;
                assert_eq!(names(&seen)[..3], ["idle2", "walk_e", "idle"], "씨앗 {seed}");
                assert_eq!(actor.support(), Some(7), "씨앗 {seed}");
            }
        }
        // 뛰어내리는 것이 35%쯤이다.
        assert!((50..=90).contains(&hopped), "뛰어내림 {hopped}, 돌아섬 {turned}");
    }

    #[test]
    fn jumping_in_place_on_a_window_comes_back_down_onto_it() {
        let motions = sample();
        let perches = [WINDOW, LOWER];
        let around = among(&perches);
        let mut actor = standing_on(&motions, WINDOW, 1000.0, 5);
        actor.set_habits(Habits { roam: false, whim_rate: 0.0 });
        actor.start(Whim::Jump, &motions);
        let seen = run(&mut actor, &motions, &around, 6.0);
        assert!(names(&seen).contains(&"land"));
        assert_eq!((actor.mode(), actor.support(), actor.position()), (Mode::Ground, Some(7), (1000.0, 600.0)));
    }

    #[test]
    fn leaping_and_hopping_are_picked_only_where_they_make_sense() {
        let motions = sample();
        let share = |actor: &mut Actor, around: &Surroundings, whim: Whim| {
            let can_climb = actor.climb_target(around).is_some();
            (0..20000).filter(|_| actor.pick_whim(around.ledge, can_climb) == Some(whim)).count() as f64 / 200.0
        };
        let perches = [WINDOW];
        let around = among(&perches);
        // 바닥에서, 가까이에 창이 있으면 쉬기를 마칠 때 10%로 뛰어오른다. 뛰어내리기는 고르지 않는다.
        let mut below = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 11);
        assert!((share(&mut below, &around, Whim::Climb) - 10.0).abs() < 1.0);
        assert_eq!(share(&mut below, &around, Whim::Hop), 0.0);
        // 창이 없으면 뛰어오르기도 고르지 않는다.
        assert_eq!(share(&mut below, &ON_LEDGE, Whim::Climb), 0.0);
        // 창 위에서는 8%로 뛰어내린다. 옮겨 뛸 다른 창이 없으면 뛰어오르기는 고르지 않는다.
        let mut above = standing_on(&motions, WINDOW, 1000.0, 11);
        assert!((share(&mut above, &around, Whim::Hop) - 8.0).abs() < 1.0);
        assert_eq!(share(&mut above, &around, Whim::Climb), 0.0);
        // 다른 창이 가까이에 있으면 그리로 옮겨 뛰기도 한다.
        let two = [WINDOW, LOWER];
        assert!((share(&mut above, &among(&two), Whim::Climb) - 10.0).abs() < 1.0);
    }

    #[test]
    fn leaps_from_one_window_down_to_another() {
        let motions = sample();
        let perches = [WINDOW, LOWER];
        let around = among(&perches);
        let mut actor = standing_on(&motions, WINDOW, 1000.0, 5);
        actor.set_habits(Habits { roam: false, whim_rate: 0.0 });
        actor.start(Whim::Climb, &motions);
        run(&mut actor, &motions, &around, 6.0);
        assert_eq!((actor.mode(), actor.support(), actor.position()), (Mode::Ground, Some(8), (1000.0, 800.0)));
    }

    #[test]
    fn the_shadow_falls_on_the_window_under_the_feet() {
        let motions = sample();
        let perches = [WINDOW];
        let around = among(&perches);
        let mut actor = standing_on(&motions, WINDOW, 1000.0, 5);
        assert_eq!(actor.ground_below(&around), 600.0);
        // 윗변 위에 떠 있을 때도 그 윗변에 진다. 윗변보다 아래에 있거나 옆으로 벗어나 있으면 바닥에 진다.
        actor.y = 400.0;
        assert_eq!(actor.ground_below(&around), 600.0);
        actor.y = 700.0;
        assert_eq!(actor.ground_below(&around), AROUND.floor);
        (actor.x, actor.y) = (1300.0, 400.0);
        assert_eq!(actor.ground_below(&around), AROUND.floor);
        // 윗변을 지나쳐 바닥까지 내려가는 중이면 바닥에 진다.
        (actor.x, actor.through) = (1000.0, true);
        assert_eq!(actor.ground_below(&around), AROUND.floor);
    }

    #[test]
    fn on_the_beat_every_frame_shows_exactly_once_on_a_window_too() {
        // 창 위에서 쉬고 걷고, 뛰어내리고, 다시 뛰어오르는 동안에도 박자마다 장이 꼭 하나씩 넘어간다.
        // 떠 있는 동안에는 박자보다 자주 움직이므로 그때는 세지 않는다.
        let motions = sample_24fps();
        let perches = [WINDOW];
        let around = among(&perches);
        let beat_ns = |beat: u64| (beat as f64 * 1e9 / 24.0).round() as u64;
        for seed in 1..=4 {
            let mut actor = standing_on(&motions, WINDOW, 1000.0, seed);
            let mut before = (actor.motion(), actor.frame(&motions));
            let (mut on_window, mut on_floor) = (0u32, 0u32);
            for beat in 1..=24 * 1200 {
                let dt = (beat_ns(beat) - beat_ns(beat - 1)) as f64 / 1e9;
                let smooth = actor.moves_smoothly();
                actor.update(dt, &motions, &around);
                let now = (actor.motion(), actor.frame(&motions));
                if actor.mode() == Mode::Ground {
                    if actor.support().is_some() { on_window += 1 } else { on_floor += 1 }
                }
                let clip = motions.clip(now.0);
                let first = clip.first_frame;
                let last = first + clip.frame_at(clip.total_ms - 1.0);
                let ok = smooth
                    || actor.moves_smoothly()
                    || if now.0 != before.0 {
                        now.1 == first
                    } else {
                        now.1 == before.1 + 1 || (clip.looping && before.1 == last && now.1 == first)
                    };
                assert!(ok, "씨앗 {seed}, {beat}번째 박자: {} {} -> {} {}", before.0, before.1, clip.name, now.1);
                before = now;
            }
            // 20분 동안 창 위에도 있었고 바닥에도 있었다.
            assert!(on_window > 24 * 30 && on_floor > 24 * 30, "씨앗 {seed}: 창 위 {on_window}, 바닥 {on_floor}");
        }
    }

    #[test]
    fn a_picked_action_starts_at_once() {
        let motions = sample();
        let perches = [WINDOW, LOWER];
        let around = among(&perches);
        // 창 위에 서 있으면 모든 행동을 골라 볼 수 있다.
        let expected = [
            (Show::Stretch, Mode::Act, "stretch"),
            (Show::Yawn, Mode::Act, "yawn"),
            (Show::Jump, Mode::Jump, "jump"),
            (Show::Sleep, Mode::Sleep, "sleep_in"),
            (Show::Sit, Mode::Sit, "edge_in"),
            (Show::Climb, Mode::Jump, "jump"),
            (Show::Hop, Mode::Jump, "jump"),
            (Show::Surprise, Mode::Act, "poke_surprise"),
            (Show::Pout, Mode::Act, "poke_pout"),
            (Show::Shy, Mode::Act, "poke_shy"),
            (Show::Pat, Mode::Act, "pat"),
        ];
        assert_eq!(expected.map(|(show, ..)| show), Show::ALL);
        for (show, mode, motion) in expected {
            let mut actor = standing_on(&motions, WINDOW, 1000.0, 5);
            // 걷는 중에 골라도 멈춰 서서 바로 한다.
            actor.walk = Some(Dir::East);
            actor.motion = motions.walk_e;
            assert!(actor.can_show(show, &around), "{show:?}");
            assert!(actor.show(show, &motions, &around), "{show:?}");
            assert_eq!((actor.mode(), motions.clip(actor.motion()).name.as_str()), (mode, motion), "{show:?}");
            assert_eq!((actor.walk, actor.t), (None, 0.0), "{show:?}");
        }
        // 창 위로 올라가기는 다른 창(아래 창)으로 옮겨 뛰고, 뛰어내리기는 바닥까지 간다.
        let mut actor = standing_on(&motions, WINDOW, 1000.0, 5);
        actor.set_habits(Habits { roam: false, whim_rate: 0.0 });
        assert!(actor.show(Show::Climb, &motions, &around));
        run(&mut actor, &motions, &around, 5.0);
        assert_eq!((actor.support(), actor.position().1), (Some(8), 800.0));
        assert!(actor.show(Show::Hop, &motions, &around));
        run(&mut actor, &motions, &around, 5.0);
        assert_eq!((actor.support(), actor.position().1), (None, AROUND.floor));
    }

    #[test]
    fn only_actions_that_make_sense_here_can_be_picked() {
        let motions = sample();
        let perches = [WINDOW];
        let pickable = |actor: &Actor, around: &Surroundings| -> Vec<Show> {
            Show::ALL.into_iter().filter(|&show| actor.can_show(show, around)).collect()
        };
        let always = [Show::Stretch, Show::Yawn, Show::Jump, Show::Sleep];
        let reactions = [Show::Surprise, Show::Pout, Show::Shy, Show::Pat];
        let list = |middle: &[Show]| [&always[..], middle, &reactions[..]].concat();

        // 화면 아래 끝이 바닥이고 창이 없는 곳: 걸터앉을 단도, 올라갈 창도, 뛰어내릴 곳도 없다.
        let floor = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 5);
        assert_eq!(pickable(&floor, &AROUND), list(&[]));
        // 작업표시줄 위: 걸터앉을 수 있다. 가까이에 창이 있으면 올라갈 수 있다.
        assert_eq!(pickable(&floor, &ON_LEDGE), list(&[Show::Sit]));
        assert_eq!(pickable(&floor, &among(&perches)), list(&[Show::Sit, Show::Climb]));
        // 창 위: 걸터앉고 뛰어내릴 수 있다. 옮겨 뛸 다른 창이 없으면 올라가기는 고를 수 없다.
        let above = standing_on(&motions, WINDOW, 1000.0, 5);
        assert_eq!(pickable(&above, &among(&perches)), list(&[Show::Sit, Show::Hop]));

        // 떠 있거나, 잡혀 있거나, 사라지는 중에는 아무것도 고를 수 없다.
        let mut falling = Actor::stand(&motions, 1000.0, 300.0, 1.0, 5);
        falling.update(STEP, &motions, &AROUND);
        let mut held = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 5);
        held.grab(&motions, (1000.0, 500.0));
        let mut leaving = Actor::stand(&motions, 1000.0, AROUND.floor, 1.0, 5);
        leaving.leave(&motions);
        for mut actor in [falling, held, leaving] {
            assert_eq!(pickable(&actor, &ON_LEDGE), []);
            let before = (actor.mode(), actor.motion());
            assert!(!actor.show(Show::Stretch, &motions, &ON_LEDGE));
            assert_eq!((actor.mode(), actor.motion()), before);
        }
    }

    #[test]
    fn a_picked_action_interrupts_what_it_was_doing() {
        // 하품하는 중, 자는 중, 걸터앉은 중에도 고를 수 있다. 하던 것을 그만두고 고른 행동을 바로 한다.
        let motions = sample();
        for busy_with in [Whim::Yawn, Whim::Sleep, Whim::Sit] {
            let mut actor = Actor::stand(&motions, 1000.0, ON_LEDGE.floor, 1.0, 5);
            actor.set_habits(Habits { roam: false, whim_rate: 0.0 });
            actor.start(busy_with, &motions);
            run(&mut actor, &motions, &ON_LEDGE, 0.5);
            assert_ne!(actor.mode(), Mode::Ground, "{busy_with:?}");
            assert!(actor.can_show(Show::Stretch, &ON_LEDGE), "{busy_with:?}");
            assert!(actor.show(Show::Stretch, &motions, &ON_LEDGE), "{busy_with:?}");
            assert_eq!((actor.mode(), motions.clip(actor.motion()).name.as_str()), (Mode::Act, "stretch"));
            // 고른 행동을 마치면 서서 쉰다. 하던 것으로 돌아가지 않는다.
            let seen = run(&mut actor, &motions, &ON_LEDGE, 3.0);
            assert_eq!(names(&seen), ["stretch", "idle2"], "{busy_with:?}");
        }
    }

    #[test]
    fn settling_stops_everything_and_stands_on_the_new_floor() {
        let motions = sample();
        let mut actor = Actor::appear(&motions, 300.0, 100.0, 1.0, 7);
        // 나타나서 떨어지는 중이었다.
        run(&mut actor, &motions, &AROUND, 0.5);
        assert!(actor.is_airborne());
        actor.settle_at(&motions, (450.0, 700.0));
        assert_eq!((actor.mode(), actor.motion(), actor.position()), (Mode::Ground, motions.idle2, (450.0, 700.0)));
        assert_eq!((actor.home(), actor.support()), (450.0, None));
        // 돌아가라는 말을 들었으면 선 뒤에 손을 흔들고 사라진다.
        actor.leave(&motions);
        actor.settle_at(&motions, (460.0, 700.0));
        assert!(actor.is_leaving() && actor.mode() == Mode::Ground);
        // 옮기면 하던 일은 그대로이고 자리만 같이 옮겨 간다.
        actor.shift((-30.0, 12.0));
        assert_eq!((actor.mode(), actor.position(), actor.home()), (Mode::Ground, (430.0, 712.0), 430.0));
    }
}
