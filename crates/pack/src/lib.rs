//! DesktopFlowerpin 그림 팩(DFPK) 읽기와 쓰기.
//!
//! 팩은 캐릭터 하나의 모든 그림을 담은 파일이다. 앱은 팩을 exe 안에 넣어 두고, 보여 줄 장만 그때그때
//! 꺼낸다. 팩을 만드는 쪽(팩 도구)과 읽는 쪽(앱)이 이 crate 하나를 함께 쓴다.
//!
//! # 형식 (판 1)
//!
//! 숫자는 모두 리틀 엔디언이다.
//!
//! ```text
//! 머리 (40바이트, 암호화하지 않음)
//!    0  "DFPK"
//!    4  형식 판 (u16) = 1
//!    6  키 번호 (u16). 개발 키는 0
//!    8  깃발 (u32). 비트 0 = 대체 그림(코드로 그린 도형)
//!   12  목차 길이 (u32, 암호문 + 태그 16바이트)
//!   16  목차 nonce (24바이트)
//!
//! 목차 (XChaCha20-Poly1305로 암호화. 덧붙인 값은 머리의 앞 16바이트)
//!    칸 한 변 (u16), 발 자리 x, y (u16 둘), 걸터앉는 단의 윗면 y, 앞 모서리 y (u16 둘)
//!    동작 수 (u16), 장 수 (u32), 사전 길이 (u32)
//!    동작 표, 동작마다:
//!        이름 길이 (u8), 이름 (UTF-8)
//!        깃발 (u8). 비트 0 = 반복
//!        다음 동작 번호 (u16). 없으면 0xFFFF
//!        장 수 (u16). 장 표에서 동작 순서대로 이어져 있다
//!        띄우는 시각 ms (u16). 없으면 0
//!    팔레트, 동작마다 256색 × 4바이트 (R, G, B, A. 알파를 곱하지 않은 값)
//!    zstd 사전 (길이가 0이면 사전 없이 압축한 팩)
//!    장 표, 장마다 42바이트:
//!        칸 안의 자리 x, y, w, h (u16 넷), 보여 줄 시간 ms (u16)
//!        덩어리 위치 (u32, 덩어리 구역의 처음부터), 덩어리 길이 (u32, 암호문 + 태그), nonce (24바이트)
//!
//! 덩어리 구역
//!    장마다 팔레트 번호(픽셀마다 1바이트, w × h개)를 zstd로 압축해 XChaCha20-Poly1305로 암호화한 것.
//!    덧붙인 값은 덩어리 위치 (u32). 픽셀이 같은 장들은 같은 덩어리를 가리킨다.
//! ```
//!
//! 머리만 빼고 모두 암호화되어 있어서, 키가 없으면 동작 이름도 팔레트도 장도 읽을 수 없다.
//! 머리는 목차의 덧붙인 값으로 들어가므로, 머리를 고치면 목차가 풀리지 않는다.

pub mod key;
mod read;
#[cfg(feature = "write")]
mod write;

pub use read::{Character, Frame, FrameDecoder, Header, Motion, Pack};
#[cfg(feature = "write")]
pub use write::{FrameSource, MotionSource, PackSource, WriteError, WriteOptions, WriteStats, write_pack};

/// 팩을 암호화하는 키의 길이(바이트).
pub const KEY_LEN: usize = 32;

/// 개발 키. 누구나 볼 수 있는 키라서 아무것도 지켜 주지 않는다.
///
/// 대체 그림 팩(코드로 그린 도형)을 이 키로 만든다. 그래서 이 저장소만 받아도 빌드하고 실행할 수 있다.
/// 진짜 그림 팩은 공개하지 않는 키로 만들고, 공식 빌드는 개발 키를 쓰면 실패한다.
pub const DEV_KEY: [u8; KEY_LEN] = *b"DesktopFlowerpin dev key, public";

/// 개발 키의 키 번호.
pub const DEV_KEY_ID: u16 = 0;

pub(crate) const MAGIC: &[u8; 4] = b"DFPK";
pub(crate) const VERSION: u16 = 1;
pub(crate) const HEADER_LEN: usize = 40;
/// 머리 가운데 목차의 덧붙인 값으로 쓰는 앞부분의 길이(nonce 앞까지).
pub(crate) const HEADER_AAD_LEN: usize = 16;
pub(crate) const NONCE_LEN: usize = 24;
pub(crate) const TAG_LEN: usize = 16;
pub(crate) const FLAG_PLACEHOLDER: u32 = 1;
pub(crate) const MOTION_FLAG_LOOP: u8 = 1;
pub(crate) const NO_MOTION: u16 = 0xFFFF;
pub(crate) const PALETTE_LEN: usize = 256 * 4;
pub(crate) const FRAME_ENTRY_LEN: usize = 42;

#[derive(Debug, PartialEq, Eq)]
pub enum PackError {
    /// 팩 파일의 모양이 맞지 않는다.
    BadFormat,
    /// 이 코드가 모르는 형식 판이다.
    UnsupportedVersion(u16),
    /// 없는 장 번호를 달라고 했다.
    NoSuchFrame,
    /// 팩에 이 이름의 동작이 없다.
    NoSuchMotion(String),
    /// 암호를 풀지 못했다. 키가 다르거나 파일이 바뀌었다.
    Decrypt,
    /// 압축을 풀지 못했거나, 풀린 크기가 장 크기와 다르다.
    Decompress,
}

impl std::fmt::Display for PackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackError::BadFormat => f.write_str("그림 팩의 형식이 맞지 않습니다"),
            PackError::UnsupportedVersion(version) => write!(f, "그림 팩의 형식 판({version})을 읽을 수 없습니다"),
            PackError::NoSuchFrame => f.write_str("그림 팩에 없는 장입니다"),
            PackError::NoSuchMotion(name) => write!(f, "그림 팩에 {name} 동작이 없습니다"),
            PackError::Decrypt => f.write_str("그림 팩의 암호를 풀지 못했습니다"),
            PackError::Decompress => f.write_str("그림 팩의 압축을 풀지 못했습니다"),
        }
    }
}

impl std::error::Error for PackError {}

/// 팔레트 색 하나(R, G, B, A. 알파를 곱하지 않은 값)를 창 버퍼에 그대로 쓸 수 있는 값으로 바꾼다.
///
/// 결과는 미리 곱한 알파의 BGRA(0xAARRGGBB)다.
pub fn premultiply(color: [u8; 4]) -> u32 {
    let [r, g, b, a] = color.map(u32::from);
    let scale = |c: u32| (c * a + 127) / 255;
    a << 24 | scale(r) << 16 | scale(g) << 8 | scale(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiply_scales_color_by_alpha() {
        assert_eq!(premultiply([255, 128, 0, 255]), 0xFFFF_8000);
        assert_eq!(premultiply([255, 255, 255, 0]), 0);
        assert_eq!(premultiply([255, 100, 1, 128]), 0x8080_3201);
    }
}
