//! 쉬는 시간 알림(집중 타이머). 정한 시간 동안 집중하면 쉬자고 알리고, 쉬는 시간이 끝나면 다시 알린다.
//!
//! 시간만 센다. 알리는 일(말풍선, 꽃핀들의 기지개)은 앱이 한다. 사용자가 자리를 비운 동안에는 집중 시간이
//! 흐르지 않는다. 쉬는 시간은 자리를 비워도 흐른다. 잠깐 멈춰 두면 둘 다 흐르지 않는다.

use std::time::{Duration, Instant};

/// 판에서 한 번에 고르는 집중 시간과 쉬는 시간(분).
pub const FOCUS_PICKS: [u32; 2] = [25, 50];
pub const REST_PICKS: [u32; 2] = [5, 10];
/// 판에서 화살표나 휠로 바꿀 수 있는 범위(분)와 한 번에 바뀌는 양.
const FOCUS_RANGE: (u32, u32, u32) = (5, 120, 5);
const REST_RANGE: (u32, u32, u32) = (1, 30, 1);

/// 판에서 고른 집중 시간과 쉬는 시간(분), 쉬는 시간이 끝나면 저절로 다시 집중할지. 기억해 둔다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    pub focus: u32,
    pub rest: u32,
    pub auto: bool,
}

impl Default for Plan {
    fn default() -> Plan {
        Plan { focus: FOCUS_PICKS[0], rest: REST_PICKS[0], auto: false }
    }
}

impl Plan {
    /// 범위를 벗어난 값은 범위 안으로 들인다. 파일에서 읽은 값에 쓴다.
    pub fn fit(self) -> Plan {
        Plan {
            focus: self.focus.clamp(FOCUS_RANGE.0, FOCUS_RANGE.1),
            rest: self.rest.clamp(REST_RANGE.0, REST_RANGE.1),
            auto: self.auto,
        }
    }

    /// 집중 시간(`rest`가 아니면)이나 쉬는 시간을 화살표로 `steps`칸 바꾼다.
    pub fn step(&mut self, rest: bool, steps: i32) {
        let ((least, most, unit), value) =
            if rest { (REST_RANGE, &mut self.rest) } else { (FOCUS_RANGE, &mut self.focus) };
        // 칸에 맞지 않는 값(예: 7분)에서 움직이면 먼저 칸에 맞춘다.
        let snapped = (*value / unit * unit).max(least) as i32;
        let moved = if steps < 0 && snapped < *value as i32 { snapped } else { snapped + steps * unit as i32 };
        *value = (moved.max(0) as u32).clamp(least, most);
    }
}

/// 지금 무엇을 하는 중인지.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Focus,
    Break,
}

/// 시간이 다 되어 일어난 일.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Turn {
    /// 집중 시간이 끝났다. 쉬는 시간이 시작된다.
    BreakTime,
    /// 쉬는 시간이 끝났다. 타이머도 끝난다.
    BackToWork,
    /// 쉬는 시간이 끝나고, 저절로 다시 집중을 시작했다.
    Again,
}

/// 돌고 있는 타이머.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Focus {
    /// 집중 시간과 쉬는 시간.
    lengths: (Duration, Duration),
    /// 쉬는 시간이 끝나면 저절로 다시 집중한다.
    auto: bool,
    phase: Phase,
    /// 지금 하는 것이 끝날 때까지 남은 시간.
    left: Duration,
    /// 마지막으로 시간을 흘린 시각.
    last: Instant,
    /// 잠깐 멈춰 두었다.
    paused: bool,
}

impl Focus {
    /// 고른 대로 집중을 시작한다.
    pub fn start(plan: Plan, now: Instant) -> Focus {
        let minutes = |value: u32| Duration::from_secs(u64::from(value) * 60);
        Focus::with((minutes(plan.focus), minutes(plan.rest)), plan.auto, now)
    }

    /// 시간을 직접 정해서 시작한다. 확인용으로 짧게 돌려 볼 때 쓴다.
    pub fn with(lengths: (Duration, Duration), auto: bool, now: Instant) -> Focus {
        Focus { lengths, auto, phase: Phase::Focus, left: lengths.0, last: now, paused: false }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 지금 하는 것의 길이.
    pub fn total(&self) -> Duration {
        match self.phase {
            Phase::Focus => self.lengths.0,
            Phase::Break => self.lengths.1,
        }
    }

    /// 집중 시간의 길이.
    pub fn focus(&self) -> Duration {
        self.lengths.0
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn set_auto(&mut self, auto: bool) {
        self.auto = auto;
    }

    /// 잠깐 멈추거나(`pause`) 다시 흐르게 한다. 멈추기 전까지의 시간은 흘려 둔다.
    pub fn pause(&mut self, pause: bool, now: Instant, away: bool) -> Option<Turn> {
        let turn = self.advance(now, away);
        self.paused = pause;
        self.last = now;
        turn
    }

    /// 지금 하는 것을 끝내고 바로 다음으로 넘어간다. 집중 중이면 쉬기로, 쉬는 중이면 다시 집중으로.
    /// 잠깐 멈춰 두었으면 다시 흐른다.
    pub fn skip(&mut self, now: Instant) {
        self.phase = match self.phase {
            Phase::Focus => Phase::Break,
            Phase::Break => Phase::Focus,
        };
        (self.left, self.last, self.paused) = (self.total(), now, false);
    }

    /// 쉬는 시간의 길이.
    pub fn rest(&self) -> Duration {
        self.lengths.1
    }

    /// 지금 하는 것이 끝날 때까지 남은 시간.
    #[cfg(test)]
    pub fn left(&self) -> Duration {
        self.left
    }

    /// `now`까지 시간을 흘린다. `away`면 사용자가 자리를 비운 것이라 집중 시간은 흐르지 않는다.
    /// 시간이 다 됐으면 일어난 일을 돌려준다. [`Turn::BackToWork`]면 타이머는 끝난 것이다.
    pub fn advance(&mut self, now: Instant, away: bool) -> Option<Turn> {
        let passed = now.saturating_duration_since(self.last);
        self.last = now;
        if self.paused || (self.phase == Phase::Focus && away) {
            return None;
        }
        self.left = self.left.saturating_sub(passed);
        if !self.left.is_zero() {
            return None;
        }
        match self.phase {
            Phase::Focus => {
                self.phase = Phase::Break;
                self.left = self.lengths.1;
                Some(Turn::BreakTime)
            }
            Phase::Break if self.auto => {
                self.phase = Phase::Focus;
                self.left = self.lengths.0;
                Some(Turn::Again)
            }
            Phase::Break => Some(Turn::BackToWork),
        }
    }

    /// 지금 하는 것이 끝날 시각. 잠깐 멈춰 두었거나 자리를 비워 집중 시간이 멈춰 있으면 없다.
    pub fn due(&self, away: bool) -> Option<Instant> {
        (!self.paused && (self.phase == Phase::Break || !away)).then(|| self.last + self.left)
    }

    /// 남은 시간을 초로 올려 센 값이 다음에 바뀌는 시각. 판에 남은 시간을 보여 주는 동안 깨어날 때를 정한다.
    /// 시간이 멈춰 있으면 없다.
    pub fn next_tick(&self, away: bool) -> Option<Instant> {
        self.due(away)?;
        let nanos = self.left.subsec_nanos();
        let step = if nanos == 0 { Duration::from_secs(1) } else { Duration::from_nanos(u64::from(nanos)) };
        Some(self.last + step)
    }

    /// 남은 초. 올려 센다(시작하자마자 25:00으로 보이게).
    pub fn left_secs(&self) -> u64 {
        self.left.as_secs() + u64::from(self.left.subsec_nanos() > 0)
    }

    /// 메뉴에 보여 줄 지금의 상태. 남은 시간은 분으로 올려 센다.
    pub fn label(&self) -> String {
        let minutes = self.left.as_secs().div_ceil(60);
        let paused = if self.paused { ", 잠깐 멈춤" } else { "" };
        match self.phase {
            Phase::Focus => format!("집중하는 중 ({minutes}분 남음{paused})"),
            Phase::Break => format!("쉬는 중 ({minutes}분 남음{paused})"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: Duration = Duration::from_secs(60);

    #[test]
    fn rests_after_focusing_then_ends() {
        let start = Instant::now();
        let mut focus = Focus::start(Plan::default(), start);
        assert_eq!((focus.phase(), focus.label()), (Phase::Focus, "집중하는 중 (25분 남음)".to_string()));
        assert_eq!(focus.advance(start + 10 * MINUTE, false), None);
        assert_eq!(focus.label(), "집중하는 중 (15분 남음)");
        assert_eq!(focus.due(false), Some(start + 25 * MINUTE));
        assert_eq!(focus.advance(start + 25 * MINUTE, false), Some(Turn::BreakTime));
        assert_eq!((focus.phase(), focus.label()), (Phase::Break, "쉬는 중 (5분 남음)".to_string()));
        assert_eq!(focus.advance(start + 29 * MINUTE, false), None);
        assert_eq!(focus.advance(start + 30 * MINUTE, false), Some(Turn::BackToWork));
    }

    #[test]
    fn being_away_pauses_the_focus_but_not_the_break() {
        let start = Instant::now();
        let mut focus = Focus::start(Plan { focus: 50, rest: 10, auto: false }, start);
        focus.advance(start + 20 * MINUTE, false);
        // 10분 동안 자리를 비웠다. 집중 시간은 그대로다.
        assert_eq!(focus.advance(start + 30 * MINUTE, true), None);
        assert_eq!(focus.due(true), None);
        assert_eq!(focus.label(), "집중하는 중 (30분 남음)");
        assert_eq!(focus.advance(start + 60 * MINUTE, false), Some(Turn::BreakTime));
        // 쉬는 시간은 자리를 비워도 흐른다.
        assert_eq!(focus.due(true), Some(start + 70 * MINUTE));
        assert_eq!(focus.advance(start + 70 * MINUTE, true), Some(Turn::BackToWork));
    }

    #[test]
    fn a_late_wake_still_turns_once() {
        let start = Instant::now();
        let mut focus = Focus::with((Duration::from_secs(10), Duration::from_secs(5)), false, start);
        // 절전에서 돌아와 한참 늦게 깼다. 한 번에 하나씩만 넘어간다.
        assert_eq!(focus.advance(start + MINUTE, false), Some(Turn::BreakTime));
        assert_eq!(focus.left(), Duration::from_secs(5));
    }

    #[test]
    fn pausing_stops_both_and_skipping_moves_on() {
        let start = Instant::now();
        let mut focus = Focus::start(Plan::default(), start);
        focus.advance(start + 5 * MINUTE, false);
        // 잠깐 멈춘 동안은 집중 시간도 흐르지 않고, 끝날 시각도 없다.
        assert_eq!(focus.pause(true, start + 6 * MINUTE, false), None);
        assert_eq!(focus.left(), 19 * MINUTE);
        assert_eq!(focus.advance(start + 60 * MINUTE, false), None);
        assert_eq!(focus.due(false), None);
        assert_eq!(focus.label(), "집중하는 중 (19분 남음, 잠깐 멈춤)");
        focus.pause(false, start + 60 * MINUTE, false);
        assert_eq!(focus.due(false), Some(start + 79 * MINUTE));
        // 지금 쉬기: 바로 쉬는 시간으로 넘어간다. 바로 집중: 다시 집중 시간으로.
        focus.skip(start + 61 * MINUTE);
        assert_eq!((focus.phase(), focus.left(), focus.total()), (Phase::Break, 5 * MINUTE, 5 * MINUTE));
        focus.pause(true, start + 62 * MINUTE, false);
        focus.skip(start + 63 * MINUTE);
        assert_eq!((focus.phase(), focus.left(), focus.is_paused()), (Phase::Focus, 25 * MINUTE, false));
        // 쉬는 시간도 잠깐 멈추면 흐르지 않는다(자리를 비워도 흐르던 것과 다르다).
        focus.skip(start + 63 * MINUTE);
        focus.pause(true, start + 64 * MINUTE, true);
        assert_eq!(focus.advance(start + 90 * MINUTE, true), None);
        assert_eq!(focus.left(), 4 * MINUTE);
    }

    #[test]
    fn going_on_by_itself_starts_the_next_focus() {
        let start = Instant::now();
        let mut focus = Focus::start(Plan { auto: true, ..Plan::default() }, start);
        assert_eq!(focus.advance(start + 25 * MINUTE, false), Some(Turn::BreakTime));
        assert_eq!(focus.advance(start + 30 * MINUTE, false), Some(Turn::Again));
        assert_eq!((focus.phase(), focus.left()), (Phase::Focus, 25 * MINUTE));
        // 도는 중에 끄면 이번 쉬는 시간이 끝날 때 타이머도 끝난다.
        focus.set_auto(false);
        focus.advance(start + 55 * MINUTE, false);
        assert_eq!(focus.advance(start + 60 * MINUTE, false), Some(Turn::BackToWork));
    }

    #[test]
    fn the_plan_steps_within_its_range() {
        let mut plan = Plan::default();
        plan.step(false, 1);
        plan.step(true, -1);
        assert_eq!((plan.focus, plan.rest), (30, 4));
        for _ in 0..40 {
            plan.step(false, 1);
            plan.step(true, -1);
        }
        assert_eq!((plan.focus, plan.rest), (120, 1));
        // 칸에 맞지 않는 값은 먼저 칸에 맞춘다.
        plan.focus = 27;
        plan.step(false, -1);
        assert_eq!(plan.focus, 25);
        plan.focus = 27;
        plan.step(false, 1);
        assert_eq!(plan.focus, 30);
        assert_eq!(Plan { focus: 500, rest: 0, auto: true }.fit(), Plan { focus: 120, rest: 1, auto: true });
    }
}
