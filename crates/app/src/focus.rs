//! 쉬는 시간 알림(집중 타이머). 정한 시간 동안 집중하면 쉬자고 알리고, 쉬는 시간이 끝나면 다시 알린다.
//!
//! 시간만 센다. 알리는 일(말풍선, 꽃핀들의 기지개)은 앱이 한다. 사용자가 자리를 비운 동안에는 집중 시간이
//! 흐르지 않는다. 쉬는 시간은 자리를 비워도 흐른다.

use std::time::{Duration, Instant};

/// 고를 수 있는 집중 시간과 쉬는 시간(분).
pub const CHOICES: [(u64, u64); 2] = [(25, 5), (50, 10)];

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
}

/// 돌고 있는 타이머.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Focus {
    /// [`CHOICES`]의 몇 번째인지.
    choice: usize,
    /// 집중 시간과 쉬는 시간.
    lengths: (Duration, Duration),
    phase: Phase,
    /// 지금 하는 것이 끝날 때까지 남은 시간.
    left: Duration,
    /// 마지막으로 시간을 흘린 시각.
    last: Instant,
}

impl Focus {
    /// [`CHOICES`]의 `choice`번째로 집중을 시작한다.
    pub fn start(choice: usize, now: Instant) -> Focus {
        let (focus, rest) = CHOICES[choice.min(CHOICES.len() - 1)];
        Focus::with(choice, (Duration::from_secs(focus * 60), Duration::from_secs(rest * 60)), now)
    }

    /// 시간을 직접 정해서 시작한다. 확인용으로 짧게 돌려 볼 때 쓴다.
    pub fn with(choice: usize, lengths: (Duration, Duration), now: Instant) -> Focus {
        Focus { choice, lengths, phase: Phase::Focus, left: lengths.0, last: now }
    }

    pub fn choice(&self) -> usize {
        self.choice
    }

    #[cfg(test)]
    pub fn phase(&self) -> Phase {
        self.phase
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
        if self.phase == Phase::Focus && away {
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
            Phase::Break => Some(Turn::BackToWork),
        }
    }

    /// 지금 하는 것이 끝날 시각. 자리를 비워 집중 시간이 멈춰 있으면 없다.
    pub fn due(&self, away: bool) -> Option<Instant> {
        (self.phase == Phase::Break || !away).then(|| self.last + self.left)
    }

    /// 메뉴에 보여 줄 지금의 상태. 남은 시간은 분으로 올려 센다.
    pub fn label(&self) -> String {
        let minutes = self.left.as_secs().div_ceil(60);
        match self.phase {
            Phase::Focus => format!("집중하는 중 ({minutes}분 남음)"),
            Phase::Break => format!("쉬는 중 ({minutes}분 남음)"),
        }
    }
}

/// 메뉴에 보여 줄 고르는 줄의 이름.
pub fn choice_label(choice: usize) -> String {
    let (focus, rest) = CHOICES[choice];
    format!("{focus}분 집중 + {rest}분 쉬기")
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: Duration = Duration::from_secs(60);

    #[test]
    fn rests_after_focusing_then_ends() {
        let start = Instant::now();
        let mut focus = Focus::start(0, start);
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
        let mut focus = Focus::start(1, start);
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
        let mut focus = Focus::with(0, (Duration::from_secs(10), Duration::from_secs(5)), start);
        // 절전에서 돌아와 한참 늦게 깼다. 한 번에 하나씩만 넘어간다.
        assert_eq!(focus.advance(start + MINUTE, false), Some(Turn::BreakTime));
        assert_eq!(focus.left(), Duration::from_secs(5));
        assert_eq!(choice_label(1), "50분 집중 + 10분 쉬기");
    }
}
