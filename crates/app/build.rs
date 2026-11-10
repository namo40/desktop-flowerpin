//! 시제품이 쓸 임시 그림 팩을 빌드에 넣는다.
//!
//! 그림은 이 저장소에 없다. 임시 팩은 저장소 밖에 있고, 있으면 exe 안에 넣는다.
//! 없으면 `dfp_has_pack`을 켜지 않고, 앱은 시험 그림으로 실행된다. 그래서 저장소만 받아도 빌드된다.
//!
//! 임시 팩은 전체가 암호화되어 있다. 팩과 키는 빌드 폴더(`target/`)로만 복사되고, `target/`은 git이 무시한다.
//!
//! 임시 팩의 자리는 환경 변수 `DFP_TEMP_PACK_DIR`로 바꿀 수 있다(`temp_pack.bin`과 `key.bin`이 있는 폴더).

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo::rerun-if-env-changed=DFP_TEMP_PACK_DIR");
    println!("cargo::rustc-check-cfg=cfg(dfp_has_pack)");

    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let dir = env::var("DFP_TEMP_PACK_DIR").map(PathBuf::from).unwrap_or_else(|_| manifest.join("../../../local/temp"));
    let (pack, key) = (dir.join("temp_pack.bin"), dir.join("key.bin"));
    println!("cargo::rerun-if-changed={}", pack.display());
    println!("cargo::rerun-if-changed={}", key.display());

    if pack.is_file() && key.is_file() {
        fs::copy(&pack, out.join("pack.bin")).expect("임시 팩 복사");
        fs::copy(&key, out.join("key.bin")).expect("임시 팩 키 복사");
        println!("cargo::rustc-cfg=dfp_has_pack");
    } else {
        println!("cargo::warning=임시 그림 팩이 없어 시험 그림으로 빌드합니다: {}", dir.display());
    }
}
