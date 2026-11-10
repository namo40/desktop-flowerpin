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

#[test]
fn guard_catches_what_it_should() {
    assert!(has_forbidden_extension("crates/app/idle.PNG"));
    assert!(has_forbidden_extension("pack0.bin"));
    assert!(!has_forbidden_extension("crates/app/src/main.rs"));
    assert!(is_allowed("placeholder/pack.dfpk"));
    assert!(!is_allowed("crates/placeholder/pack.dfpk"));
}
