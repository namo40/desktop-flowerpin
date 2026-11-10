//! 사용자가 고르는 설정. 항목, 고를 수 있는 것, 기본값을 정한다.
//!
//! 모든 항목은 "몇 가지 가운데 하나를 고르는 것"으로 다룬다. 켜고 끄는 항목도 [끔, 켬] 가운데 하나를 고르는
//! 것이다. 그래서 설정 창, 파일 읽기와 쓰기, 기본값으로 되돌리기가 항목 표([`SPECS`]) 하나만 보고 돌아간다.
//!
//! 파일(`settings.json`)에는 고른 순번이 아니라 뜻이 있는 값을 적는다(크기는 100, 확인 간격은 3처럼).
//! 고를 수 있는 것을 나중에 더하거나 순서를 바꿔도 예전 파일이 그대로 읽힌다.

use serde_json::Value;

use crate::actor::Habits;

/// 파일 형식의 판. 형식을 바꾸면 올린다.
pub const VERSION: i64 = 1;

/// 설정 항목. 순서는 [`SPECS`]와 같다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    StartWithWindows,
    Size,
    AlwaysOnTop,
    IdleActions,
    ExtraStay,
    MaxCharacters,
    LiveCheck,
    LiveCheckInterval,
    LiveStartNotice,
    LiveEndNotice,
    HideOnFullscreen,
    ReminderToast,
}

/// 항목 수.
pub const COUNT: usize = 12;

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
    pub picks: &'static [Pick],
    /// 기본으로 고르는 것의 순번.
    pub default: usize,
}

impl Spec {
    /// 켜고 끄는 항목인가.
    pub fn is_flag(&self) -> bool {
        matches!(self.picks[0].stored, Stored::Flag(_))
    }
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
    Spec { key, name, tab, picks: OFF_ON, default: on as usize }
}

pub const SPECS: [Spec; COUNT] = [
    // 일반
    flag(Key::StartWithWindows, "start_with_windows", 0, false),
    Spec {
        key: Key::Size,
        name: "size",
        tab: 0,
        picks: &[number("작게", 75), number("보통", 100), number("크게", 125)],
        default: 1,
    },
    flag(Key::AlwaysOnTop, "always_on_top", 0, true),
    // 꽃핀
    Spec {
        key: Key::IdleActions,
        name: "idle_actions",
        tab: 1,
        picks: &[number("적게", 50), number("보통", 100), number("자주", 200)],
        default: 1,
    },
    Spec {
        key: Key::ExtraStay,
        name: "extra_stay",
        tab: 1,
        picks: &[number("계속", 0), number("15분", 15), number("5분", 5), number("1분", 1)],
        default: 0,
    },
    Spec {
        key: Key::MaxCharacters,
        name: "max_characters",
        tab: 1,
        picks: &[
            number("1명", 1),
            number("2명", 2),
            number("3명", 3),
            number("4명", 4),
            number("5명", 5),
            number("6명", 6),
            number("7명", 7),
            number("8명", 8),
            number("9명", 9),
        ],
        default: 8,
    },
    // 방송
    flag(Key::LiveCheck, "live_check", 2, true),
    Spec {
        key: Key::LiveCheckInterval,
        name: "live_check_min",
        tab: 2,
        picks: &[number("1분", 1), number("3분", 3), number("5분", 5)],
        default: 0,
    },
    Spec { key: Key::LiveStartNotice, name: "live_start_notice", tab: 2, picks: NOTICES, default: 0 },
    Spec { key: Key::LiveEndNotice, name: "live_end_notice", tab: 2, picks: NOTICES, default: 1 },
    // 방해하지 않기
    flag(Key::HideOnFullscreen, "hide_on_fullscreen", 3, true),
    flag(Key::ReminderToast, "reminder_toast", 3, true),
];

/// 항목의 생김새를 찾는다.
pub fn spec(key: Key) -> &'static Spec {
    &SPECS[key as usize]
}

/// 사용자가 고른 것들. 항목마다 고른 순번을 들고 있다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    chosen: [u8; COUNT],
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { chosen: std::array::from_fn(|i| SPECS[i].default as u8) }
    }
}

impl Settings {
    /// 고른 것의 순번.
    pub fn get(&self, key: Key) -> usize {
        self.chosen[key as usize] as usize
    }

    /// 순번으로 고른다. 없는 순번이면 바꾸지 않는다.
    pub fn set(&mut self, key: Key, pick: usize) {
        if pick < spec(key).picks.len() {
            self.chosen[key as usize] = pick as u8;
        }
    }

    fn stored(&self, key: Key) -> Stored {
        spec(key).picks[self.get(key)].stored
    }

    /// 켜고 끄는 항목이 켜져 있는가.
    pub fn is_on(&self, key: Key) -> bool {
        self.stored(key) == Stored::Flag(true)
    }

    /// 숫자로 적는 항목의 값. 숫자 항목이 아니면 0이다.
    pub fn number(&self, key: Key) -> i64 {
        if let Stored::Number(value) = self.stored(key) { value } else { 0 }
    }

    /// 한 탭의 항목을 모두 기본값으로 되돌린다.
    pub fn reset_tab(&mut self, tab: usize) {
        for (chosen, spec) in self.chosen.iter_mut().zip(&SPECS) {
            if spec.tab == tab {
                *chosen = spec.default as u8;
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
    /// 빠진 항목과, 고를 수 없는 값이 적힌 항목은 기본값으로 둔다. 모르는 키는 무시한다.
    pub fn from_json(text: &str) -> Option<Settings> {
        let value: Value = serde_json::from_str(text).ok()?;
        let object = value.as_object()?;
        let mut settings = Settings::default();
        for spec in &SPECS {
            let Some(found) = object.get(spec.name) else { continue };
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
            assert!(spec.default < spec.picks.len(), "{}", spec.name);
            assert!(SPECS[..i].iter().all(|other| other.name != spec.name), "{}", spec.name);
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
        assert_eq!(settings.size_percent(), 100);
        assert_eq!(settings.habits(), Habits::default());
        assert_eq!(settings.number(Key::MaxCharacters), 9);
        assert_eq!(settings.number(Key::ExtraStay), 0);
        assert_eq!(spec(Key::LiveStartNotice).picks[settings.get(Key::LiveStartNotice)].label, "말풍선 + Windows 알림");
        assert_eq!(spec(Key::LiveEndNotice).picks[settings.get(Key::LiveEndNotice)].label, "말풍선만");
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
            text.contains("\"version\": 1")
                && text.contains("\"size\": 125")
                && text.contains("\"always_on_top\": false")
        );
        assert_eq!(Settings::from_json(&text), Some(settings.clone()));
        assert_eq!(settings.size_percent(), 125);
        assert_eq!(settings.habits(), Habits { roam: true, whim_rate: 2.0 });
    }

    #[test]
    fn missing_and_unknown_keys_are_tolerated() {
        let settings = Settings::from_json(r#"{"version": 1, "size": 75, "from_the_future": [1, 2]}"#).unwrap();
        let mut expected = Settings::default();
        expected.set(Key::Size, 0);
        assert_eq!(settings, expected);
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
