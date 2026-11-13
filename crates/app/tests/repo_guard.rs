//! 저장소에 그림, 그림 팩, 키가 들어가지 않게 지키는 검사.
//!
//! 한 번이라도 커밋되면 지워도 git 기록에서 되살릴 수 있다. 그래서 지금 git이 따라가는 파일뿐 아니라
//! 지난 커밋에 있던 파일 이름까지 본다. 예외는 셋뿐이다: 대체 그림 팩을 두는 `placeholder/` 폴더,
//! 바로 가기 단추에 쓰는 아이콘 셋([`LINK_ICONS`]), README에 쓰는 화면 그림들([`README_IMAGES`]).

use std::path::{Path, PathBuf};
use std::process::Command;

const FORBIDDEN_EXTENSIONS: &[&str] = &["png", "bmp", "gif", "jpg", "jpeg", "webp", "pak", "dfpk", "bin", "key"];

/// 파일 앞머리에 이 바이트가 있으면 그림이나 그림 팩이다. 확장자를 바꿔도 걸리게 한다.
const SIGNATURES: &[(&[u8], &str)] = &[
    (b"\x89PNG\r\n\x1a\n", "PNG 그림"),
    (b"GIF8", "GIF 그림"),
    (b"\xFF\xD8\xFF", "JPEG 그림"),
    (b"DFPR", "임시 그림 팩"),
    (b"DFPK", "그림 팩"),
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// git 명령의 출력을 줄마다 나눠 돌려준다. git이 없거나 저장소가 아니면 `None`이다.
fn git_lines(args: &[&str]) -> Option<Vec<String>> {
    let output = Command::new("git").args(args).current_dir(repo_root()).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).lines().map(str::to_owned).filter(|l| !l.is_empty()).collect())
}

/// 바로 가기 단추에 쓰는 아이콘. 치지직, 유튜브, 네이버 카페의 공식 아이콘을 크기만 맞춘 것이다.
/// 꽃핀 그림이 아니어서 저장소에 둔다. 이 이름의 파일만, 이 크기일 때만 받는다.
const LINK_ICONS: [&str; 3] =
    ["crates/app/res/icons/chzzk.png", "crates/app/res/icons/naver_cafe.png", "crates/app/res/icons/youtube.png"];
const LINK_ICON_SIDE: u32 = 96;

/// README에 쓰는 화면 그림과 그 너비, 높이. 실행 중인 프로그램의 화면을 찍은 것이다. 꽃핀이 찍혀 있지만
/// 그림 팩의 원본은 아니어서 저장소에 둔다. 이 이름의 파일만, 이 크기일 때만 받는다.
const README_IMAGES: [(&str, u32, u32); 7] = [
    ("docs/images/buttons-group.png", 660, 390),
    ("docs/images/buttons.png", 660, 390),
    ("docs/images/desktop.png", 1020, 780),
    ("docs/images/focus.png", 1020, 420),
    ("docs/images/hello.png", 960, 500),
    ("docs/images/studio.png", 756, 499),
    ("docs/images/together.png", 1020, 430),
];
const README_IMAGE_LIMIT: usize = 1024 * 1024;

fn is_allowed(path: &str) -> bool {
    path.starts_with("placeholder/")
        || LINK_ICONS.contains(&path)
        || README_IMAGES.iter().any(|(name, ..)| *name == path)
}

/// 폴더에 있는 파일을 저장소 뿌리에서 본 경로로, 이름 차례대로 돌려준다.
fn files_in(folder: &str) -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(repo_root().join(folder))
        .expect(folder)
        .map(|entry| format!("{folder}/{}", entry.expect("파일").file_name().to_string_lossy()))
        .collect();
    found.sort();
    found
}

/// PNG 파일의 바이트 수와 너비, 높이. 너비와 높이는 첫 덩어리(IHDR)에 있다.
fn read_png(path: &str) -> (usize, u32, u32) {
    let bytes = std::fs::read(repo_root().join(path)).expect(path);
    assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"), "{path}: PNG가 아님");
    let number = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().expect("4바이트"));
    (bytes.len(), number(16), number(20))
}

fn has_forbidden_extension(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| FORBIDDEN_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

#[test]
fn tracked_files_hold_no_art() {
    let Some(files) = git_lines(&["-c", "core.quotepath=off", "ls-files"]) else {
        eprintln!("git 저장소가 아니라 검사를 건너뜁니다.");
        return;
    };
    let mut problems = Vec::new();
    for path in files.iter().filter(|p| !is_allowed(p)) {
        if has_forbidden_extension(path) {
            problems.push(format!("{path}: 넣으면 안 되는 종류의 파일"));
            continue;
        }
        let Ok(bytes) = std::fs::read(repo_root().join(path)) else { continue };
        if let Some((_, what)) = SIGNATURES.iter().find(|(signature, _)| bytes.starts_with(signature)) {
            problems.push(format!("{path}: {what}"));
        }
    }
    assert!(problems.is_empty(), "저장소에 그림이나 팩이 들어 있습니다:\n{}", problems.join("\n"));
}

#[test]
fn history_holds_no_art() {
    let Some(files) = git_lines(&["-c", "core.quotepath=off", "log", "--all", "--name-only", "--format="]) else {
        eprintln!("git 저장소가 아니라 검사를 건너뜁니다.");
        return;
    };
    let mut problems: Vec<&String> = files.iter().filter(|p| !is_allowed(p) && has_forbidden_extension(p)).collect();
    problems.sort();
    problems.dedup();
    assert!(problems.is_empty(), "지난 커밋에 그림이나 팩이 들어간 적이 있습니다:\n{problems:#?}");
}

/// 아이콘 폴더에는 정해 둔 아이콘 셋만 있고, 모두 작은 정사각형 PNG다. 다른 그림을 이 폴더에 넣어 검사를
/// 비켜 가지 못하게 한다.
#[test]
fn icon_folder_holds_only_the_link_icons() {
    assert_eq!(files_in("crates/app/res/icons"), LINK_ICONS);
    for path in LINK_ICONS {
        let (bytes, width, height) = read_png(path);
        assert_eq!((width, height), (LINK_ICON_SIDE, LINK_ICON_SIDE), "{path}");
        assert!(bytes < 16 * 1024, "{path}: 아이콘치고 너무 큼");
    }
}

/// 화면 그림 폴더에는 정해 둔 그림만 있고, README가 그것을 모두 쓴다. 다른 그림을 이 폴더에 넣어 검사를
/// 비켜 가지 못하게 한다.
#[test]
fn image_folder_holds_only_the_readme_images() {
    let names: Vec<&str> = README_IMAGES.iter().map(|(name, ..)| *name).collect();
    assert_eq!(files_in("docs/images"), names);
    let readme = std::fs::read_to_string(repo_root().join("README.md")).expect("README.md");
    for (path, width, height) in README_IMAGES {
        let (bytes, found_width, found_height) = read_png(path);
        assert_eq!((found_width, found_height), (width, height), "{path}");
        assert!(bytes < README_IMAGE_LIMIT, "{path}: 화면 그림치고 너무 큼");
        assert!(readme.contains(&format!("]({path})")), "{path}: README에서 쓰지 않는 그림");
    }
}

/// `placeholder/`에는 대체 그림 팩만 둘 수 있다. 진짜 그림 팩을 여기에 잘못 넣으면 그대로 공개된다.
#[test]
fn placeholder_folder_holds_only_placeholder_packs() {
    let folder = repo_root().join("placeholder");
    let mut packs = 0;
    for entry in std::fs::read_dir(&folder).expect("placeholder 폴더") {
        let path = entry.expect("placeholder 폴더의 파일").path();
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default().to_string();
        if name == "README.md" {
            continue;
        }
        assert!(name.ends_with(".dfpk"), "placeholder/{name}: 대체 그림 팩이 아닌 파일");
        let bytes = std::fs::read(&path).expect("대체 그림 팩 읽기");
        let header = dfp_pack::Header::parse(&bytes).unwrap_or_else(|e| panic!("placeholder/{name}: {e}"));
        assert!(header.placeholder, "placeholder/{name}: 대체 그림 표시가 없는 팩");
        assert_eq!(header.key_id, dfp_pack::DEV_KEY_ID, "placeholder/{name}: 개발 키로 만들지 않은 팩");
        // 머리는 목차와 묶여 있어서, 개발 키로 열리면 위의 표시도 진짜다.
        assert!(dfp_pack::Pack::open(&bytes, &dfp_pack::DEV_KEY).is_ok(), "placeholder/{name}: 개발 키로 열리지 않음");
        packs += 1;
    }
    assert!(packs > 0, "placeholder 폴더에 대체 그림 팩이 없습니다");
}

#[test]
fn guard_catches_what_it_should() {
    assert!(has_forbidden_extension("crates/app/idle.PNG"));
    assert!(has_forbidden_extension("pack0.bin"));
    assert!(!has_forbidden_extension("crates/app/src/main.rs"));
    assert!(is_allowed("placeholder/pack.dfpk"));
    assert!(!is_allowed("crates/placeholder/pack.dfpk"));
    assert!(is_allowed("docs/images/hello.png"));
    assert!(!is_allowed("docs/images/other.png"));
    assert!(!is_allowed("docs/hello.png"));
}
