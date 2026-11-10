//! 저장소에 그림, 그림 팩, 키가 들어가지 않게 지키는 검사.
//!
//! 한 번이라도 커밋되면 지워도 git 기록에서 되살릴 수 있다. 그래서 지금 git이 따라가는 파일뿐 아니라
//! 지난 커밋에 있던 파일 이름까지 본다. 대체 그림 팩을 두는 `placeholder/` 폴더만 예외다.

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

fn is_allowed(path: &str) -> bool {
    path.starts_with("placeholder/")
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
}
