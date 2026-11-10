//! 말풍선으로 알릴 것들의 줄.
//!
//! 말풍선은 한 번에 하나만 띄운다. 알릴 것이 여럿이면 줄을 세우고, 떠 있는 것이 닫히면 다음 것을 띄운다.
//! 줄에서 기다리는 동안 쓸모없어진 것(이미 끝난 방송의 시작 안내 같은 것)은 버린다.
//!
//! 창을 모른다. 무엇을 언제 띄우고 닫을지만 정하므로 단위 테스트로 확인할 수 있다.

use std::time::{Duration, Instant};

use crate::bubble::Speech;

/// 알릴 것의 종류. 먼저 띄울 것부터 차례로 적었다.
// 알림과 메모는 그 기능을 만들 때부터 쓴다.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// 사용자가 정해 둔 시각의 알림. 때를 놓치면 안 되므로 가장 먼저 띄운다.
    Reminder,
    /// 방송 안내.
    Live,
    /// 꽃핀이 들고 있는 메모.
    Memo,
    /// 그 밖의 안내.
    Note,
}

/// 말풍선을 클릭했을 때 할 일. 무엇을 하든 말풍선은 닫힌다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Click {
    Close,
    /// 주소를 기본 브라우저로 연다.
    Open(String),
}

/// 알릴 것 하나.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub kind: Kind,
    pub speech: Speech,
    /// 떠 있는 시간. 없으면 클릭할 때까지 떠 있다.
    pub lasts: Option<Duration>,
    pub click: Click,
    /// 나중에 버릴 때 가리키는 이름. 같은 이름을 가진 것이 함께 버려진다. 버릴 일이 없으면 비워 둔다.
    pub tag: Option<&'static str>,
}

/// 알릴 것들의 줄과, 지금 떠 있는 것.
#[derive(Default)]
pub struct Notices {
    /// 기다리는 것들. 들어온 차례대로다.
    waiting: Vec<Notice>,
    /// 떠 있는 것과, 저절로 닫힐 시각.
    showing: Option<(Notice, Option<Instant>)>,
}

impl Notices {
    /// 줄의 끝에 세운다.
    pub fn push(&mut self, notice: Notice) {
        self.waiting.push(notice);
    }

    /// 이름이 `tag`인 것이 줄에서 기다리고 있는가. 떠 있는 것은 치지 않는다.
    pub fn is_waiting(&self, tag: &'static str) -> bool {
        self.waiting.iter().any(|notice| notice.tag == Some(tag))
    }

    /// 이름이 `tag`인 것을 모두 버린다. 떠 있던 것을 버렸으면 `true`를 돌려준다(말풍선을 닫아야 한다).
    pub fn cancel(&mut self, tag: &'static str) -> bool {
        self.waiting.retain(|notice| notice.tag != Some(tag));
        let shown = self.showing.as_ref().is_some_and(|(notice, _)| notice.tag == Some(tag));
        if shown {
            self.showing = None;
        }
        shown
    }

    /// 지금 떠 있는 것.
    pub fn showing(&self) -> Option<&Notice> {
        self.showing.as_ref().map(|(notice, _)| notice)
    }

    /// 떠 있는 것이 저절로 닫힐 시각. 그때 [`Notices::is_over`]를 물어야 한다.
    pub fn closes_at(&self) -> Option<Instant> {
        self.showing.as_ref().and_then(|(_, at)| *at)
    }

    /// 떠 있는 것이 닫힐 때가 됐는가.
    pub fn is_over(&self, now: Instant) -> bool {
        self.closes_at().is_some_and(|at| now >= at)
    }

    /// 떠 있는 것을 닫고 돌려준다. 클릭했거나 떠 있을 시간이 다 됐을 때 부른다.
    pub fn dismiss(&mut self) -> Option<Notice> {
        self.showing.take().map(|(notice, _)| notice)
    }

    /// 떠 있는 것이 없고 띄워도 되는 때(`ready`)면, 줄에서 다음 것을 꺼내 띄운 것으로 적고 돌려준다.
    ///
    /// 종류가 앞선 것이 먼저고, 종류가 같으면 먼저 들어온 것이 먼저다. `ready`가 아니면(꽃핀이 떠 있거나
    /// 잡혀 있거나 숨어 있으면) 줄을 그대로 두고 기다린다.
    pub fn next(&mut self, now: Instant, ready: bool) -> Option<&Notice> {
        if self.showing.is_some() || !ready {
            return None;
        }
        // 종류가 같으면 앞에 있는 것이 고른다(min_by_key는 같을 때 먼저 나온 것을 고른다).
        let first = (0..self.waiting.len()).min_by_key(|&index| self.waiting[index].kind)?;
        let notice = self.waiting.remove(first);
        let closes = notice.lasts.map(|lasts| now + lasts);
        self.showing = Some((notice, closes));
        self.showing()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(kind: Kind, title: &str) -> Notice {
        let speech = Speech { title: title.to_string(), body: String::new() };
        Notice { kind, speech, lasts: None, click: Click::Close, tag: None }
    }

    fn title(notice: Option<&Notice>) -> Option<&str> {
        notice.map(|notice| notice.speech.title.as_str())
    }

    #[test]
    fn shows_one_at_a_time_in_order_of_kind_then_arrival() {
        let now = Instant::now();
        let mut notices = Notices::default();
        for (kind, name) in [
            (Kind::Memo, "메모"),
            (Kind::Live, "방송 1"),
            (Kind::Note, "안내"),
            (Kind::Reminder, "알림"),
            (Kind::Live, "방송 2"),
        ] {
            notices.push(notice(kind, name));
        }
        let mut shown = Vec::new();
        while let Some(name) = title(notices.next(now, true)).map(str::to_string) {
            // 떠 있는 동안에는 다음 것이 나오지 않는다.
            assert_eq!(title(notices.next(now, true)), None);
            assert_eq!(title(notices.showing()), Some(name.as_str()));
            shown.push(name);
            notices.dismiss();
        }
        assert_eq!(shown, ["알림", "방송 1", "방송 2", "메모", "안내"]);
        assert!(notices.showing().is_none() && notices.dismiss().is_none());
    }

    #[test]
    fn knows_what_is_still_waiting() {
        let now = Instant::now();
        let mut notices = Notices::default();
        notices.push(Notice { tag: Some("first"), ..notice(Kind::Note, "하나") });
        notices.push(Notice { tag: Some("second"), ..notice(Kind::Note, "둘") });
        assert!(notices.is_waiting("first") && notices.is_waiting("second") && !notices.is_waiting("third"));
        // 떠 있는 것은 기다리는 것이 아니다.
        notices.next(now, true);
        assert!(!notices.is_waiting("first") && notices.is_waiting("second"));
        notices.cancel("second");
        assert!(!notices.is_waiting("second"));
    }

    #[test]
    fn waits_while_it_cannot_be_shown() {
        let now = Instant::now();
        let mut notices = Notices::default();
        notices.push(notice(Kind::Live, "방송 시작!"));
        // 꽃핀이 떠 있거나 잡혀 있는 동안에는 띄우지 않고 줄에 둔다.
        assert_eq!(title(notices.next(now, false)), None);
        assert_eq!(title(notices.next(now, false)), None);
        assert_eq!(title(notices.next(now, true)), Some("방송 시작!"));
    }

    #[test]
    fn closes_by_itself_only_when_it_has_a_time_limit() {
        let now = Instant::now();
        let mut notices = Notices::default();
        notices.push(Notice { lasts: Some(Duration::from_secs(10)), ..notice(Kind::Live, "방송 끝") });
        notices.push(notice(Kind::Note, "클릭할 때까지"));
        notices.next(now, true);
        assert_eq!(notices.closes_at(), Some(now + Duration::from_secs(10)));
        assert!(!notices.is_over(now + Duration::from_secs(9)));
        assert!(notices.is_over(now + Duration::from_secs(10)));
        notices.dismiss();
        // 시간 제한이 없는 것은 얼마가 지나도 닫히지 않는다.
        let later = now + Duration::from_secs(20);
        notices.next(later, true);
        assert_eq!(notices.closes_at(), None);
        assert!(!notices.is_over(later + Duration::from_secs(3600)));
    }

    #[test]
    fn stale_ones_are_dropped_whether_waiting_or_showing() {
        let now = Instant::now();
        let mut notices = Notices::default();
        notices.push(Notice { tag: Some("live-start"), ..notice(Kind::Live, "방송 시작!") });
        notices.push(notice(Kind::Note, "안내"));
        // 줄에서 기다리는 동안 방송이 끝났다. 시작 안내는 띄우지 않는다.
        assert!(!notices.cancel("live-start"));
        assert_eq!(title(notices.next(now, true)), Some("안내"));
        notices.dismiss();
        // 떠 있는 것을 버리면 말풍선을 닫아야 한다고 알려 준다.
        notices.push(Notice { tag: Some("live-start"), ..notice(Kind::Live, "방송 시작!") });
        notices.next(now, true);
        assert!(notices.cancel("live-start"));
        assert!(notices.showing().is_none());
        // 없는 이름을 버리라고 해도 아무 일도 없다.
        assert!(!notices.cancel("live-start"));
    }
}
