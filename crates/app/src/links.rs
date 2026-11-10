//! 바로 가기. 메뉴와 설정 창에서 고르면 기본 브라우저로 연다.
//!
//! 앱이 직접 인터넷에 연결하는 것이 아니라 브라우저를 여는 것이다. 주소는 앱 안에 고정해 두고,
//! 바뀌면 새 버전에서 고친다.

pub struct Link {
    /// 메뉴에 보이는 이름.
    pub name: &'static str,
    pub url: &'static str,
}

pub const LINKS: [Link; 4] = [
    Link { name: "방송 (치지직)", url: "https://chzzk.naver.com/a86031fdfeb1b223ad1172cf6ba4b94e" },
    Link { name: "유튜브", url: "https://www.youtube.com/@KoKopinya" },
    Link { name: "팬카페", url: "https://cafe.naver.com/cherrypach" },
    Link {
        name: "방송 시간표",
        url: "https://docs.google.com/spreadsheets/d/1Q6JSFLgTAoLosGAOCV-mjGA4mVIcoui4ENU-o7y4Wcw/edit?pli=1&gid=1607846358#gid=1607846358",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_https_and_distinct() {
        for (i, link) in LINKS.iter().enumerate() {
            assert!(link.url.starts_with("https://"), "{}", link.name);
            assert!(!link.name.is_empty());
            assert!(LINKS[..i].iter().all(|other| other.url != link.url && other.name != link.name));
        }
    }
}
