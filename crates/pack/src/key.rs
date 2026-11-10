//! 팩을 암호화하는 키 다루기.
//!
//! - 키는 exe 안에 통째로 두지 않는다. 빌드할 때 두 조각으로 나눠 넣고(키 = A xor B), 팩을 열 때만
//!   합쳐 쓴 뒤 지운다.
//! - 공식 빌드는 개발 키나 대체 그림으로 만들어지면 안 된다. [`check_release`]가 그것을 잡는다.
//!
//! 코드가 공개라서 조각내기로 키를 숨길 수는 없다. exe를 문자열 검색이나 흔한 도구로 훑어서는
//! 키가 나오지 않게 하는 정도다. 그림을 지키는 마지막 수단은 라이선스다.

use zeroize::Zeroizing;

use crate::{DEV_KEY, DEV_KEY_ID, Header, KEY_LEN};

/// 16진수 글자 64개로 적은 키를 읽는다. 앞뒤 공백과 줄바꿈은 봐준다.
pub fn parse_hex(text: &str) -> Option<[u8; KEY_LEN]> {
    let text = text.trim().as_bytes();
    if text.len() != KEY_LEN * 2 {
        return None;
    }
    let digit = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let mut key = [0u8; KEY_LEN];
    for (byte, pair) in key.iter_mut().zip(text.as_chunks::<2>().0) {
        *byte = digit(pair[0])? << 4 | digit(pair[1])?;
    }
    Some(key)
}

/// 키를 두 조각으로 나눈다. `mask`는 빌드할 때마다 새로 뽑은 난수여야 한다.
///
/// 두 조각을 xor하면 키가 된다. 조각 하나만으로는 키에 대해 아무것도 알 수 없다.
pub fn split(key: &[u8; KEY_LEN], mask: [u8; KEY_LEN]) -> ([u8; KEY_LEN], [u8; KEY_LEN]) {
    let mut other = *key;
    for (byte, m) in other.iter_mut().zip(mask) {
        *byte ^= m;
    }
    (mask, other)
}

/// 두 조각을 합쳐 키를 만든다. 돌려준 키는 쓰고 버릴 때 메모리에서 지워진다.
pub fn join(a: &[u8; KEY_LEN], b: &[u8; KEY_LEN]) -> Zeroizing<[u8; KEY_LEN]> {
    let mut key = Zeroizing::new(*a);
    for (byte, other) in key.iter_mut().zip(b) {
        *byte ^= other;
    }
    key
}

/// 공식 빌드에 쓰면 안 되는 까닭.
#[derive(Debug, PartialEq, Eq)]
pub enum ReleaseProblem {
    /// 키가 공개된 개발 키다.
    DevKey,
    /// 팩이 개발 키 번호로 만들어졌다.
    DevKeyId,
    /// 팩이 진짜 그림이 아니라 대체 그림이다.
    Placeholder,
}

impl std::fmt::Display for ReleaseProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ReleaseProblem::DevKey => "공식 빌드에 개발 키를 쓸 수 없습니다",
            ReleaseProblem::DevKeyId => "공식 빌드에 개발 키 번호로 만든 그림 팩을 쓸 수 없습니다",
            ReleaseProblem::Placeholder => "공식 빌드에 대체 그림 팩을 쓸 수 없습니다",
        })
    }
}

/// 이 키와 팩으로 공식 빌드를 만들어도 되는지 본다.
pub fn check_release(key: &[u8; KEY_LEN], header: &Header) -> Result<(), ReleaseProblem> {
    if key == &DEV_KEY {
        return Err(ReleaseProblem::DevKey);
    }
    if header.key_id == DEV_KEY_ID {
        return Err(ReleaseProblem::DevKeyId);
    }
    if header.placeholder {
        return Err(ReleaseProblem::Placeholder);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Character, PackSource, WriteOptions, write_pack};

    const REAL_KEY: [u8; KEY_LEN] = [0x5A; KEY_LEN];

    fn header(key: &[u8; KEY_LEN], key_id: u16, placeholder: bool) -> Header {
        let character = Character { cell: 16, pivot: (8, 13), foot_y: 14, hold: (8, 7), ledge_top: 0, ledge_front: 0 };
        let source = PackSource { character, placeholder, motions: Vec::new() };
        let options = WriteOptions { key_id, zstd_level: 3, dict_size: 0 };
        Header::parse(&write_pack(&source, key, &options).unwrap().0).unwrap()
    }

    #[test]
    fn hex_key_is_parsed() {
        let text = "000102030405060708090a0b0c0d0e0f101112131415161718191A1B1C1D1E1F";
        let expected: [u8; KEY_LEN] = std::array::from_fn(|i| i as u8);
        assert_eq!(parse_hex(text), Some(expected));
        assert_eq!(parse_hex(&format!("  {text}\r\n")), Some(expected));
        assert_eq!(parse_hex(&text[..62]), None);
        assert_eq!(parse_hex(&format!("{text}00")), None);
        assert_eq!(parse_hex(&text.replace('a', "g")), None);
        assert_eq!(parse_hex(""), None);
    }

    #[test]
    fn split_key_joins_back() {
        let mask: [u8; KEY_LEN] = std::array::from_fn(|i| (i * 37 + 11) as u8);
        let (a, b) = split(&REAL_KEY, mask);
        assert_eq!(*join(&a, &b), REAL_KEY);
        // 어느 조각도 키와 같지 않다.
        assert_ne!(a, REAL_KEY);
        assert_ne!(b, REAL_KEY);
    }

    #[test]
    fn release_check_passes_a_real_pack() {
        assert_eq!(check_release(&REAL_KEY, &header(&REAL_KEY, 1, false)), Ok(()));
    }

    #[test]
    fn release_check_catches_dev_key_and_placeholder() {
        assert_eq!(check_release(&DEV_KEY, &header(&DEV_KEY, 1, false)), Err(ReleaseProblem::DevKey));
        assert_eq!(check_release(&REAL_KEY, &header(&REAL_KEY, DEV_KEY_ID, false)), Err(ReleaseProblem::DevKeyId));
        assert_eq!(check_release(&REAL_KEY, &header(&REAL_KEY, 1, true)), Err(ReleaseProblem::Placeholder));
    }
}
