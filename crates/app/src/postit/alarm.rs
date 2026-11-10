//! 포스트잇의 알림을 판과 포스트잇이 다루기 쉬운 모양으로 적은 것.
//!
//! 메모의 알림([`Remind`])과 같은 뜻이다. 판에서는 날짜를 하루씩 넘기고 시와 분을 따로 돌리므로,
//! 시각을 날수와 시, 분으로 나눠 들고 있는 쪽이 다루기 쉽다.

use crate::memos::Remind;
use crate::when::{When, civil_from_days, clock_label};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alarm {
    None,
    /// 정한 날(1970-01-01부터의 날수), 정한 시각에 한 번.
    Once {
        day: i64,
        hour: u32,
        minute: u32,
    },
    /// 날마다 이 시각에.
    Daily {
        hour: u32,
        minute: u32,
    },
}

impl Alarm {
    pub fn from(remind: Remind) -> Alarm {
        match remind {
            Remind::None => Alarm::None,
            Remind::Once(at) => {
                let of_day = at.minute_of_day() as u32;
                Alarm::Once { day: at.day(), hour: of_day / 60, minute: of_day % 60 }
            }
            Remind::Daily { hour, minute } => Alarm::Daily { hour, minute },
        }
    }

    pub fn remind(self) -> Remind {
        match self {
            Alarm::None => Remind::None,
            Alarm::Once { day, hour, minute } => Remind::Once(When::on(day, hour, minute)),
            Alarm::Daily { hour, minute } => Remind::Daily { hour, minute },
        }
    }

    /// 한 번 알리는 것인데 그 시각이 이미 지났는가.
    pub fn is_past(self, now: When) -> bool {
        matches!(self.remind(), Remind::Once(at) if at <= now)
    }

    /// 포스트잇에 적는 짧은 글. "오늘 오후 7:00", "11/12 오후 7:00", "매일 오후 11:00"처럼 적는다.
    pub fn chip(self, today: i64) -> Option<String> {
        match self {
            Alarm::None => None,
            Alarm::Once { day, hour, minute } => {
                let date = match day - today {
                    0 => "오늘".to_string(),
                    1 => "내일".to_string(),
                    _ => {
                        let (_, month, date) = civil_from_days(day);
                        format!("{month}/{date}")
                    }
                };
                Some(format!("{date} {}", clock_label(hour, minute)))
            }
            Alarm::Daily { hour, minute } => Some(format!("매일 {}", clock_label(hour, minute))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> When {
        When::parse(text).unwrap_or_else(|| panic!("{text}"))
    }

    #[test]
    fn an_alarm_and_a_reminder_convert_both_ways() {
        let once = Remind::Once(at("2026-11-12 19:05"));
        let daily = Remind::Daily { hour: 23, minute: 0 };
        for remind in [Remind::None, once, daily] {
            assert_eq!(Alarm::from(remind).remind(), remind);
        }
        assert!(
            Alarm::from(once).is_past(at("2026-11-12 19:05")) && !Alarm::from(once).is_past(at("2026-11-12 19:04"))
        );
        assert!(!Alarm::from(daily).is_past(at("2026-11-12 23:30")) && !Alarm::None.is_past(at("2026-11-12 23:30")));
    }

    #[test]
    fn the_note_shows_when_it_rings_in_a_few_words() {
        let today = at("2026-11-10 21:00").day();
        let chip = |text: &str| Alarm::from(Remind::Once(at(text))).chip(today).unwrap();
        assert_eq!(chip("2026-11-10 19:00"), "오늘 오후 7:00");
        assert_eq!(chip("2026-11-11 09:00"), "내일 오전 9:00");
        assert_eq!(chip("2026-11-14 00:05"), "11/14 오전 12:05");
        assert_eq!(Alarm::Daily { hour: 23, minute: 0 }.chip(today).unwrap(), "매일 오후 11:00");
        assert_eq!(Alarm::None.chip(today), None);
    }
}
