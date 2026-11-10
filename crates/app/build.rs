//! 그림 팩과 그 키를 빌드에 넣는다.
//!
//! # 어느 팩을 넣나
//!
//! 1. 진짜 그림 팩: 팩 폴더에 `flowerpin.dfpk`가 있으면 그것을 넣는다. 팩 폴더는 환경 변수
//!    `DFP_PACK_DIR`로 정하고, 정하지 않으면 이 저장소 옆의 `../local`을 본다. 키는 환경 변수
//!    `DFP_PACK_KEY`(16진수 64글자)나 팩 폴더의 `pack.key`(32바이트)에서 읽는다.
//! 2. 대체 그림 팩: 진짜 팩이 없으면 저장소의 `placeholder/flowerpin.dfpk`를 개발 키로 넣는다.
//!    그래서 이 저장소만 받아도 빌드된다. 꽃핀 대신 도형이 나온다.
//!
//! 진짜 그림 팩과 그 키는 이 저장소에 없다. 팩은 빌드 폴더(`target/`)로만 복사되고, `target/`은 git이
//! 무시한다.
//!
//! # 키
//!
//! 키는 통째로 넣지 않는다. 빌드할 때마다 새로 뽑은 난수로 두 조각을 내서 `key_parts.rs`로 쓴다.
//! 빌드하면서 그 키로 팩이 열리는지도 확인한다. 키가 틀리면 실행해 보기 전에 여기서 멈춘다.
//!
//! # 공식 빌드
//!
//! `DFP_RELEASE=1`이면 공식 빌드다. 개발 키, 개발 키 번호로 만든 팩, 대체 그림 팩이면 빌드가 실패한다.

use std::env;
use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};

use dfp_pack::{DEV_KEY, Header, KEY_LEN, Pack, key};

const PACK_NAME: &str = "flowerpin.dfpk";
const KEY_NAME: &str = "pack.key";

fn main() {
    println!("cargo::rerun-if-env-changed=DFP_PACK_DIR");
    println!("cargo::rerun-if-env-changed=DFP_PACK_KEY");
    println!("cargo::rerun-if-env-changed=DFP_RELEASE");

    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR")).join("../..");

    let chosen_dir = env::var("DFP_PACK_DIR").ok().map(PathBuf::from);
    let dir = chosen_dir.clone().unwrap_or_else(|| root.join("../local"));
    let real_pack = dir.join(PACK_NAME);
    let key_file = dir.join(KEY_NAME);
    let placeholder = root.join("placeholder").join(PACK_NAME);
    for path in [&real_pack, &key_file, &placeholder] {
        println!("cargo::rerun-if-changed={}", path.display());
    }

    let (pack_path, pack_key) = if real_pack.is_file() {
        (real_pack, read_key(&key_file))
    } else if let Some(dir) = chosen_dir {
        // 폴더를 직접 정했는데 팩이 없으면 잘못 정한 것이다. 말없이 대체 그림으로 넘어가지 않는다.
        panic!("DFP_PACK_DIR({})에 {PACK_NAME}가 없습니다", dir.display());
    } else {
        println!("cargo::warning=대체 그림 팩으로 빌드합니다. 꽃핀 대신 도형이 나옵니다.");
        (placeholder, DEV_KEY)
    };

    let bytes = fs::read(&pack_path).unwrap_or_else(|e| panic!("{}: {e}", pack_path.display()));
    let header = Header::parse(&bytes).unwrap_or_else(|e| panic!("{}: {e}", pack_path.display()));
    if let Err(e) = Pack::open(&bytes, &pack_key) {
        panic!("{}: {e}. 키가 이 팩을 만든 키와 같은지 확인해 주세요", pack_path.display());
    }
    if env::var("DFP_RELEASE").is_ok_and(|value| value == "1")
        && let Err(problem) = key::check_release(&pack_key, &header)
    {
        panic!("{problem}");
    }
    fs::write(out.join("pack.dfpk"), &bytes).expect("그림 팩 복사");

    // 키를 두 조각으로 나눠 넣는다. 난수는 빌드할 때마다 새로 뽑는다.
    let mut mask = [0u8; KEY_LEN];
    getrandom::fill(&mut mask).expect("운영체제에서 난수를 얻지 못했습니다");
    let (a, b) = key::split(&pack_key, mask);
    let mut code = String::new();
    for (name, part) in [("KEY_A", a), ("KEY_B", b)] {
        writeln!(code, "static {name}: [u8; {KEY_LEN}] = {part:?};").expect("문자열에 쓰기");
    }
    fs::write(out.join("key_parts.rs"), code).expect("키 조각 쓰기");
}

/// 진짜 그림 팩의 키를 읽는다. 환경 변수가 먼저고, 없으면 팩 옆의 키 파일을 본다.
fn read_key(key_file: &Path) -> [u8; KEY_LEN] {
    if let Ok(text) = env::var("DFP_PACK_KEY") {
        return key::parse_hex(&text).expect("DFP_PACK_KEY는 16진수 64글자여야 합니다");
    }
    let bytes = fs::read(key_file)
        .unwrap_or_else(|e| panic!("그림 팩의 키가 없습니다. DFP_PACK_KEY나 {}로 주세요: {e}", key_file.display()));
    bytes.try_into().unwrap_or_else(|_| panic!("{}: 키 파일은 32바이트여야 합니다", key_file.display()))
}
