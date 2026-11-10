//! 메모와 알림.
//!
//! 메모에 시각을 붙이면 알림이 된다. 한 번 알리는 것과 날마다 알리는 것이 있다.
//!
//! 창도 파일도 모른다. 메모를 고치고, 알릴 때가 된 것을 가려내고, 파일에 적을 글로 바꾸는 일만 하므로
//! 단위 테스트로 확인할 수 있다. 지금 시각은 늘 밖에서 받는다.

use serde_json::Value;

use crate::when::{When, clock_label, parse_clock};

const VERSION: i64 = 1;
/// 메모 하나에 적을 수 있는 글자 수.
pub const TEXT_LIMIT: usize = 300;
/// 둘 수 있는 메모의 수.
pub const COUNT_LIMIT: usize = 50;
/// "조금 뒤에 다시"를 고르면 이만큼 뒤에 다시 알린다(분).
pub const SNOOZE_MINUTES: i64 = 5;
/// 알릴 시각을 이만큼 넘겨서야 알아챘으면 놓친 알림으로 친다(분). 앱이나 PC가 꺼져 있던 경우다.
const MISSED_AFTER: i64 = 5;

/// 메모에 붙인 알림.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Remind {
    None,
    /// 정한 날, 정한 시각에 한 번.
    Once(When),
    /// 날마다 이 시각에.
    Daily {
        hour: u32,
        minute: u32,
    },
}

/// 메모가 포스트잇으로 화면에 붙어 있는 모습.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sheet {
    /// 기울이지 않았을 때 종이의 왼쪽 위가 놓이는 화면 좌표(px). 아직 붙인 적이 없으면 `None`이다.
    pub at: Option<(i32, i32)>,
    /// 종이의 크기. 화면 배율이 100%일 때의 픽셀이다.
    pub size: (u32, u32),
    /// 종이의 색(0xRRGGBB).
    pub color: u32,
    /// 붙어 있을 때의 기울기. 10분의 1도이고, 시계 방향이 양수다.
    pub tilt: i32,
    /// 다른 창들 위에 늘 떠 있는가.
    pub top: bool,
}

impl Sheet {
    /// 종이의 가장 작은 크기와 가장 큰 크기.
    pub const SMALLEST: (u32, u32) = (150, 120);
    pub const LARGEST: (u32, u32) = (420, 420);
}

impl Default for Sheet {
    fn default() -> Sheet {
        Sheet { at: None, size: (196, 196), color: 0xFFF3A6, tilt: 0, top: false }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Memo {
    pub id: u64,
    pub text: String,
    /// 만든 시각.
    pub created: When,
    pub remind: Remind,
    /// 꽃핀이 들고 있는가. 들고 있는 메모는 꽃핀을 클릭하면 말풍선으로 보여 준다.
    pub held: bool,
    /// 다음에 알릴 시각. 알릴 일이 없거나 이미 알렸으면 `None`이다.
    pub next: Option<When>,
    pub sheet: Sheet,
}

impl Memo {
    /// 목록에 보이는 한 줄. 글의 첫 줄이다.
    pub fn title(&self) -> String {
        let line = self.text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("");
        if line.is_empty() { "(빈 메모)".to_string() } else { line.to_string() }
    }

    /// 알림을 바꾸고, 다음에 알릴 시각을 다시 정한다.
    pub fn set_remind(&mut self, remind: Remind, now: When) {
        self.remind = remind;
        self.next = upcoming(remind, now);
    }

    /// 알림이 어떻게 되어 있는지를 한 줄로 적는다. 메모 창에 보여 준다.
    pub fn remind_line(&self) -> String {
        match (self.remind, self.next) {
            (Remind::None, _) => String::new(),
            (_, Some(next)) => format!("다음 알림: {}", next.label()),
            (Remind::Once(_), None) => "지난 시각입니다. 알리지 않습니다.".to_string(),
            (Remind::Daily { hour, minute }, None) => format!("날마다 {}", clock_label(hour, minute)),
        }
    }
}

/// `now` 뒤에 처음으로 알릴 시각. 한 번 알리는 것이 이미 지났으면 `None`이다.
fn upcoming(remind: Remind, now: When) -> Option<When> {
    match remind {
        Remind::None => None,
        Remind::Once(at) => (at > now).then_some(at),
        Remind::Daily { hour, minute } => Some(now.next_at(hour, minute)),
    }
}

/// 알릴 때가 된 메모들.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Due {
    /// 제때 알리는 것.
    pub on_time: Vec<Memo>,
    /// 알릴 시각을 한참 넘긴 것. 앱이 꺼져 있던 동안 지나간 알림이다.
    pub missed: Vec<Memo>,
}

/// 메모 모두. 새로 만든 것이 앞에 온다.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Memos {
    list: Vec<Memo>,
}

impl Memos {
    pub fn list(&self) -> &[Memo] {
        &self.list
    }

    pub fn get(&self, id: u64) -> Option<&Memo> {
        self.list.iter().find(|memo| memo.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Memo> {
        self.list.iter_mut().find(|memo| memo.id == id)
    }

    /// 빈 메모를 맨 앞에 만들고 그 번호를 돌려준다. 더 둘 수 없으면 `None`이다.
    pub fn add(&mut self, now: When) -> Option<u64> {
        if self.list.len() >= COUNT_LIMIT {
            return None;
        }
        let id = self.list.iter().map(|memo| memo.id).max().unwrap_or(0) + 1;
        let memo = Memo {
            id,
            text: String::new(),
            created: now,
            remind: Remind::None,
            held: false,
            next: None,
            sheet: Sheet::default(),
        };
        self.list.insert(0, memo);
        Some(id)
    }

    /// 메모를 지운다. 있던 것을 지웠으면 `true`다.
    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.list.len();
        self.list.retain(|memo| memo.id != id);
        self.list.len() != before
    }

    /// 꽃핀이 들고 있는 메모들. 글이 빈 것은 뺀다.
    pub fn held(&self) -> impl Iterator<Item = &Memo> {
        self.list.iter().filter(|memo| memo.held && !memo.text.trim().is_empty())
    }

    /// 알릴 것이 남아 있는가. 없으면 시각을 볼 까닭이 없다.
    pub fn has_pending(&self) -> bool {
        self.list.iter().any(|memo| memo.next.is_some())
    }

    /// 파일에서 읽은 뒤에 한 번 부른다. 날마다 알리는 메모인데 다음 시각이 빠져 있으면 채운다.
    pub fn settle(&mut self, now: When) {
        for memo in &mut self.list {
            if let (Remind::Daily { .. }, None) = (memo.remind, memo.next) {
                memo.next = upcoming(memo.remind, now);
            }
        }
    }

    /// 알릴 때가 된 메모를 꺼내고, 그 메모들의 다음 알릴 시각을 정한다.
    ///
    /// 돌려주는 것은 그때의 메모를 베낀 것이다. 꺼낸 것이 하나라도 있으면 저장해야 한다.
    pub fn take_due(&mut self, now: When) -> Due {
        let mut due = Due::default();
        for memo in &mut self.list {
            let Some(at) = memo.next.filter(|&at| at <= now) else { continue };
            let late = now.minutes_since(at) >= MISSED_AFTER;
            if late { &mut due.missed } else { &mut due.on_time }.push(memo.clone());
            memo.next = match memo.remind {
                Remind::Daily { hour, minute } => Some(now.next_at(hour, minute)),
                _ => None,
            };
        }
        due
    }

    /// 조금 뒤에 다시 알리게 한다. 그런 메모가 있으면 `true`다.
    pub fn snooze(&mut self, id: u64, now: When) -> bool {
        let memo = self.get_mut(id);
        memo.map(|memo| memo.next = Some(now.plus_minutes(SNOOZE_MINUTES))).is_some()
    }

    /// 파일에 적을 글을 만든다. 사람이 열어 봐도 읽기 쉽게, 번호와 글이 먼저 오도록 직접 적는다.
    pub fn to_json(&self) -> String {
        let mut text = format!("{{\n  \"version\": {VERSION},\n  \"memos\": [");
        for (index, memo) in self.list.iter().enumerate() {
            let remind = match memo.remind {
                Remind::None => "{ \"kind\": \"none\" }".to_string(),
                Remind::Once(at) => format!("{{ \"kind\": \"once\", \"at\": \"{}\" }}", at.text()),
                Remind::Daily { hour, minute } => {
                    format!("{{ \"kind\": \"daily\", \"time\": \"{hour:02}:{minute:02}\" }}")
                }
            };
            let next = memo.next.map_or("null".to_string(), |next| format!("\"{}\"", next.text()));
            let lines = [
                format!("\"id\": {}", memo.id),
                // 사용자가 적은 글이다. 따옴표나 줄바꿈이 섞여 있어도 파일이 깨지지 않게 적는다.
                format!("\"text\": {}", Value::String(memo.text.clone())),
                format!("\"created\": \"{}\"", memo.created.text()),
                format!("\"remind\": {remind}"),
                format!("\"held\": {}", memo.held),
                format!("\"next\": {next}"),
                format!("\"sheet\": {}", sheet_json(&memo.sheet)),
            ];
            let comma = if index == 0 { "" } else { "," };
            text.push_str(&format!("{comma}\n    {{\n      {}\n    }}", lines.join(",\n      ")));
        }
        text.push_str(if self.list.is_empty() { "]\n}\n" } else { "\n  ]\n}\n" });
        text
    }

    /// 파일의 글을 읽는다. JSON이 아니거나 메모 목록이 없으면 `None`이다.
    ///
    /// 읽을 수 없는 메모는 건너뛰고 나머지는 살린다. 번호가 겹치면 뒤의 것을 버린다.
    pub fn from_json(text: &str) -> Option<Memos> {
        let value: Value = serde_json::from_str(text).ok()?;
        let mut list: Vec<Memo> = Vec::new();
        for entry in value.get("memos")?.as_array()? {
            let Some(memo) = read_memo(entry) else { continue };
            if list.len() < COUNT_LIMIT && list.iter().all(|other| other.id != memo.id) {
                list.push(memo);
            }
        }
        Some(Memos { list })
    }
}

/// 포스트잇의 모습을 한 줄로 적는다. 아직 붙인 적이 없으면 자리는 적지 않는다.
fn sheet_json(sheet: &Sheet) -> String {
    let at = sheet.at.map_or(String::new(), |(x, y)| format!("\"x\": {x}, \"y\": {y}, "));
    let (tenths, sign) = (sheet.tilt.abs(), if sheet.tilt < 0 { "-" } else { "" });
    format!(
        "{{ {at}\"w\": {}, \"h\": {}, \"color\": \"#{:06X}\", \"tilt\": {sign}{}.{}, \"top\": {} }}",
        sheet.size.0,
        sheet.size.1,
        sheet.color & 0xFF_FFFF,
        tenths / 10,
        tenths % 10,
        sheet.top
    )
}

/// 포스트잇의 모습을 읽는다. 없거나 읽을 수 없는 값은 처음 값으로 둔다. 적어 둔 글이 더 중요하다.
fn read_sheet(entry: Option<&Value>) -> Sheet {
    let mut sheet = Sheet::default();
    let Some(entry) = entry else { return sheet };
    let whole = |key: &str| entry.get(key).and_then(Value::as_i64).and_then(|value| i32::try_from(value).ok());
    if let (Some(x), Some(y)) = (whole("x"), whole("y")) {
        sheet.at = Some((x, y));
    }
    if let (Some(w), Some(h)) = (whole("w"), whole("h")) {
        let fit = |value: i32, least: u32, most: u32| (value.max(0) as u32).clamp(least, most);
        sheet.size = (fit(w, Sheet::SMALLEST.0, Sheet::LARGEST.0), fit(h, Sheet::SMALLEST.1, Sheet::LARGEST.1));
    }
    let color = entry.get("color").and_then(Value::as_str).and_then(|text| text.strip_prefix('#'));
    if let Some(color) = color.filter(|hex| hex.len() == 6).and_then(|hex| u32::from_str_radix(hex, 16).ok()) {
        sheet.color = color;
    }
    if let Some(tilt) = entry.get("tilt").and_then(Value::as_f64).filter(|tilt| tilt.abs() <= 45.0) {
        sheet.tilt = (tilt * 10.0).round() as i32;
    }
    sheet.top = entry.get("top").and_then(Value::as_bool).unwrap_or(false);
    sheet
}

fn read_memo(entry: &Value) -> Option<Memo> {
    let id = entry.get("id")?.as_u64().filter(|&id| id > 0)?;
    let text: String = entry.get("text")?.as_str()?.chars().take(TEXT_LIMIT).collect();
    let when = |value: Option<&Value>| value.and_then(Value::as_str).and_then(When::parse);
    let remind = entry.get("remind");
    let kind = remind.and_then(|remind| remind.get("kind")).and_then(Value::as_str).unwrap_or("none");
    let remind = match kind {
        "once" => when(remind.and_then(|remind| remind.get("at"))).map(Remind::Once),
        "daily" => remind
            .and_then(|remind| remind.get("time"))
            .and_then(Value::as_str)
            .and_then(parse_clock)
            .map(|(hour, minute)| Remind::Daily { hour, minute }),
        _ => None,
    }
    // 알림을 읽지 못했으면 알림 없는 메모로 살린다. 적어 둔 글이 더 중요하다.
    .unwrap_or(Remind::None);
    Some(Memo {
        id,
        text,
        created: when(entry.get("created")).unwrap_or(When::from_seconds(0)),
        remind,
        held: entry.get("held").and_then(Value::as_bool).unwrap_or(false),
        next: if remind == Remind::None { None } else { when(entry.get("next")) },
        sheet: read_sheet(entry.get("sheet")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> When {
        When::parse(text).unwrap_or_else(|| panic!("{text}"))
    }

    /// 메모 셋: 날마다 23시, 11월 12일 19시에 한 번, 알림 없음(들고 있기).
    fn sample(now: When) -> Memos {
        let mut memos = Memos::default();
        let plain = memos.add(now).unwrap();
        let once = memos.add(now).unwrap();
        let daily = memos.add(now).unwrap();
        let memo = memos.get_mut(plain).unwrap();
        (memo.text, memo.held) = ("우유 사기\n두 통".to_string(), true);
        let memo = memos.get_mut(once).unwrap();
        memo.text = "방송 보기".to_string();
        memo.set_remind(Remind::Once(at("2026-11-12 19:00")), now);
        let memo = memos.get_mut(daily).unwrap();
        memo.text = "스트레칭".to_string();
        memo.set_remind(Remind::Daily { hour: 23, minute: 0 }, now);
        memos
    }

    fn titles(memos: &[Memo]) -> Vec<String> {
        memos.iter().map(Memo::title).collect()
    }

    #[test]
    fn new_memos_go_first_and_get_fresh_numbers() {
        let now = at("2026-11-10 21:00");
        let mut memos = sample(now);
        assert_eq!(titles(memos.list()), ["스트레칭", "방송 보기", "우유 사기"]);
        assert_eq!(memos.list().iter().map(|memo| memo.id).collect::<Vec<_>>(), [3, 2, 1]);
        // 지운 번호를 바로 다시 쓰지 않는다(가장 큰 번호 다음을 쓴다).
        assert!(memos.remove(2) && !memos.remove(2));
        assert_eq!(memos.add(now), Some(4));
        assert_eq!(memos.list()[0].title(), "(빈 메모)");
        assert_eq!(titles(&memos.held().cloned().collect::<Vec<_>>()), ["우유 사기"]);
    }

    #[test]
    fn there_is_a_limit_on_how_many() {
        let now = at("2026-11-10 21:00");
        let mut memos = Memos::default();
        for _ in 0..COUNT_LIMIT {
            assert!(memos.add(now).is_some());
        }
        assert_eq!(memos.add(now), None);
    }

    #[test]
    fn setting_a_reminder_decides_when_it_rings_next() {
        let now = at("2026-11-10 21:00");
        let mut memos = sample(now);
        assert_eq!(memos.get(3).unwrap().next, Some(at("2026-11-10 23:00")));
        assert_eq!(memos.get(2).unwrap().next, Some(at("2026-11-12 19:00")));
        assert_eq!((memos.get(1).unwrap().next, memos.has_pending()), (None, true));
        assert_eq!(memos.get(2).unwrap().remind_line(), "다음 알림: 11월 12일 오후 7:00");
        // 이미 지난 시각을 고르면 알리지 않는다.
        let memo = memos.get_mut(2).unwrap();
        memo.set_remind(Remind::Once(at("2026-11-10 20:59")), now);
        assert_eq!((memo.next, memo.remind_line().as_str()), (None, "지난 시각입니다. 알리지 않습니다."));
        memo.set_remind(Remind::Once(now), now);
        assert_eq!(memo.next, None);
        // 알림을 없애면 다음 시각도 없어진다.
        let memo = memos.get_mut(3).unwrap();
        memo.set_remind(Remind::None, now);
        assert_eq!((memo.next, memo.remind_line().as_str(), memos.has_pending()), (None, "", false));
    }

    #[test]
    fn due_reminders_ring_once_and_move_on() {
        let mut memos = sample(at("2026-11-10 21:00"));
        assert_eq!(memos.take_due(at("2026-11-10 22:59")), Due::default());
        // 날마다 알리는 것은 알린 뒤 내일로 넘어간다.
        let due = memos.take_due(at("2026-11-10 23:00"));
        assert_eq!((titles(&due.on_time), titles(&due.missed)), (vec!["스트레칭".to_string()], vec![]));
        assert_eq!(memos.get(3).unwrap().next, Some(at("2026-11-11 23:00")));
        assert_eq!(memos.take_due(at("2026-11-10 23:00")), Due::default());
        // 한 번 알리는 것은 알린 뒤 다시 알리지 않는다. 조금 늦게 알아챈 것(4분)도 제때로 친다.
        // 그사이 어젯밤 것은 지나갔다.
        let due = memos.take_due(at("2026-11-12 19:04"));
        assert_eq!(
            (titles(&due.on_time), titles(&due.missed)),
            (vec!["방송 보기".to_string()], vec!["스트레칭".to_string()])
        );
        assert_eq!(
            (memos.get(2).unwrap().next, memos.get(2).unwrap().remind),
            (None, Remind::Once(at("2026-11-12 19:00")))
        );
    }

    #[test]
    fn reminders_that_passed_while_away_are_reported_as_missed() {
        let mut memos = sample(at("2026-11-10 21:00"));
        // 사흘 뒤에 켰다. 날마다 알리는 것은 여러 번 지나갔어도 한 번만 놓친 것으로 치고, 다음은 오늘 밤이다.
        let due = memos.take_due(at("2026-11-13 09:00"));
        assert_eq!(
            (titles(&due.on_time), titles(&due.missed)),
            (vec![], vec!["스트레칭".to_string(), "방송 보기".to_string()])
        );
        assert_eq!(memos.get(3).unwrap().next, Some(at("2026-11-13 23:00")));
        assert_eq!(memos.get(2).unwrap().next, None);
        assert_eq!(memos.take_due(at("2026-11-13 09:00")), Due::default());
    }

    #[test]
    fn snoozing_rings_again_a_little_later() {
        let mut memos = sample(at("2026-11-10 21:00"));
        memos.take_due(at("2026-11-12 19:00"));
        // 한 번 알리는 것도, 날마다 알리는 것도 5분 뒤에 다시 알린다.
        assert!(memos.snooze(2, at("2026-11-12 19:01")) && !memos.snooze(99, at("2026-11-12 19:01")));
        assert_eq!(titles(&memos.take_due(at("2026-11-12 19:05")).on_time), Vec::<String>::new());
        assert_eq!(titles(&memos.take_due(at("2026-11-12 19:06")).on_time), ["방송 보기"]);
        assert_eq!(memos.get(2).unwrap().next, None);
        memos.take_due(at("2026-11-12 23:00"));
        memos.snooze(3, at("2026-11-12 23:02"));
        assert_eq!(titles(&memos.take_due(at("2026-11-12 23:07")).on_time), ["스트레칭"]);
        // 미룬 것을 알린 뒤에는 원래대로 내일 그 시각이다.
        assert_eq!(memos.get(3).unwrap().next, Some(at("2026-11-13 23:00")));
    }

    #[test]
    fn memos_read_back_the_same() {
        let now = at("2026-11-10 21:00");
        let mut memos = sample(now);
        memos.get_mut(1).unwrap().text = "따옴표 \" 와 역빗금 \\ 와\n줄바꿈, 그리고 🌸".to_string();
        memos.take_due(at("2026-11-12 19:00"));
        memos.get_mut(2).unwrap().sheet =
            Sheet { at: Some((-1200, 340)), size: (300, 240), color: 0xCBE6FB, tilt: -45, top: true };
        memos.get_mut(3).unwrap().sheet.tilt = 6;
        let text = memos.to_json();
        assert!(text.starts_with(
            "{\n  \"version\": 1,\n  \"memos\": [\n    {\n      \"id\": 3,\n      \"text\": \"스트레칭\","
        ));
        // 붙인 적이 없는 메모는 자리를 적지 않는다.
        let last = r##""sheet": { "w": 196, "h": 196, "color": "#FFF3A6", "tilt": 0.0, "top": false }"##;
        assert!(text.ends_with(&format!("\"next\": null,\n      {last}\n    }}\n  ]\n}}\n")), "{text}");
        let placed = r##"{ "x": -1200, "y": 340, "w": 300, "h": 240, "color": "#CBE6FB", "tilt": -4.5, "top": true }"##;
        assert!(text.contains(placed) && text.contains(r#""tilt": 0.6,"#), "{text}");
        assert_eq!(Memos::default().to_json(), "{\n  \"version\": 1,\n  \"memos\": []\n}\n");
        assert_eq!(Memos::from_json(&text), Some(memos));
        assert_eq!(Memos::from_json(&Memos::default().to_json()), Some(Memos::default()));
    }

    #[test]
    fn a_broken_file_is_refused_but_a_broken_memo_is_only_skipped() {
        for text in ["", "not json", "[]", r#"{"version": 1}"#, r#"{"memos": "none"}"#] {
            assert_eq!(Memos::from_json(text), None, "{text}");
        }
        let text = r#"{"version": 1, "memos": [
            {"id": 5, "text": "살릴 메모", "created": "2026-11-10 21:00", "remind": {"kind": "weekly"}, "next": "2026-11-11 09:00"},
            {"id": 5, "text": "번호가 겹침"},
            {"id": 0, "text": "번호가 없음"},
            {"text": "번호가 빠짐"},
            {"id": 6},
            "메모가 아님",
            {"id": 7, "text": "날마다", "remind": {"kind": "daily", "time": "07:30"}, "held": true},
            {"id": 8, "text": "시각이 이상함", "remind": {"kind": "once", "at": "2026-02-30 10:00"}, "next": "2026-02-30 10:00"}
        ]}"#;
        let mut memos = Memos::from_json(text).unwrap();
        assert_eq!(titles(memos.list()), ["살릴 메모", "날마다", "시각이 이상함"]);
        // 모르는 종류의 알림과 읽지 못한 시각은 알림 없는 메모로 살린다.
        let plain = memos.get(5).unwrap();
        assert_eq!((plain.remind, plain.next, plain.created), (Remind::None, None, at("2026-11-10 21:00")));
        assert_eq!((memos.get(8).unwrap().remind, memos.get(8).unwrap().next), (Remind::None, None));
        // 다음 시각이 빠진 날마다 알림은 읽은 뒤에 채운다.
        assert_eq!((memos.get(7).unwrap().next, memos.get(7).unwrap().held), (None, true));
        memos.settle(at("2026-11-10 21:00"));
        assert_eq!(memos.get(7).unwrap().next, Some(at("2026-11-11 07:30")));
        // 포스트잇의 모습이 없던 때의 파일은 처음 값으로 읽는다. 이상한 값도 처음 값으로 두거나 한도 안으로 줄인다.
        assert_eq!(memos.get(5).unwrap().sheet, Sheet::default());
        let odd = r##"{"memos": [{"id": 1, "text": "가", "sheet": {"x": 10, "w": 9000, "h": 1, "color": "#12", "tilt": 80, "top": 1}}]}"##;
        let sheet = Memos::from_json(odd).unwrap().get(1).unwrap().sheet;
        assert_eq!(sheet, Sheet { size: (Sheet::LARGEST.0, Sheet::SMALLEST.1), ..Sheet::default() });
        // 너무 긴 글은 한도까지만 읽는다.
        let long = format!(r#"{{"memos": [{{"id": 1, "text": "{}"}}]}}"#, "가".repeat(TEXT_LIMIT + 50));
        assert_eq!(Memos::from_json(&long).unwrap().get(1).unwrap().text.chars().count(), TEXT_LIMIT);
    }
}
