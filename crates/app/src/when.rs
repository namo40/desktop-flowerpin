//! 달력 시각 셈.
//!
//! 알림은 "오후 7시"처럼 이 PC의 벽시계에 보이는 시각으로 정한다. 그래서 시간대를 따지지 않고,
//! 벽시계의 시각을 그대로 수로 바꿔 견준다. 시계를 고치거나 시간대를 옮기면 그 시계를 따라간다.
//!
//! Windows를 부르지 않는다. 지금 시각은 밖에서 받는다.

/// 그레고리력의 날짜를 1970-01-01부터의 날수로 바꾼다. 날짜가 맞는지는 보지 않는다.
pub fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    // 3월을 한 해의 첫 달로 놓고 센다(윤년의 2월 29일이 해의 맨 끝에 온다).
    let (year, month) = if month <= 2 { (year - 1, month + 9) } else { (year, month - 3) };
    let era_days = year * 365 + year.div_euclid(4) - year.div_euclid(100) + year.div_euclid(400);
    era_days + (153 * month + 2) / 5 + day - 1 - 719_468
}

/// 1970-01-01부터의 날수를 날짜(년, 월, 일)로 바꾼다.
pub fn civil_from_days(days: i64) -> (i64, i64, i64) {
    // 400년(146,097일)을 한 묶음으로 본다.
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month + 2) / 5 + 1;
    let (year, month) = (year_of_era + era * 400, if month < 10 { month + 3 } else { month - 9 });
    (if month <= 2 { year + 1 } else { year }, month, day)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    let (next_year, next_month) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    days_from_civil(next_year, next_month, 1) - days_from_civil(year, month, 1)
}

/// "오후 3:12"처럼 적은 시각.
pub fn clock_label(hour: u32, minute: u32) -> String {
    let half = if hour < 12 { "오전" } else { "오후" };
    format!("{half} {}:{minute:02}", if hour.is_multiple_of(12) { 12 } else { hour % 12 })
}

/// 벽시계의 한 시각. 분까지 센다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct When {
    /// 1970-01-01 00:00부터의 분.
    minutes: i64,
}

impl When {
    /// 없는 날짜나 시각이면(2월 30일, 25시) `None`이다.
    pub fn new(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> Option<When> {
        let (year, month, day) = (year as i64, month as i64, day as i64);
        let real = (1970..=9999).contains(&year)
            && (1..=12).contains(&month)
            && (1..=days_in_month(year, month)).contains(&day)
            && hour < 24
            && minute < 60;
        real.then(|| When { minutes: days_from_civil(year, month, day) * 1_440 + (hour * 60 + minute) as i64 })
    }

    /// 벽시계의 1970-01-01 00:00부터 흐른 초로 만든다. 초는 버린다.
    pub fn from_seconds(seconds: i64) -> When {
        When { minutes: seconds.div_euclid(60) }
    }

    /// 년, 월, 일, 시, 분.
    pub fn parts(self) -> (i32, u32, u32, u32, u32) {
        let (year, month, day) = civil_from_days(self.minutes.div_euclid(1_440));
        let of_day = self.minutes.rem_euclid(1_440) as u32;
        (year as i32, month as u32, day as u32, of_day / 60, of_day % 60)
    }

    pub fn plus_minutes(self, minutes: i64) -> When {
        When { minutes: self.minutes + minutes }
    }

    /// `earlier`부터 이 시각까지 흐른 분. 이 시각이 앞이면 음수다.
    pub fn minutes_since(self, earlier: When) -> i64 {
        self.minutes - earlier.minutes
    }

    /// 이 시각보다 뒤이면서 가장 가까운 `hour`시 `minute`분. 오늘 그 시각이 지났거나 지금이면 내일이다.
    pub fn next_at(self, hour: u32, minute: u32) -> When {
        let today = self.minutes.div_euclid(1_440) * 1_440 + (hour.min(23) * 60 + minute.min(59)) as i64;
        When { minutes: if today > self.minutes { today } else { today + 1_440 } }
    }

    /// 파일에 적는 글. "2026-11-12 19:00"처럼 적는다.
    pub fn text(self) -> String {
        let (year, month, day, hour, minute) = self.parts();
        format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}")
    }

    /// [`When::text`]가 적은 글을 읽는다.
    pub fn parse(text: &str) -> Option<When> {
        let (date, time) = text.trim().split_once(' ')?;
        let mut date = date.split('-').map(|part| part.parse::<u32>().ok());
        let (year, month, day) = (date.next()??, date.next()??, date.next()??);
        let (hour, minute) = parse_clock(time)?;
        if date.next().is_some() {
            return None;
        }
        When::new(i32::try_from(year).ok()?, month, day, hour, minute)
    }

    /// "오후 7:00"처럼 적은 시각.
    pub fn clock(self) -> String {
        let (_, _, _, hour, minute) = self.parts();
        clock_label(hour, minute)
    }

    /// "11월 12일 오후 7:00"처럼 적은 날짜와 시각.
    pub fn label(self) -> String {
        let (_, month, day, hour, minute) = self.parts();
        format!("{month}월 {day}일 {}", clock_label(hour, minute))
    }
}

/// "19:00"처럼 적은 시각을 읽는다.
pub fn parse_clock(text: &str) -> Option<(u32, u32)> {
    let (hour, minute) = text.trim().split_once(':')?;
    let (hour, minute) = (hour.parse::<u32>().ok()?, minute.parse::<u32>().ok()?);
    (hour < 24 && minute < 60).then_some((hour, minute))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> When {
        When::parse(text).unwrap_or_else(|| panic!("{text}"))
    }

    #[test]
    fn days_and_dates_convert_both_ways() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2026, 11, 10), 20_767);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // 윤년과 평년, 해가 바뀌는 날, 400년마다의 윤년(2000)과 100년마다의 평년(2100)을 지난다.
        let mut day = days_from_civil(1999, 12, 30);
        let end = days_from_civil(2101, 1, 2);
        let mut previous = civil_from_days(day - 1);
        while day <= end {
            let (year, month, date) = civil_from_days(day);
            assert_eq!(days_from_civil(year, month, date), day);
            assert!((year, month, date) > previous, "{year}-{month}-{date}");
            previous = (year, month, date);
            day += 1;
        }
        assert_eq!(civil_from_days(days_from_civil(2028, 2, 29)), (2028, 2, 29));
        assert_eq!(civil_from_days(days_from_civil(2028, 2, 29) + 1), (2028, 3, 1));
        assert_eq!(civil_from_days(days_from_civil(2100, 2, 28) + 1), (2100, 3, 1));
    }

    #[test]
    fn a_moment_is_written_and_read_back() {
        let when = When::new(2026, 11, 12, 19, 0).unwrap();
        assert_eq!(when.text(), "2026-11-12 19:00");
        assert_eq!(When::parse("2026-11-12 19:00"), Some(when));
        assert_eq!(when.parts(), (2026, 11, 12, 19, 0));
        assert_eq!((when.clock(), when.label()), ("오후 7:00".to_string(), "11월 12일 오후 7:00".to_string()));
        assert_eq!(at("2026-01-05 00:07").clock(), "오전 12:07");
        assert_eq!(at("2026-01-05 12:00").clock(), "오후 12:00");
        // 초로 만들면 초는 버린다.
        assert_eq!(When::from_seconds(20_767 * 86_400 + 19 * 3_600 + 59).text(), "2026-11-10 19:00");
    }

    #[test]
    fn dates_that_do_not_exist_are_refused() {
        assert_eq!(When::new(2026, 2, 29, 0, 0), None);
        assert!(When::new(2028, 2, 29, 0, 0).is_some());
        assert_eq!(When::new(2026, 4, 31, 0, 0), None);
        assert_eq!(When::new(2026, 13, 1, 0, 0), None);
        assert_eq!(When::new(2026, 1, 1, 24, 0), None);
        assert_eq!(When::new(2026, 1, 1, 0, 60), None);
        assert_eq!(When::new(1969, 12, 31, 0, 0), None);
        for text in ["", "2026-11-12", "2026-11-12 19", "2026-11-12 19:00:00", "2026-11-12-1 19:00", "내일 19:00"] {
            assert_eq!(When::parse(text), None, "{text}");
        }
        assert_eq!((parse_clock("07:05"), parse_clock("24:00"), parse_clock("7")), (Some((7, 5)), None, None));
    }

    #[test]
    fn the_next_time_of_day_is_today_or_tomorrow() {
        let now = at("2026-11-10 21:30");
        assert_eq!(now.next_at(23, 0), at("2026-11-10 23:00"));
        assert_eq!(now.next_at(7, 0), at("2026-11-11 07:00"));
        // 지금 그 시각이면 내일이다. 방금 알린 것을 또 알리지 않는다.
        assert_eq!(now.next_at(21, 30), at("2026-11-11 21:30"));
        // 달과 해가 넘어간다.
        assert_eq!(at("2026-12-31 23:59").next_at(0, 0), at("2027-01-01 00:00"));
        assert_eq!(at("2026-11-10 21:30").plus_minutes(5).minutes_since(now), 5);
    }
}
