//! 그림 팩과 그 키를 빌드에 넣는다.
//!
//! 지금은 시제품이 쓰던 임시 팩을 넣는다. 그림은 이 저장소에 없다. 임시 팩은 저장소 밖에
//! 있고, 있으면 exe 안에 넣는다. 없으면 `dfp_has_pack`을 켜지 않고, 앱은 시험 그림으로 실행된다.
//! 팩은 빌드 폴더(`target/`)로만 복사되고, `target/`은 git이 무시한다.
//!
//! 키는 통째로 넣지 않는다. 빌드할 때마다 새로 뽑은 난수로 두 조각을 내서 `key_parts.rs`로 쓴다.
//!
//! 환경 변수
//! - `DFP_TEMP_PACK_DIR`: 임시 팩이 있는 폴더(`temp_pack.bin`과 `key.bin`).
//! - `DFP_RELEASE=1`: 공식 빌드. 개발 키나 임시 팩으로는 빌드가 실패한다.

use std::env;
use std::fmt::Write;
use std::fs;
use std::path::PathBuf;

use dfp_pack::{DEV_KEY, KEY_LEN, key};

fn main() {
    println!("cargo::rerun-if-env-changed=DFP_TEMP_PACK_DIR");
    println!("cargo::rerun-if-env-changed=DFP_RELEASE");
    println!("cargo::rustc-check-cfg=cfg(dfp_has_pack)");

    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let dir = env::var("DFP_TEMP_PACK_DIR").map(PathBuf::from).unwrap_or_else(|_| manifest.join("../../../local/temp"));
    let (pack, key_file) = (dir.join("temp_pack.bin"), dir.join("key.bin"));
    println!("cargo::rerun-if-changed={}", pack.display());
    println!("cargo::rerun-if-changed={}", key_file.display());

    let has_pack = pack.is_file() && key_file.is_file();
    let pack_key: [u8; KEY_LEN] = if has_pack {
        fs::copy(&pack, out.join("pack.bin")).expect("임시 팩 복사");
        println!("cargo::rustc-cfg=dfp_has_pack");
        fs::read(&key_file).ok().and_then(|bytes| bytes.try_into().ok()).expect("임시 팩의 키는 32바이트여야 합니다")
    } else {
        println!("cargo::warning=임시 그림 팩이 없어 시험 그림으로 빌드합니다: {}", dir.display());
        DEV_KEY
    };

    if env::var("DFP_RELEASE").is_ok_and(|value| value == "1") {
        if pack_key == DEV_KEY {
            panic!("{}", key::ReleaseProblem::DevKey);
        }
        panic!("공식 빌드에 임시 그림 팩을 쓸 수 없습니다");
    }

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
