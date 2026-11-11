//! 새 버전이 나왔는지 본다.
//!
//! 하루에 한 번 GitHub에 이 앱의 가장 새 릴리스가 무엇인지 묻는다. 지금 쓰는 것보다 새 버전이면 알려 주고,
//! 받는 곳(릴리스 페이지)을 열 수 있게 한다. 받아서 바꾸는 일은 사용자가 한다. 앱이 스스로 내려받거나
//! 제 파일을 고치지 않는다.
//!
//! 여기에는 무엇을 묻고 답을 어떻게 읽는지만 있다. 인터넷에 묻는 일은 [`crate::checker`]가 한다.
//! - 묻는 곳은 GitHub가 공개한 API의 주소 하나다. 로그인 정보와 쿠키를 보내지 않는다.
//! - 답에서 읽는 것은 릴리스의 이름표(`tag_name`) 하나다. 답에 적힌 주소는 쓰지 않는다. 여는 주소는
//!   여기에 정해 둔 것뿐이다.

use std::fmt;

use serde_json::Value;

/// 가장 새 릴리스의 페이지. 새 버전을 받는 곳이다.
pub const RELEASES_URL: &str = "https://github.com/namo40/desktop-flowerpin/releases/latest";
/// 가장 새 릴리스를 묻는 곳.
pub const HOST: &str = "api.github.com";
pub const PATH: &str = "/repos/namo40/desktop-flowerpin/releases/latest";

/// 다시 묻기까지의 시간(초). 하루에 한 번이다.
pub const EVERY: i64 = 24 * 60 * 60;
/// 묻지 못했을 때(연결이 안 됨) 다시 묻기까지의 시간(초).
pub const RETRY: u64 = 60 * 60;
/// 켠 뒤 처음 묻기까지 기다리는 시간(초). 켜자마자 묻지 않는다.
pub const FIRST: u64 = 20;

/// 버전. 점으로 나눈 수 셋이다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u32, pub u32, pub u32);

impl Version {
    /// "0.1.0"이나 "v0.1.0"을 읽는다. 수가 하나나 둘이면 뒤를 0으로 채운다. 그 밖의 글은 `None`이다.
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.trim();
        let text = text.strip_prefix(['v', 'V']).unwrap_or(text);
        let number = |part: &str| {
            let digits = !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
            digits.then(|| part.parse::<u32>().ok()).flatten()
        };
        let numbers: Vec<u32> = text.split('.').map(number).collect::<Option<_>>()?;
        match numbers[..] {
            [major] => Some(Version(major, 0, 0)),
            [major, minor] => Some(Version(major, minor, 0)),
            [major, minor, patch] => Some(Version(major, minor, patch)),
            _ => None,
        }
    }

    /// 지금 돌고 있는 이 앱의 버전.
    pub fn current() -> Version {
        Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Version(0, 0, 0))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// GitHub에서 온 답.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reply {
    /// 가장 새 릴리스의 버전.
    Latest(Version),
    /// 공개된 릴리스가 없다.
    Nothing,
    /// 묻지 못했거나 답을 읽지 못했다.
    Failed,
}

/// HTTP 상태 번호와 응답 본문을 읽어 무슨 답인지 정한다.
pub fn judge(status: u32, body: &str) -> Reply {
    let latest = || serde_json::from_str::<Value>(body).ok()?.get("tag_name")?.as_str().and_then(Version::parse);
    match status {
        200 => latest().map_or(Reply::Failed, Reply::Latest),
        404 => Reply::Nothing,
        _ => Reply::Failed,
    }
}

/// 새 버전 확인이 지금 어떤지. 설정 창에 한 줄로 보여 준다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// 아직 물어보지 않았다.
    Unknown,
    /// 물어 놓고 답을 기다리는 중이다.
    Asking,
    /// 지금 쓰는 것이 가장 새 버전이다.
    Current,
    /// 더 새 버전이 나와 있다.
    Newer(Version),
    /// 공개된 릴리스가 없다.
    Nothing,
    Failed,
}

impl Status {
    /// 저장해 둔 "가장 새 버전"으로 시작할 때의 상태를 정한다. 지금 쓰는 것보다 새 버전이면 그것을 알고
    /// 시작한다. 그래야 켜자마자, 인터넷에 묻기 전에도 메뉴에 받는 길이 보인다.
    pub fn remembered(latest: Option<&str>, current: Version) -> Status {
        match latest.and_then(Version::parse) {
            Some(latest) if latest > current => Status::Newer(latest),
            _ => Status::Unknown,
        }
    }

    /// 답을 받은 뒤의 상태. 묻지 못했으면, 알고 있던 새 버전은 그대로 안다.
    pub fn after(self, reply: Reply, current: Version) -> Status {
        match reply {
            Reply::Latest(latest) if latest > current => Status::Newer(latest),
            Reply::Latest(_) => Status::Current,
            Reply::Nothing => Status::Nothing,
            Reply::Failed => match self {
                Status::Newer(_) => self,
                _ => Status::Failed,
            },
        }
    }

    /// 더 새 버전이 나와 있으면 그 버전.
    pub fn newer(self) -> Option<Version> {
        if let Status::Newer(version) = self { Some(version) } else { None }
    }
}

/// 설정 창에 보여 줄 한 줄. `enabled`는 저절로 확인하게 해 두었는지다.
pub fn status_line(status: Status, enabled: bool) -> String {
    match status {
        Status::Unknown if enabled => "새 버전이 있는지 아직 확인하지 않았습니다.".to_string(),
        Status::Unknown => "새 버전 확인을 껐습니다. 아래 단추로 확인할 수 있습니다.".to_string(),
        Status::Asking => "새 버전이 있는지 확인하는 중입니다.".to_string(),
        Status::Current => "가장 새 버전을 쓰고 있습니다.".to_string(),
        Status::Newer(version) => format!("새 버전이 나왔습니다: {version}"),
        Status::Nothing => "공개된 버전이 아직 없습니다.".to_string(),
        Status::Failed => "새 버전이 있는지 확인하지 못했습니다.".to_string(),
    }
}

/// 다음에 묻기까지 기다릴 시간(초). `checked`는 마지막으로 답을 받은 때, `now`는 지금이다(유닉스 초).
/// 물은 적이 없으면 바로 묻는다. 적어 둔 때가 지금보다 뒤면(PC의 시계가 바뀜) 그것도 바로 묻는다.
pub fn wait(checked: Option<i64>, now: i64) -> u64 {
    match checked {
        Some(at) if at <= now => (at + EVERY - now).max(0) as u64,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_read_and_compared_by_number() {
        assert_eq!(Version::parse("0.1.0"), Some(Version(0, 1, 0)));
        assert_eq!(Version::parse("v1.2.3"), Some(Version(1, 2, 3)));
        assert_eq!(Version::parse(" V2 "), Some(Version(2, 0, 0)));
        assert_eq!(Version::parse("1.4"), Some(Version(1, 4, 0)));
        for wrong in ["", "v", "1..2", "1.2.3.4", "1.2.x", "1.2.3-beta", "+1.2.3", "latest"] {
            assert_eq!(Version::parse(wrong), None, "{wrong}");
        }
        // 글이 아니라 수로 견준다.
        assert!(Version(0, 10, 0) > Version(0, 9, 9));
        assert!(Version(1, 0, 0) > Version(0, 99, 99));
        assert_eq!(Version(0, 2, 0).to_string(), "0.2.0");
        // 이 앱의 버전은 읽히는 모양이다.
        assert_eq!(Version::current().to_string(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn the_reply_tells_the_latest_release() {
        let body = r#"{"tag_name": "v0.2.0", "html_url": "https://example.com/elsewhere", "draft": false}"#;
        assert_eq!(judge(200, body), Reply::Latest(Version(0, 2, 0)));
        // 릴리스가 하나도 없으면 404가 온다.
        assert_eq!(judge(404, ""), Reply::Nothing);
        // 그 밖의 답과 읽을 수 없는 답은 묻지 못한 것으로 친다.
        assert_eq!(judge(403, ""), Reply::Failed);
        assert_eq!(judge(200, "<html>"), Reply::Failed);
        assert_eq!(judge(200, r#"{"tag_name": "nightly"}"#), Reply::Failed);
        assert_eq!(judge(200, r#"{"name": "0.2.0"}"#), Reply::Failed);
    }

    #[test]
    fn a_newer_release_is_remembered_until_it_is_installed() {
        let current = Version(0, 1, 0);
        let newer = Status::Newer(Version(0, 2, 0));
        assert_eq!(Status::Unknown.after(Reply::Latest(Version(0, 2, 0)), current), newer);
        assert_eq!(Status::Unknown.after(Reply::Latest(Version(0, 1, 0)), current), Status::Current);
        // 지금 쓰는 것보다 옛 버전이 가장 새 릴리스로 올라와 있어도 새 버전이라고 하지 않는다.
        assert_eq!(Status::Unknown.after(Reply::Latest(Version(0, 0, 9)), current), Status::Current);
        assert_eq!(Status::Unknown.after(Reply::Nothing, current), Status::Nothing);
        // 묻지 못했으면, 알고 있던 새 버전은 그대로 안다.
        assert_eq!(newer.after(Reply::Failed, current), newer);
        assert_eq!(Status::Current.after(Reply::Failed, current), Status::Failed);
        // 다시 켰을 때: 저장해 둔 것이 지금보다 새 버전일 때만 알고 시작한다.
        assert_eq!(Status::remembered(Some("0.2.0"), current), newer);
        assert_eq!(Status::remembered(Some("0.2.0"), Version(0, 2, 0)), Status::Unknown);
        assert_eq!(Status::remembered(Some("이상한 글"), current), Status::Unknown);
        assert_eq!(Status::remembered(None, current), Status::Unknown);
        assert_eq!((newer.newer(), Status::Current.newer()), (Some(Version(0, 2, 0)), None));
    }

    #[test]
    fn it_asks_once_a_day() {
        // 물은 적이 없으면 바로 묻는다.
        assert_eq!(wait(None, 1_000_000), 0);
        // 한 시간 전에 물었으면 23시간 뒤에 묻는다. 하루가 넘었으면 바로 묻는다.
        assert_eq!(wait(Some(1_000_000 - 3600), 1_000_000), 23 * 3600);
        assert_eq!(wait(Some(1_000_000 - EVERY - 5), 1_000_000), 0);
        // 적어 둔 때가 지금보다 뒤면 시계가 바뀐 것이다. 바로 묻는다.
        assert_eq!(wait(Some(2_000_000), 1_000_000), 0);
    }

    #[test]
    fn the_status_line_says_what_is_known() {
        assert_eq!(status_line(Status::Newer(Version(0, 2, 0)), true), "새 버전이 나왔습니다: 0.2.0");
        assert_ne!(status_line(Status::Unknown, true), status_line(Status::Unknown, false));
        let all = [Status::Unknown, Status::Asking, Status::Current, Status::Nothing, Status::Failed];
        assert!(all.iter().all(|&status| !status_line(status, true).is_empty()));
    }
}
