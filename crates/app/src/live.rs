//! 꽃핀 방송이 켜져 있는지를 판단한다.
//!
//! 치지직에서 받은 응답을 읽고, 앞서 알던 것과 견줘 "방송이 시작됐다", "끝났다"를 알아낸다.
//! 인터넷을 직접 쓰지 않는다(그 일은 [`crate::checker`]가 한다). 그래서 단위 테스트로 확인할 수 있다.
//!
//! 묻는 주소는 치지직이 공식으로 공개한 API가 아니라 치지직 웹이 쓰는 주소다. 예고 없이 바뀔 수 있으므로,
//! 응답이 알던 모양이 아니면 더 묻지 않고 멈춘다.

use std::time::Duration;

use serde_json::Value;

/// 꽃핀의 치지직 채널.
pub const CHANNEL_ID: &str = "a86031fdfeb1b223ad1172cf6ba4b94e";
/// 방송 상태를 묻는 곳.
pub const HOST: &str = "api.chzzk.naver.com";

/// 방송 상태를 묻는 주소의 경로.
pub fn path() -> String {
    format!("/polling/v2/channels/{CHANNEL_ID}/live-status")
}

/// 확인에 실패했을 때 다시 물을 때까지 기다리는 시간(분). 거듭 실패할수록 길어지고, 마지막 값에서 멈춘다.
const RETRY_MINUTES: [u64; 5] = [1, 2, 4, 8, 10];
/// 방송 시작 시각이 적힌 시간대. 응답에 시간대가 없어서 한국 시간으로 본다.
const KST_OFFSET: i64 = 9 * 3600;

/// 켜져 있는 방송.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Live {
    pub title: String,
    pub category: String,
    pub viewers: u64,
    /// 방송을 시작한 시각. 응답에 적힌 글 그대로다(예: "2026-11-09 16:11:06"). 같은 방송인지 가리는 데도 쓴다.
    pub opened: String,
}

/// 한 번 물어서 알아낸 것.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    Open(Live),
    Closed,
    /// 응답은 왔는데 알던 모양이 아니다. 주소가 바뀐 것으로 본다.
    Moved,
    /// 치지직이 받아 주지 않았다(로그인하라고 하거나, 막았거나, 너무 자주 묻는다고 했다).
    Refused,
    /// 연결하지 못했거나, 응답이 깨졌거나, 다른 채널의 것이다.
    Failed,
}

/// HTTP 상태 번호와 응답 본문으로 판단한다.
pub fn judge(status: u32, body: &str) -> Reply {
    match status {
        200 => read(body),
        401 | 403 | 429 => Reply::Refused,
        // 치지직 쪽의 일시적인 문제다. 나중에 다시 묻는다.
        500..=599 => Reply::Failed,
        // 주소가 없어졌거나 다른 곳으로 넘겼다. 넘긴 곳을 따라가지 않는다.
        _ => Reply::Moved,
    }
}

/// 응답 본문을 읽는다.
pub fn read(body: &str) -> Reply {
    let Ok(value) = serde_json::from_str::<Value>(body) else { return Reply::Failed };
    // 치지직은 문제가 있으면 code에 200이 아닌 값을 넣어 돌려준다. 옛 주소가 그랬다.
    if value.get("code").and_then(Value::as_i64) != Some(200) {
        return Reply::Moved;
    }
    let Some(content) = value.get("content").filter(|content| content.is_object()) else { return Reply::Moved };
    let text = |key: &str| content.get(key).and_then(Value::as_str).unwrap_or("").trim().to_string();
    if text("channelId") != CHANNEL_ID {
        return Reply::Failed;
    }
    match text("status").as_str() {
        "OPEN" => Reply::Open(Live {
            title: text("liveTitle"),
            category: text("liveCategoryValue"),
            viewers: content.get("concurrentUserCount").and_then(Value::as_u64).unwrap_or(0),
            opened: text("openDate"),
        }),
        "CLOSE" => Reply::Closed,
        _ => Reply::Moved,
    }
}

/// 방송에 대해 지금 아는 것. 설정 창과 메뉴에 보여 준다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum View {
    /// 아직 한 번도 확인하지 못했다.
    Unknown,
    Open(Live),
    Closed,
    /// 마지막 확인에 실패했다.
    Failed,
    /// 주소가 바뀌어 확인을 멈췄다. 새 버전이 필요하다.
    Moved,
    /// 치지직이 받아 주지 않아 확인을 멈췄다.
    Refused,
}

/// 알릴 일.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Started(Live),
    Ended,
    /// 앱을 켜고 처음 확인했더니 이미 방송 중이다.
    AlreadyLive(Live),
}

/// 응답을 차례로 받아, 알릴 일이 생겼는지 가려낸다.
pub struct Tracker {
    view: View,
    /// 마지막으로 확인에 성공했을 때 방송이 켜져 있었는가. 한 번도 성공하지 못했으면 `None`이다.
    was_open: Option<bool>,
    /// 마지막으로 안내한 방송의 시작 시각. 같은 방송을 두 번 안내하지 않는 데 쓴다.
    announced: Option<String>,
    /// 연달아 실패한 횟수.
    failures: usize,
}

impl Tracker {
    /// `announced`는 지난번에 앱을 쓸 때 마지막으로 안내한 방송의 시작 시각이다.
    pub fn new(announced: Option<String>) -> Tracker {
        Tracker { view: View::Unknown, was_open: None, announced, failures: 0 }
    }

    pub fn view(&self) -> &View {
        &self.view
    }

    /// 지금 켜져 있는 방송. 마지막 확인이 실패했으면 모르는 것으로 친다.
    pub fn live(&self) -> Option<&Live> {
        if let View::Open(live) = &self.view { Some(live) } else { None }
    }

    /// 마지막으로 안내한 방송의 시작 시각. 앱을 다시 켰을 때 쓰도록 저장해 둔다.
    pub fn announced(&self) -> Option<&str> {
        self.announced.as_deref()
    }

    /// 응답 하나를 받는다. 알릴 일이 생겼으면 돌려준다.
    pub fn feed(&mut self, reply: Reply) -> Option<Event> {
        match reply {
            Reply::Open(live) => {
                self.failures = 0;
                let event = if self.announced.as_deref() == Some(live.opened.as_str()) {
                    // 이미 안내한 방송이다. 앱을 다시 켰거나, 인터넷이 끊겼다 돌아온 것이다.
                    None
                } else if self.was_open.is_none() {
                    Some(Event::AlreadyLive(live.clone()))
                } else {
                    Some(Event::Started(live.clone()))
                };
                if event.is_some() {
                    self.announced = Some(live.opened.clone());
                }
                self.was_open = Some(true);
                self.view = View::Open(live);
                event
            }
            Reply::Closed => {
                self.failures = 0;
                let ended = self.was_open == Some(true);
                self.was_open = Some(false);
                self.view = View::Closed;
                ended.then_some(Event::Ended)
            }
            Reply::Failed => {
                // 마지막으로 알던 것은 그대로 둔다. 끊겼다 돌아왔을 때 방송이 끝나 있으면 그때 끝났다고 알린다.
                self.failures += 1;
                self.view = View::Failed;
                None
            }
            Reply::Moved => {
                self.view = View::Moved;
                None
            }
            Reply::Refused => {
                self.view = View::Refused;
                None
            }
        }
    }

    /// 다음에 물을 때까지 기다릴 시간. 주소가 바뀌었거나 치지직이 받아 주지 않았으면 더 묻지 않는다(`None`).
    /// 앱을 다시 켜면 한 번 더 물어본다.
    ///
    /// 보통은 사용자가 고른 간격이다. 실패하면 1분 뒤에 다시 묻고, 거듭 실패하면 10분까지 늘린다.
    pub fn wait(&self, interval: Duration) -> Option<Duration> {
        match self.view {
            View::Moved | View::Refused => None,
            View::Failed => {
                let step = self.failures.clamp(1, RETRY_MINUTES.len()) - 1;
                Some(Duration::from_secs(RETRY_MINUTES[step] * 60))
            }
            _ => Some(interval),
        }
    }
}

/// "2026-11-09 16:11:06"처럼 적힌 한국 시각을 1970년부터의 초로 바꾼다.
fn unix_seconds(text: &str) -> Option<i64> {
    let (date, time) = text.trim().split_once(' ')?;
    let mut date = date.split('-').map(|part| part.parse::<i64>().ok());
    let mut time = time.split(':').map(|part| part.parse::<i64>().ok());
    let (year, month, day) = (date.next()??, date.next()??, date.next()??);
    let (hour, minute, second) = (time.next()??, time.next()??, time.next()??);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    // 3월을 한 해의 첫 달로 놓고 날수를 센다(윤년의 2월 29일이 해의 맨 끝에 온다).
    let (year, month) = if month <= 2 { (year - 1, month + 9) } else { (year, month - 3) };
    let era_days = year * 365 + year.div_euclid(4) - year.div_euclid(100) + year.div_euclid(400);
    let days = era_days + (153 * month + 2) / 5 + day - 1 - 719_468;
    Some(days * 86_400 + hour * 3600 + minute * 60 + second - KST_OFFSET)
}

/// 방송을 시작한 지 얼마나 됐는지. "13분째", "2시간 13분째"처럼 적는다. 시각을 읽지 못하면 `None`이다.
///
/// `now`는 1970년부터의 초(UTC)다.
pub fn running_for(opened: &str, now: i64) -> Option<String> {
    let minutes = (now - unix_seconds(opened)?).max(0) / 60;
    Some(if minutes < 60 {
        format!("{minutes}분째")
    } else {
        format!("{}시간 {}분째", minutes / 60, minutes % 60)
    })
}

/// 설정 창 방송 탭의 상태 줄.
///
/// `enabled`는 방송 상태 확인이 켜져 있는지, `checked`는 마지막으로 확인한 시각(시, 분)이다.
pub fn status_line(view: &View, enabled: bool, checked: Option<(u32, u32)>) -> String {
    if !enabled {
        return "방송 상태 확인이 꺼져 있습니다. 인터넷을 쓰지 않습니다.".to_string();
    }
    let when = checked.map_or(String::new(), |(hour, minute)| {
        let half = if hour < 12 { "오전" } else { "오후" };
        format!(" · 마지막 확인 {half} {}:{minute:02}", if hour % 12 == 0 { 12 } else { hour % 12 })
    });
    match view {
        View::Unknown => "지금: 확인하는 중입니다".to_string(),
        View::Open(live) => format!("지금: 방송 중 · {} · 시청자 {}명", live.title, with_commas(live.viewers)),
        View::Closed => format!("지금: 방송 꺼짐{when}"),
        View::Failed => format!("지금: 확인하지 못했습니다{when}"),
        View::Moved => "새 버전이 필요합니다. 치지직의 주소가 바뀌어 확인을 멈췄습니다.".to_string(),
        View::Refused => "치지직이 받아 주지 않아 확인을 멈췄습니다.".to_string(),
    }
}

/// 트레이 아이콘에 마우스를 올리면 보이는 글.
pub fn tip(live: &Live) -> String {
    format!("방송 중: {} · 시청자 {}명", shorten(&live.title, 60), with_commas(live.viewers))
}

/// 글이 `most`자보다 길면 줄이고 끝에 "…"를 붙인다.
pub fn shorten(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_string();
    }
    let kept: String = text.chars().take(most.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

/// 세 자리마다 쉼표를 넣은 수.
pub fn with_commas(number: u64) -> String {
    let digits = number.to_string();
    let mut text = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            text.push(',');
        }
        text.push(digit);
    }
    text
}

/// 방송 정보 한 줄. "종합 게임 · 시청자 2,226명 · 2시간 13분째"처럼 적는다.
pub fn details(live: &Live, now: i64) -> String {
    let mut parts = Vec::new();
    if !live.category.is_empty() {
        parts.push(live.category.clone());
    }
    parts.push(format!("시청자 {}명", with_commas(live.viewers)));
    parts.extend(running_for(&live.opened, now));
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 치지직이 돌려주는 것과 같은 모양의 응답. 쓰지 않는 값은 몇 개만 넣었다.
    fn body(status: &str, title: &str, viewers: u64, opened: &str) -> String {
        format!(
            r#"{{"code":200,"message":null,"content":{{"liveTitle":"{title}","status":"{status}",
            "concurrentUserCount":{viewers},"openDate":"{opened}","closeDate":null,"chatChannelId":"N2rEkm",
            "categoryType":"GAME","liveCategory":"various_games","liveCategoryValue":"종합 게임",
            "livePollingStatusJson":"{{\"callPeriodMilliSecond\":10000}}","channelId":"{CHANNEL_ID}"}}}}"#
        )
    }

    fn live(opened: &str) -> Live {
        Live {
            title: "꽃핀 방송".to_string(),
            category: "종합 게임".to_string(),
            viewers: 2229,
            opened: opened.to_string(),
        }
    }

    const FIRST: &str = "2026-11-09 16:11:06";
    const SECOND: &str = "2026-11-10 08:59:10";

    #[test]
    fn reads_an_open_and_a_closed_reply() {
        assert_eq!(read(&body("OPEN", " 꽃핀 방송 ", 2229, FIRST)), Reply::Open(live(FIRST)));
        // 꺼져 있을 때도 마지막 방송의 제목과 시청자 수가 남아 있다. 쓰지 않는다.
        assert_eq!(read(&body("CLOSE", "꽃핀 방송", 2229, FIRST)), Reply::Closed);
    }

    #[test]
    fn a_reply_in_an_unknown_shape_means_the_address_moved() {
        // 옛 주소가 돌려주던 것.
        let old = r#"{"code":9004,"message":"앱 업데이트 후에 정상 시청 가능합니다.","content":null}"#;
        assert_eq!(read(old), Reply::Moved);
        assert_eq!(read(r#"{"code":200,"content":null}"#), Reply::Moved);
        assert_eq!(read(&body("PAUSED", "꽃핀 방송", 1, FIRST)), Reply::Moved);
    }

    #[test]
    fn a_broken_or_foreign_reply_is_a_failure() {
        for text in ["", "<html>잠시 후 다시 시도해 주세요</html>", "{\"code\":200"] {
            assert_eq!(read(text), Reply::Failed, "{text}");
        }
        // 다른 채널의 응답은 쓰지 않는다.
        let other = body("OPEN", "다른 방송", 5, FIRST).replace(CHANNEL_ID, "80b36a0a00000000000000000000beef");
        assert_eq!(read(&other), Reply::Failed);
    }

    #[test]
    fn the_http_status_decides_before_the_body() {
        let open = body("OPEN", "꽃핀 방송", 2229, FIRST);
        assert_eq!(judge(200, &open), Reply::Open(live(FIRST)));
        // 받아 주지 않으면, 본문이 무엇이든 멈춘다.
        for status in [401, 403, 429] {
            assert_eq!(judge(status, &open), Reply::Refused, "{status}");
        }
        // 치지직 쪽의 일시적인 문제는 실패로 치고 나중에 다시 묻는다.
        for status in [500, 502, 503] {
            assert_eq!(judge(status, ""), Reply::Failed, "{status}");
        }
        // 주소가 없어졌거나 다른 곳으로 넘겼다.
        for status in [204, 301, 302, 400, 404, 410] {
            assert_eq!(judge(status, &open), Reply::Moved, "{status}");
        }
    }

    #[test]
    fn stops_asking_when_refused() {
        let minute = Duration::from_secs(60);
        let mut tracker = Tracker::new(None);
        tracker.feed(Reply::Open(live(FIRST)));
        assert_eq!(tracker.feed(Reply::Refused), None);
        assert_eq!((tracker.view(), tracker.live(), tracker.wait(minute)), (&View::Refused, None, None));
        assert!(status_line(tracker.view(), true, Some((15, 12))).contains("받아 주지 않아"));
    }

    #[test]
    fn tells_when_a_stream_starts_and_ends() {
        let mut tracker = Tracker::new(None);
        assert_eq!(tracker.view(), &View::Unknown);
        // 켰을 때 꺼져 있었다. 알릴 것이 없다.
        assert_eq!(tracker.feed(Reply::Closed), None);
        assert_eq!(tracker.feed(Reply::Closed), None);
        // 켜졌다. 한 번만 알린다.
        assert_eq!(tracker.feed(Reply::Open(live(FIRST))), Some(Event::Started(live(FIRST))));
        assert_eq!(tracker.feed(Reply::Open(live(FIRST))), None);
        assert_eq!((tracker.live(), tracker.announced()), (Some(&live(FIRST)), Some(FIRST)));
        // 꺼졌다. 한 번만 알린다.
        assert_eq!(tracker.feed(Reply::Closed), Some(Event::Ended));
        assert_eq!(tracker.feed(Reply::Closed), None);
        assert_eq!((tracker.view(), tracker.live()), (&View::Closed, None));
        // 다음 방송.
        assert_eq!(tracker.feed(Reply::Open(live(SECOND))), Some(Event::Started(live(SECOND))));
    }

    #[test]
    fn already_live_at_startup_is_told_once_and_not_again_after_a_restart() {
        let mut tracker = Tracker::new(None);
        assert_eq!(tracker.feed(Reply::Open(live(FIRST))), Some(Event::AlreadyLive(live(FIRST))));
        assert_eq!(tracker.feed(Reply::Open(live(FIRST))), None);
        // 앱을 껐다 켰다. 같은 방송은 다시 안내하지 않는다. 끝나면 끝났다고는 알린다.
        let mut again = Tracker::new(tracker.announced().map(str::to_string));
        assert_eq!(again.feed(Reply::Open(live(FIRST))), None);
        assert_eq!(again.live(), Some(&live(FIRST)));
        assert_eq!(again.feed(Reply::Closed), Some(Event::Ended));
        // 앱이 꺼져 있던 사이에 시작한 다른 방송은 안내한다.
        let mut later = Tracker::new(Some(FIRST.to_string()));
        assert_eq!(later.feed(Reply::Open(live(SECOND))), Some(Event::AlreadyLive(live(SECOND))));
    }

    #[test]
    fn a_dropped_connection_does_not_cause_a_second_notice() {
        let mut tracker = Tracker::new(None);
        tracker.feed(Reply::Closed);
        tracker.feed(Reply::Open(live(FIRST)));
        // 인터넷이 끊겼다 돌아왔다. 같은 방송이면 다시 알리지 않는다.
        assert_eq!(tracker.feed(Reply::Failed), None);
        assert_eq!((tracker.view(), tracker.live()), (&View::Failed, None));
        assert_eq!(tracker.feed(Reply::Open(live(FIRST))), None);
        // 끊긴 사이에 방송이 끝났으면, 돌아왔을 때 끝났다고 알린다.
        tracker.feed(Reply::Failed);
        assert_eq!(tracker.feed(Reply::Closed), Some(Event::Ended));
        // 끊긴 사이에 끝났다가 새로 시작했으면 새 방송으로 알린다.
        tracker.feed(Reply::Open(live(FIRST)));
        tracker.feed(Reply::Failed);
        assert_eq!(tracker.feed(Reply::Open(live(SECOND))), Some(Event::Started(live(SECOND))));
    }

    #[test]
    fn waits_longer_after_repeated_failures_and_stops_when_moved() {
        let minute = Duration::from_secs(60);
        let mut tracker = Tracker::new(None);
        assert_eq!(tracker.wait(3 * minute), Some(3 * minute));
        let mut waits = Vec::new();
        for _ in 0..7 {
            tracker.feed(Reply::Failed);
            waits.push(tracker.wait(3 * minute).unwrap().as_secs() / 60);
        }
        assert_eq!(waits, [1, 2, 4, 8, 10, 10, 10]);
        // 한 번 성공하면 원래 간격으로 돌아가고, 다시 실패하면 1분부터다.
        tracker.feed(Reply::Closed);
        assert_eq!(tracker.wait(3 * minute), Some(3 * minute));
        tracker.feed(Reply::Failed);
        assert_eq!(tracker.wait(3 * minute), Some(minute));
        // 주소가 바뀌었으면 더 묻지 않는다.
        tracker.feed(Reply::Moved);
        assert_eq!((tracker.view(), tracker.wait(3 * minute)), (&View::Moved, None));
    }

    #[test]
    fn start_time_is_read_as_korean_time() {
        // 2026-11-09 16:11:06 한국 시간은 07:11:06 UTC다.
        assert_eq!(unix_seconds(FIRST), Some(1_794_208_266));
        assert_eq!(unix_seconds("1970-01-01 09:00:00"), Some(0));
        // 윤년의 2월 29일과 그다음 날.
        assert_eq!(unix_seconds("2028-03-01 00:00:00").unwrap() - unix_seconds("2028-02-29 00:00:00").unwrap(), 86_400);
        assert_eq!(unix_seconds("2027-03-01 00:00:00").unwrap() - unix_seconds("2027-02-28 00:00:00").unwrap(), 86_400);
        for text in ["", "2026-11-09", "2026-13-09 16:11:06", "어제 16:11:06", "2026-11-09 25:00:00"] {
            assert_eq!(unix_seconds(text), None, "{text}");
        }
    }

    #[test]
    fn status_line_says_what_is_known() {
        let line = |view: &View, checked| status_line(view, true, checked);
        assert_eq!(line(&View::Unknown, None), "지금: 확인하는 중입니다");
        assert_eq!(line(&View::Open(live(FIRST)), Some((15, 12))), "지금: 방송 중 · 꽃핀 방송 · 시청자 2,229명");
        assert_eq!(line(&View::Closed, Some((15, 12))), "지금: 방송 꺼짐 · 마지막 확인 오후 3:12");
        assert_eq!(line(&View::Closed, Some((0, 5))), "지금: 방송 꺼짐 · 마지막 확인 오전 12:05");
        assert_eq!(line(&View::Failed, Some((12, 0))), "지금: 확인하지 못했습니다 · 마지막 확인 오후 12:00");
        assert!(line(&View::Moved, Some((9, 0))).starts_with("새 버전이 필요합니다."));
        // 꺼 두었으면 무엇을 알고 있든 꺼져 있다고만 말한다.
        assert!(status_line(&View::Open(live(FIRST)), false, None).contains("꺼져 있습니다"));
    }

    #[test]
    fn long_titles_are_shortened() {
        assert_eq!(shorten("짧은 제목", 10), "짧은 제목");
        assert_eq!(shorten("열 글자짜리 제목이다", 11), "열 글자짜리 제목이다");
        assert_eq!(shorten("가나다라마바사아자차카", 10), "가나다라마바사아자…");
        // 자른 자리가 빈칸이면 빈칸은 남기지 않는다.
        assert_eq!(shorten("열 글자가 넘는 제목입니다", 10), "열 글자가 넘는…");
        assert_eq!(shorten("하나 둘 셋 넷", 4), "하나…");
        assert_eq!(tip(&live(FIRST)), "방송 중: 꽃핀 방송 · 시청자 2,229명");
    }

    #[test]
    fn details_line_reads_naturally() {
        let started = unix_seconds(FIRST).unwrap();
        assert_eq!(running_for(FIRST, started + 13 * 60 + 59).as_deref(), Some("13분째"));
        assert_eq!(running_for(FIRST, started + 2 * 3600 + 13 * 60).as_deref(), Some("2시간 13분째"));
        // PC 시계가 조금 늦어도 음수가 되지 않는다.
        assert_eq!(running_for(FIRST, started - 30).as_deref(), Some("0분째"));
        assert_eq!(
            (with_commas(7), with_commas(999), with_commas(2226), with_commas(1234567)),
            ("7".to_string(), "999".to_string(), "2,226".to_string(), "1,234,567".to_string())
        );
        assert_eq!(details(&live(FIRST), started + 2 * 3600 + 13 * 60), "종합 게임 · 시청자 2,229명 · 2시간 13분째");
        // 시작 시각을 읽지 못하면 그 부분만 빠진다.
        assert_eq!(details(&Live { category: String::new(), ..live("?") }, 0), "시청자 2,229명");
    }
}
