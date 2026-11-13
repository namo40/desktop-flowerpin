//! 사용자가 고르는 설정. 항목, 고를 수 있는 것, 기본값을 정한다.
//!
//! 거의 모든 항목은 "몇 가지 가운데 하나를 고르는 것"으로 다룬다. 켜고 끄는 항목도 [끔, 켬] 가운데 하나를
//! 고르는 것이다. 머무는 시간처럼 수를 직접 적는 항목만 따로, 받는 범위를 둔다. 그래서 설정 창, 파일 읽기와
//! 쓰기, 기본값으로 되돌리기가 항목 표([`SPECS`]) 하나만 보고 돌아간다.
//!
//! 파일(`settings.json`)에는 고른 순번이 아니라 뜻이 있는 값을 적는다(크기는 100, 확인 간격은 3처럼).
//! 고를 수 있는 것을 나중에 더하거나 순서를 바꿔도 예전 파일이 그대로 읽힌다.

use serde_json::Value;

use crate::actor::Habits;

/// 파일 형식의 판. 형식을 바꾸면 올린다.
///
/// 판 2에서 한 번에 둘 수 있는 꽃핀의 수가 9명에서 [`CROWD_MAX`]명으로 늘었다.
pub const VERSION: i64 = 2;

/// 한 번에 나와 있을 수 있는 꽃핀의 가장 큰 수. 기본 꽃핀도 센다.
///
/// 꽃핀이 모두 돌아다닐 때 CPU 한 코어의 20%쯤을 쓰는 수다. 만든 PC(Ryzen 9 3950X, 화면 배율 100%)에서
/// 꽃핀 크기를 "크게", 가끔 하는 행동을 "자주"로 두고 수를 늘려 가며 쟀다(40명 19.9%, 45명 22.6%).
/// 크기와 행동을 기본값으로 두면 같은 40명이 13%쯤이다. 느린 PC나 배율이 큰 화면에서는 더 든다.
pub const CROWD_MAX: i64 = 40;

/// 판 1에서 가장 많이 둘 수 있던 꽃핀의 수.
const OLD_CROWD_MAX: i64 = 9;

/// 설정 항목. 순서는 [`SPECS`]와 같다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    StartWithWindows,
    Size,
    AlwaysOnTop,
    UpdateCheck,
    IdleActions,
    FollowCursor,
    Stay,
    MaxCharacters,
    LiveCheck,
    LiveCheckInterval,
    LiveStartNotice,
    LiveEndNotice,
    HideOnFullscreen,
    ReminderToast,
}

/// 항목 수.
pub const COUNT: usize = 14;

/// 파일에 적는 값.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stored {
    Flag(bool),
    Number(i64),
    Word(&'static str),
}

/// 고를 수 있는 것 하나. 설정 창에 보이는 이름과 파일에 적는 값이다.
#[derive(Clone, Copy, Debug)]
pub struct Pick {
    pub label: &'static str,
    pub stored: Stored,
}

/// 항목 하나의 생김새.
#[derive(Clone, Copy, Debug)]
pub struct Spec {
    pub key: Key,
    /// `settings.json`의 키.
    pub name: &'static str,
    /// 설정 창의 몇 번째 탭에 있는지(일반 0, 꽃핀 1, 방송 2, 방해하지 않기 3).
    pub tab: usize,
    /// 고를 수 있는 것. 수를 직접 적는 항목은 비어 있다.
    pub picks: &'static [Pick],
    /// 수를 직접 적는 항목이 받는 범위(양 끝도 받는다). 고르는 항목은 `None`이다.
    pub range: Option<(i64, i64)>,
    /// 기본값. 고르는 항목은 기본으로 고르는 것의 순번이고, 수를 적는 항목은 그 수다.
    pub default: i64,
}

impl Spec {
    /// 켜고 끄는 항목인가.
    pub fn is_flag(&self) -> bool {
        matches!(self.picks.first(), Some(Pick { stored: Stored::Flag(_), .. }))
    }
}

/// 몇 가지 가운데 하나를 고르는 항목.
const fn choice(key: Key, name: &'static str, tab: usize, picks: &'static [Pick], default: i64) -> Spec {
    Spec { key, name, tab, picks, range: None, default }
}

const fn number(label: &'static str, value: i64) -> Pick {
    Pick { label, stored: Stored::Number(value) }
}

const fn word(label: &'static str, value: &'static str) -> Pick {
    Pick { label, stored: Stored::Word(value) }
}

const OFF_ON: &[Pick] =
    &[Pick { label: "", stored: Stored::Flag(false) }, Pick { label: "", stored: Stored::Flag(true) }];
const NOTICES: &[Pick] =
    &[word("말풍선 + Windows 알림", "bubble_toast"), word("말풍선만", "bubble"), word("끄기", "off")];

const fn flag(key: Key, name: &'static str, tab: usize, on: bool) -> Spec {
    choice(key, name, tab, OFF_ON, on as i64)
}

pub const SPECS: [Spec; COUNT] = [
    // 일반
    flag(Key::StartWithWindows, "start_with_windows", 0, false),
    choice(Key::Size, "size", 0, &[number("작게", 75), number("보통", 100), number("크게", 125)], 1),
    flag(Key::AlwaysOnTop, "always_on_top", 0, true),
    // 새 버전이 나왔는지 하루에 한 번 GitHub에 묻는다.
    flag(Key::UpdateCheck, "update_check", 0, true),
    // 꽃핀
    choice(Key::IdleActions, "idle_actions", 1, &[number("적게", 50), number("보통", 100), number("자주", 200)], 1),
    // 마우스 커서가 바닥 가까이에 머물면 가까운 꽃핀이 그 아래로 걸어온다.
    flag(Key::FollowCursor, "follow_cursor", 1, true),
    // 더 부른 꽃핀이 머무는 시간(초). 0이면 보낼 때까지 머문다.
    Spec { key: Key::Stay, name: "stay_seconds", tab: 1, picks: &[], range: Some((0, 3600)), default: 30 },
    // 한 번에 나와 있을 수 있는 꽃핀의 수. 기본 꽃핀도 센다.
    Spec {
        key: Key::MaxCharacters,
        name: "max_characters",
        tab: 1,
        picks: &[],
        range: Some((1, CROWD_MAX)),
        default: CROWD_MAX,
    },
    // 방송
    flag(Key::LiveCheck, "live_check", 2, true),
    choice(Key::LiveCheckInterval, "live_check_min", 2, &[number("1분", 1), number("3분", 3), number("5분", 5)], 0),
    choice(Key::LiveStartNotice, "live_start_notice", 2, NOTICES, 0),
    choice(Key::LiveEndNotice, "live_end_notice", 2, NOTICES, 1),
    // 방해하지 않기
    flag(Key::HideOnFullscreen, "hide_on_fullscreen", 3, true),
    flag(Key::ReminderToast, "reminder_toast", 3, true),
];

/// 항목의 생김새를 찾는다.
pub fn spec(key: Key) -> &'static Spec {
    &SPECS[key as usize]
}

/// 사용자가 정한 것들. 항목마다 고른 순번을 들고 있다. 수를 직접 적는 항목은 그 수를 들고 있다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    values: [i64; COUNT],
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { values: std::array::from_fn(|i| SPECS[i].default) }
    }
}

impl Settings {
    /// 고른 것의 순번. 고르는 항목에만 뜻이 있다.
    pub fn get(&self, key: Key) -> usize {
        self.values[key as usize] as usize
    }

    /// 순번으로 고른다. 없는 순번이면 바꾸지 않는다.
    pub fn set(&mut self, key: Key, pick: usize) {
        if pick < spec(key).picks.len() {
            self.values[key as usize] = pick as i64;
        }
    }

    /// 수를 직접 적는 항목에 수를 적는다. 범위를 벗어나면 가까운 끝으로 맞춘다.
    pub fn set_number(&mut self, key: Key, value: i64) {
        if let Some((least, most)) = spec(key).range {
            self.values[key as usize] = value.clamp(least, most);
        }
    }

    fn stored(&self, key: Key) -> Stored {
        match spec(key).range {
            Some(_) => Stored::Number(self.values[key as usize]),
            None => spec(key).picks[self.get(key)].stored,
        }
    }

    /// 켜고 끄는 항목이 켜져 있는가.
    pub fn is_on(&self, key: Key) -> bool {
        self.stored(key) == Stored::Flag(true)
    }

    /// 값이 수인 항목의 값. 고른 것에 딸린 수이거나, 직접 적은 수다. 수가 아닌 항목이면 0이다.
    pub fn number(&self, key: Key) -> i64 {
        if let Stored::Number(value) = self.stored(key) { value } else { 0 }
    }

    /// 값이 글인 항목의 값. 글이 아닌 항목이면 빈 글이다.
    pub fn word(&self, key: Key) -> &'static str {
        if let Stored::Word(word) = self.stored(key) { word } else { "" }
    }

    /// 한 탭의 항목을 모두 기본값으로 되돌린다.
    pub fn reset_tab(&mut self, tab: usize) {
        for (value, spec) in self.values.iter_mut().zip(&SPECS) {
            if spec.tab == tab {
                *value = spec.default;
            }
        }
    }

    /// 꽃핀 크기(퍼센트).
    pub fn size_percent(&self) -> u32 {
        self.number(Key::Size) as u32
    }

    /// 꽃핀의 버릇.
    pub fn habits(&self) -> Habits {
        Habits { whim_rate: self.number(Key::IdleActions) as f64 / 100.0, ..Habits::default() }
    }

    /// 파일에 적을 글을 만든다.
    pub fn to_json(&self) -> String {
        let mut text = format!("{{\n  \"version\": {VERSION}");
        for spec in &SPECS {
            let value = match self.stored(spec.key) {
                Stored::Flag(on) => on.to_string(),
                Stored::Number(value) => value.to_string(),
                Stored::Word(word) => format!("\"{word}\""),
            };
            text.push_str(&format!(",\n  \"{}\": {value}", spec.name));
        }
        text.push_str("\n}\n");
        text
    }

    /// 파일의 글을 읽는다. JSON이 아니거나 맨 바깥이 `{ }`가 아니면 `None`이다.
    ///
    /// 빠진 항목과, 고를 수 없는 값이나 범위 밖의 수가 적힌 항목은 기본값으로 둔다. 모르는 키는 무시한다.
    pub fn from_json(text: &str) -> Option<Settings> {
        let value: Value = serde_json::from_str(text).ok()?;
        let object = value.as_object()?;
        let old = object.get("version").and_then(Value::as_i64).unwrap_or(1) < 2;
        let mut settings = Settings::default();
        for spec in &SPECS {
            let Some(found) = object.get(spec.name) else { continue };
            // 판 1에서 가장 많이 두게 해 둔 것은, 늘어난 뒤에도 가장 많이 두는 것으로 읽는다.
            if old && spec.key == Key::MaxCharacters && found.as_i64() == Some(OLD_CROWD_MAX) {
                continue;
            }
            if let Some((least, most)) = spec.range {
                if let Some(value) = found.as_i64().filter(|value| (least..=most).contains(value)) {
                    settings.set_number(spec.key, value);
                }
                continue;
            }
            let matches = |pick: &Pick| match pick.stored {
                Stored::Flag(on) => found.as_bool() == Some(on),
                Stored::Number(value) => found.as_i64() == Some(value),
                Stored::Word(word) => found.as_str() == Some(word),
            };
            if let Some(pick) = spec.picks.iter().position(matches) {
                settings.set(spec.key, pick);
            }
        }
        Some(settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs_are_in_key_order_and_names_are_unique() {
        for (i, spec) in SPECS.iter().enumerate() {
            assert_eq!(spec.key as usize, i, "{}", spec.name);
            assert!(SPECS[..i].iter().all(|other| other.name != spec.name), "{}", spec.name);
            if let Some((least, most)) = spec.range {
                // 수를 직접 적는 항목은 고를 것이 없고, 기본값이 범위 안에 있다.
                assert!(spec.picks.is_empty() && (least..=most).contains(&spec.default), "{}", spec.name);
                continue;
            }
            assert!((0..spec.picks.len() as i64).contains(&spec.default), "{}", spec.name);
            // 한 항목 안에서는 값의 종류가 같고, 같은 값이 두 번 나오지 않는다.
            for (n, pick) in spec.picks.iter().enumerate() {
                assert_eq!(
                    std::mem::discriminant(&pick.stored),
                    std::mem::discriminant(&spec.picks[0].stored),
                    "{}",
                    spec.name
                );
                assert!(spec.picks[..n].iter().all(|other| other.stored != pick.stored), "{}", spec.name);
            }
        }
    }

    #[test]
    fn defaults_match_the_feature_list() {
        let settings = Settings::default();
        assert!(!settings.is_on(Key::StartWithWindows));
        assert!(settings.is_on(Key::AlwaysOnTop));
        assert!(settings.is_on(Key::UpdateCheck));
        assert_eq!(settings.size_percent(), 100);
        assert_eq!(settings.habits(), Habits::default());
        assert_eq!(settings.number(Key::MaxCharacters), CROWD_MAX);
        assert_eq!(settings.number(Key::Stay), 30);
        assert_eq!(spec(Key::LiveStartNotice).picks[settings.get(Key::LiveStartNotice)].label, "말풍선 + Windows 알림");
        assert_eq!(spec(Key::LiveEndNotice).picks[settings.get(Key::LiveEndNotice)].label, "말풍선만");
        assert_eq!(
            (settings.word(Key::LiveStartNotice), settings.word(Key::LiveEndNotice)),
            ("bubble_toast", "bubble")
        );
        assert_eq!(settings.word(Key::Size), "");
    }

    #[test]
    fn written_file_reads_back_the_same() {
        let mut settings = Settings::default();
        settings.set(Key::Size, 2);
        settings.set(Key::AlwaysOnTop, 0);
        settings.set(Key::IdleActions, 2);
        settings.set(Key::LiveEndNotice, 2);
        let text = settings.to_json();
        assert!(
            text.contains("\"version\": 2")
                && text.contains("\"size\": 125")
                && text.contains("\"always_on_top\": false")
        );
        assert_eq!(Settings::from_json(&text), Some(settings.clone()));
        assert_eq!(settings.size_percent(), 125);
        assert_eq!(settings.habits(), Habits { roam: true, whim_rate: 2.0 });
    }

    #[test]
    fn a_typed_number_is_kept_within_its_range() {
        let mut settings = Settings::default();
        settings.set_number(Key::Stay, 45);
        assert_eq!(settings.number(Key::Stay), 45);
        assert!(settings.to_json().contains("\"stay_seconds\": 45"));
        assert_eq!(Settings::from_json(&settings.to_json()), Some(settings.clone()));
        // 범위를 벗어난 수는 가까운 끝으로 맞춘다.
        settings.set_number(Key::Stay, 99999);
        assert_eq!(settings.number(Key::Stay), 3600);
        settings.set_number(Key::Stay, -5);
        assert_eq!(settings.number(Key::Stay), 0);
        // 고르는 항목에는 수를 적을 수 없다.
        settings.set_number(Key::Size, 3);
        assert_eq!(settings.size_percent(), 100);
        // 파일에 범위 밖의 수나 수가 아닌 것이 적혀 있으면 기본값이다.
        for text in [r#"{"stay_seconds": 99999}"#, r#"{"stay_seconds": -1}"#, r#"{"stay_seconds": "30"}"#] {
            assert_eq!(Settings::from_json(text).unwrap().number(Key::Stay), 30, "{text}");
        }
        // 한 탭을 되돌리면 기본값으로 돌아온다.
        settings.set_number(Key::Stay, 120);
        settings.reset_tab(1);
        assert_eq!(settings.number(Key::Stay), 30);
    }

    #[test]
    fn missing_and_unknown_keys_are_tolerated() {
        let settings = Settings::from_json(r#"{"version": 1, "size": 75, "from_the_future": [1, 2]}"#).unwrap();
        let mut expected = Settings::default();
        expected.set(Key::Size, 0);
        assert_eq!(settings, expected);
    }

    #[test]
    fn the_old_crowd_limit_reads_as_the_new_one() {
        let read = |text: &str| Settings::from_json(text).unwrap().number(Key::MaxCharacters);
        // 판 1에서 가장 많이(9명) 두게 해 둔 것은 늘어난 수로 읽는다. 줄여 둔 것은 그대로다.
        assert_eq!(read(r#"{"version": 1, "max_characters": 9}"#), CROWD_MAX);
        assert_eq!(read(r#"{"max_characters": 9}"#), CROWD_MAX);
        assert_eq!(read(r#"{"version": 1, "max_characters": 4}"#), 4);
        // 판 2에서 9명으로 정한 것은 9명이다.
        assert_eq!(read(r#"{"version": 2, "max_characters": 9}"#), 9);
        // 범위를 벗어난 수는 기본값으로 둔다.
        assert_eq!(read(&format!(r#"{{"version": 2, "max_characters": {}}}"#, CROWD_MAX + 1)), CROWD_MAX);
        assert_eq!(read(r#"{"version": 2, "max_characters": 0}"#), CROWD_MAX);
        let mut settings = Settings::default();
        settings.set_number(Key::MaxCharacters, 9);
        assert_eq!(Settings::from_json(&settings.to_json()), Some(settings));
    }

    #[test]
    fn values_that_cannot_be_picked_fall_back_to_the_default() {
        // 고를 수 없는 숫자, 종류가 다른 값은 그 항목만 기본값으로 둔다.
        let text = r#"{"size": 90, "always_on_top": "yes", "max_characters": 3, "live_start_notice": 5}"#;
        let settings = Settings::from_json(text).unwrap();
        assert_eq!(settings.size_percent(), 100);
        assert!(settings.is_on(Key::AlwaysOnTop));
        assert_eq!(settings.number(Key::MaxCharacters), 3);
        assert_eq!(settings.get(Key::LiveStartNotice), 0);
    }

    #[test]
    fn broken_text_is_rejected() {
        for text in ["", "{", "[1, 2]", "\"size\": 100", "{\"size\": 100,}"] {
            assert_eq!(Settings::from_json(text), None, "{text}");
        }
    }

    #[test]
    fn reset_touches_only_one_tab() {
        let mut settings = Settings::default();
        settings.set(Key::Size, 0);
        settings.set(Key::IdleActions, 0);
        settings.set(Key::LiveCheck, 0);
        settings.reset_tab(1);
        assert_eq!(
            (settings.get(Key::Size), settings.get(Key::IdleActions), settings.is_on(Key::LiveCheck)),
            (0, 1, false)
        );
    }

    #[test]
    fn picking_an_index_that_does_not_exist_changes_nothing() {
        let mut settings = Settings::default();
        settings.set(Key::Size, 9);
        assert_eq!(settings, Settings::default());
    }
}
