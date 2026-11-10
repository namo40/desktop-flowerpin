//! 그림 팩을 푸는 키.
//!
//! 키는 exe 안에 통째로 들어 있지 않다. 빌드할 때 `build.rs`가 두 조각으로 나눠 넣고(키 = A xor B),
//! 팩을 열 때만 합쳐 쓴 뒤 지운다. 이것이 무엇을 막고 무엇을 못 막는지는 [`dfp_pack::key`]에 적혀 있다.

use std::hint::black_box;

use dfp_pack::KEY_LEN;

// `build.rs`가 만든 두 조각 `KEY_A`, `KEY_B`.
include!(concat!(env!("OUT_DIR"), "/key_parts.rs"));

/// 키를 합쳐 `open`에 넘긴다. 키는 `open`이 끝나면 메모리에서 지워진다.
pub fn with_pack_key<T>(open: impl FnOnce(&[u8; KEY_LEN]) -> T) -> T {
    // `black_box`를 거치지 않으면 컴파일러가 두 조각을 미리 합쳐서 exe에 키를 통째로 넣을 수 있다.
    let key = dfp_pack::key::join(black_box(&KEY_A), black_box(&KEY_B));
    open(&key)
}
