//! 팩 쓰기. 형식은 crate 문서에 있다.
//!
//! 팩을 만드는 도구만 쓴다. 앱은 읽기만 하므로 `write` 기능을 켜지 않는다.

use std::collections::HashMap;

use chacha20poly1305::XChaCha20Poly1305;
use chacha20poly1305::aead::{AeadInOut, KeyInit};

use crate::{
    Character, FLAG_PLACEHOLDER, HEADER_AAD_LEN, HEADER_LEN, KEY_LEN, MAGIC, MOTION_FLAG_LOOP, NO_MOTION, NONCE_LEN,
    VERSION,
};

/// 팩에 넣을 장 하나.
#[derive(Clone, Debug)]
pub struct FrameSource {
    /// 장이 칸 안에서 차지하는 자리.
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    pub duration_ms: u16,
    /// 팔레트 번호. 한 줄이 `w`개이고 모두 `w × h`개다.
    pub indices: Vec<u8>,
}

/// 팩에 넣을 동작 하나.
#[derive(Clone, Debug)]
pub struct MotionSource {
    pub name: String,
    pub looping: bool,
    /// 이 동작 뒤에 이어지는 동작의 이름.
    pub next: Option<String>,
    pub launch_ms: Option<u16>,
    /// 256색 팔레트. 색마다 R, G, B, A이고 알파를 곱하지 않은 값이다.
    pub palette: [[u8; 4]; 256],
    pub frames: Vec<FrameSource>,
}

/// 팩에 넣을 것 전부.
#[derive(Clone, Debug)]
pub struct PackSource {
    pub character: Character,
    /// 진짜 그림이 아니라 코드로 그린 대체 그림이다.
    pub placeholder: bool,
    pub motions: Vec<MotionSource>,
}

#[derive(Clone, Debug)]
pub struct WriteOptions {
    pub key_id: u16,
    /// zstd 압축 단계. 높을수록 느리게 만들고 작게 나온다. 푸는 속도는 거의 같다.
    pub zstd_level: i32,
    /// 학습할 zstd 사전의 최대 크기(바이트). 0이면 사전을 쓰지 않는다.
    ///
    /// 장마다 따로 압축하면 장끼리 닮은 점을 살리지 못한다. 모든 장에서 자주 나오는 무늬를 사전에
    /// 모아 두면 장 하나만 압축해도 잘 줄어든다.
    pub dict_size: usize,
}

/// 만든 팩의 크기 내역.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteStats {
    pub frames: usize,
    /// 서로 다른 덩어리 수. 픽셀이 같은 장들은 덩어리 하나를 함께 쓴다.
    pub unique_chunks: usize,
    /// 모든 장의 팔레트 번호를 더한 크기.
    pub raw_bytes: usize,
    /// 덩어리 구역의 크기(암호화한 뒤).
    pub chunk_bytes: usize,
    pub dict_bytes: usize,
    /// 목차의 크기(암호화한 뒤).
    pub toc_bytes: usize,
    pub total_bytes: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum WriteError {
    /// `next`에 적힌 동작이 없다.
    UnknownNext {
        motion: String,
        next: String,
    },
    /// 장의 크기가 0이거나, 칸을 벗어나거나, 팔레트 번호 개수가 `w × h`와 다르다.
    BadFrame {
        motion: String,
        frame: usize,
    },
    /// 형식이 담을 수 있는 수를 넘었다(동작 수, 장 수, 이름 길이, 팩 크기).
    TooLarge(&'static str),
    /// zstd 사전을 학습하지 못했다. 장이 너무 적거나 작으면 그렇다.
    Dictionary(String),
    Compress(String),
    /// 운영체제에서 난수를 얻지 못했다.
    Random,
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteError::UnknownNext { motion, next } => {
                write!(f, "동작 {motion}의 다음 동작 {next}이(가) 없습니다")
            }
            WriteError::BadFrame { motion, frame } => write!(f, "동작 {motion}의 {frame}번 장이 올바르지 않습니다"),
            WriteError::TooLarge(what) => write!(f, "{what}이(가) 너무 큽니다"),
            WriteError::Dictionary(why) => write!(f, "zstd 사전을 학습하지 못했습니다: {why}"),
            WriteError::Compress(why) => write!(f, "압축하지 못했습니다: {why}"),
            WriteError::Random => f.write_str("난수를 얻지 못했습니다"),
        }
    }
}

impl std::error::Error for WriteError {}

fn random_nonce() -> Result<[u8; NONCE_LEN], WriteError> {
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::fill(&mut nonce).map_err(|_| WriteError::Random)?;
    Ok(nonce)
}

/// `plain`을 암호화하고 뒤에 태그를 붙인다.
fn seal(cipher: &XChaCha20Poly1305, nonce: &[u8; NONCE_LEN], aad: &[u8], mut plain: Vec<u8>) -> Vec<u8> {
    let tag = cipher
        .encrypt_inout_detached(nonce.into(), aad, plain.as_mut_slice().into())
        .expect("팩에 넣는 덩어리는 암호화할 수 있는 길이를 넘지 않는다");
    plain.extend_from_slice(&tag);
    plain
}

/// `source`를 `key`로 암호화한 팩으로 만든다.
pub fn write_pack(
    source: &PackSource,
    key: &[u8; KEY_LEN],
    options: &WriteOptions,
) -> Result<(Vec<u8>, WriteStats), WriteError> {
    let cell = source.character.cell as u32;
    if source.motions.len() >= NO_MOTION as usize {
        return Err(WriteError::TooLarge("동작 수"));
    }
    for motion in &source.motions {
        if motion.name.len() > u8::MAX as usize {
            return Err(WriteError::TooLarge("동작 이름"));
        }
        if motion.frames.len() > u16::MAX as usize {
            return Err(WriteError::TooLarge("동작의 장 수"));
        }
        for (index, frame) in motion.frames.iter().enumerate() {
            let fits = frame.w > 0
                && frame.h > 0
                && frame.x as u32 + frame.w as u32 <= cell
                && frame.y as u32 + frame.h as u32 <= cell
                && frame.indices.len() == frame.w as usize * frame.h as usize;
            if !fits {
                return Err(WriteError::BadFrame { motion: motion.name.clone(), frame: index });
            }
        }
    }
    let frames: Vec<&FrameSource> = source.motions.iter().flat_map(|motion| &motion.frames).collect();

    // 사전은 장을 하나 걸러 하나씩 보여 주고 학습시킨다. 이웃한 장은 거의 같아서 절반이면 충분하다.
    let dictionary = if options.dict_size == 0 {
        Vec::new()
    } else {
        let samples: Vec<&[u8]> = frames.iter().step_by(2).map(|frame| frame.indices.as_slice()).collect();
        zstd::dict::from_samples(&samples, options.dict_size).map_err(|e| WriteError::Dictionary(e.to_string()))?
    };
    let mut compressor = if dictionary.is_empty() {
        zstd::bulk::Compressor::new(options.zstd_level)
    } else {
        zstd::bulk::Compressor::with_dictionary(options.zstd_level, &dictionary)
    }
    .map_err(|e| WriteError::Compress(e.to_string()))?;

    // 장마다 따로 압축하고 암호화해 덩어리 구역에 잇는다. 픽셀이 같은 장은 먼저 만든 덩어리를 가리킨다.
    let cipher = XChaCha20Poly1305::new(key.into());
    let mut chunks = Vec::new();
    let mut placed: HashMap<&[u8], (u32, u32, [u8; NONCE_LEN])> = HashMap::new();
    let mut entries = Vec::with_capacity(frames.len());
    for frame in &frames {
        let place = match placed.get(frame.indices.as_slice()) {
            Some(place) => *place,
            None => {
                let compressed =
                    compressor.compress(&frame.indices).map_err(|e| WriteError::Compress(e.to_string()))?;
                let offset = u32::try_from(chunks.len()).map_err(|_| WriteError::TooLarge("팩"))?;
                let nonce = random_nonce()?;
                let sealed = seal(&cipher, &nonce, &offset.to_le_bytes(), compressed);
                let place = (offset, sealed.len() as u32, nonce);
                chunks.extend_from_slice(&sealed);
                placed.insert(&frame.indices, place);
                place
            }
        };
        entries.push(place);
    }
    u32::try_from(chunks.len()).map_err(|_| WriteError::TooLarge("팩"))?;

    let mut toc = Vec::new();
    let character = &source.character;
    for value in [character.cell, character.foot.0, character.foot.1, character.ledge_top, character.ledge_front] {
        toc.extend_from_slice(&value.to_le_bytes());
    }
    toc.extend_from_slice(&(source.motions.len() as u16).to_le_bytes());
    toc.extend_from_slice(&u32::try_from(frames.len()).map_err(|_| WriteError::TooLarge("장 수"))?.to_le_bytes());
    toc.extend_from_slice(&u32::try_from(dictionary.len()).map_err(|_| WriteError::TooLarge("사전"))?.to_le_bytes());
    for motion in &source.motions {
        let next = match &motion.next {
            None => NO_MOTION,
            Some(next) => source
                .motions
                .iter()
                .position(|m| &m.name == next)
                .ok_or_else(|| WriteError::UnknownNext { motion: motion.name.clone(), next: next.clone() })?
                as u16,
        };
        toc.push(motion.name.len() as u8);
        toc.extend_from_slice(motion.name.as_bytes());
        toc.push(if motion.looping { MOTION_FLAG_LOOP } else { 0 });
        toc.extend_from_slice(&next.to_le_bytes());
        toc.extend_from_slice(&(motion.frames.len() as u16).to_le_bytes());
        toc.extend_from_slice(&motion.launch_ms.unwrap_or(0).to_le_bytes());
    }
    for motion in &source.motions {
        toc.extend(motion.palette.iter().flatten());
    }
    toc.extend_from_slice(&dictionary);
    for (frame, (offset, len, nonce)) in frames.iter().zip(&entries) {
        for value in [frame.x, frame.y, frame.w, frame.h, frame.duration_ms] {
            toc.extend_from_slice(&value.to_le_bytes());
        }
        toc.extend_from_slice(&offset.to_le_bytes());
        toc.extend_from_slice(&len.to_le_bytes());
        toc.extend_from_slice(nonce);
    }

    // 머리의 앞부분을 목차의 덧붙인 값으로 넣어서, 머리(키 번호, 깃발)를 고치면 목차가 풀리지 않게 한다.
    let toc_nonce = random_nonce()?;
    let toc_len = u32::try_from(toc.len() + crate::TAG_LEN).map_err(|_| WriteError::TooLarge("목차"))?;
    let mut pack = Vec::with_capacity(HEADER_LEN + toc_len as usize + chunks.len());
    pack.extend_from_slice(MAGIC);
    pack.extend_from_slice(&VERSION.to_le_bytes());
    pack.extend_from_slice(&options.key_id.to_le_bytes());
    pack.extend_from_slice(&(if source.placeholder { FLAG_PLACEHOLDER } else { 0 }).to_le_bytes());
    pack.extend_from_slice(&toc_len.to_le_bytes());
    debug_assert_eq!(pack.len(), HEADER_AAD_LEN);
    let sealed_toc = seal(&cipher, &toc_nonce, &pack, toc);
    pack.extend_from_slice(&toc_nonce);
    debug_assert_eq!(pack.len(), HEADER_LEN);
    pack.extend_from_slice(&sealed_toc);
    pack.extend_from_slice(&chunks);

    let stats = WriteStats {
        frames: frames.len(),
        unique_chunks: placed.len(),
        raw_bytes: frames.iter().map(|frame| frame.indices.len()).sum(),
        chunk_bytes: chunks.len(),
        dict_bytes: dictionary.len(),
        toc_bytes: sealed_toc.len(),
        total_bytes: pack.len(),
    };
    Ok((pack, stats))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DEV_KEY, Header, Pack, PackError, premultiply};

    const OPTIONS: WriteOptions = WriteOptions { key_id: 7, zstd_level: 3, dict_size: 0 };

    fn character() -> Character {
        Character { cell: 16, foot: (8, 14), ledge_top: 10, ledge_front: 13 }
    }

    /// 번호가 곧 색인 팔레트. `seed`에 따라 색이 다르다.
    fn palette(seed: u8) -> [[u8; 4]; 256] {
        std::array::from_fn(|i| [i as u8, seed, 255 - i as u8, if i == 0 { 0 } else { 200 }])
    }

    fn frame(x: u16, y: u16, w: u16, h: u16, duration_ms: u16, seed: u8) -> FrameSource {
        let indices = (0..w * h).map(|i| (i as u8).wrapping_mul(7).wrapping_add(seed)).collect();
        FrameSource { x, y, w, h, duration_ms, indices }
    }

    /// 동작 셋이 든 작은 팩 원본. 반복 동작, 다음 동작, 띄우는 시각, 픽셀이 같은 장이 모두 들어 있다.
    fn source() -> PackSource {
        let stand = MotionSource {
            name: "stand".into(),
            looping: true,
            next: None,
            launch_ms: None,
            palette: palette(1),
            frames: vec![frame(2, 3, 5, 6, 41, 0), frame(1, 1, 8, 9, 42, 1), frame(2, 3, 5, 6, 250, 0)],
        };
        let jump = MotionSource {
            name: "jump".into(),
            looping: false,
            next: Some("landing".into()),
            launch_ms: Some(750),
            palette: palette(2),
            frames: vec![frame(0, 0, 16, 16, 42, 9)],
        };
        let landing = MotionSource {
            name: "landing".into(),
            looping: false,
            next: Some("stand".into()),
            launch_ms: None,
            palette: palette(3),
            frames: vec![frame(4, 4, 3, 3, 83, 5), frame(15, 15, 1, 1, 208, 6)],
        };
        PackSource { character: character(), placeholder: false, motions: vec![stand, jump, landing] }
    }

    #[test]
    fn written_pack_reads_back_the_same() {
        let source = source();
        let (bytes, stats) = write_pack(&source, &DEV_KEY, &OPTIONS).unwrap();
        let (pack, mut decoder) = Pack::open(&bytes, &DEV_KEY).unwrap();

        assert_eq!(pack.header().key_id, 7);
        assert!(!pack.header().placeholder);
        assert_eq!(pack.character(), source.character);
        assert_eq!(pack.motions().len(), 3);
        assert_eq!(pack.motion_id("landing"), Some(2));
        assert_eq!(pack.motion_id("nothing"), None);

        let mut frame_id = 0;
        for (id, (motion, expected)) in pack.motions().iter().zip(&source.motions).enumerate() {
            assert_eq!(motion.name, expected.name);
            assert_eq!(motion.looping, expected.looping);
            assert_eq!(motion.next.map(|next| pack.motions()[next as usize].name.clone()), expected.next);
            assert_eq!(motion.launch_ms, expected.launch_ms);
            assert_eq!(motion.first_frame, frame_id);
            assert_eq!(motion.frame_count, expected.frames.len());
            assert!(motion.palette.iter().zip(&expected.palette).all(|(&read, &raw)| read == premultiply(raw)));
            for source_frame in &expected.frames {
                let frame = &pack.frames()[frame_id];
                assert_eq!(frame.motion as usize, id);
                assert_eq!(
                    (frame.x, frame.y, frame.w, frame.h, frame.duration_ms),
                    (source_frame.x, source_frame.y, source_frame.w, source_frame.h, source_frame.duration_ms)
                );
                let mut indices = vec![0u8; 16 * 16];
                let count = decoder.decode(&pack, frame_id, &mut indices).unwrap();
                assert_eq!(&indices[..count], source_frame.indices.as_slice());
                frame_id += 1;
            }
        }
        assert_eq!(frame_id, pack.frames().len());
        assert_eq!(stats.frames, 6);
        assert_eq!(stats.total_bytes, bytes.len());
    }

    #[test]
    fn same_pixels_share_one_chunk() {
        let (_, stats) = write_pack(&source(), &DEV_KEY, &OPTIONS).unwrap();
        // stand의 첫 장과 셋째 장은 픽셀이 같다.
        assert_eq!((stats.frames, stats.unique_chunks), (6, 5));
    }

    #[test]
    fn header_is_readable_without_the_key() {
        let mut source = source();
        source.placeholder = true;
        let (bytes, _) = write_pack(&source, &DEV_KEY, &OPTIONS).unwrap();
        let header = Header::parse(&bytes).unwrap();
        assert_eq!((header.key_id, header.placeholder), (7, true));
    }

    #[test]
    fn wrong_key_fails() {
        let (bytes, _) = write_pack(&source(), &DEV_KEY, &OPTIONS).unwrap();
        assert_eq!(Pack::open(&bytes, &[9; KEY_LEN]).err(), Some(PackError::Decrypt));
    }

    #[test]
    fn changed_header_fails() {
        let (bytes, _) = write_pack(&source(), &DEV_KEY, &OPTIONS).unwrap();
        // 키 번호(6)와 깃발(8)을 고치면 목차가 풀리지 않는다. "대체 그림"을 "진짜 그림"으로 속일 수 없다.
        for at in [6, 8] {
            let mut changed = bytes.clone();
            changed[at] ^= 1;
            assert_eq!(Pack::open(&changed, &DEV_KEY).err(), Some(PackError::Decrypt), "{at}");
        }
    }

    #[test]
    fn changed_chunk_fails() {
        let (mut bytes, _) = write_pack(&source(), &DEV_KEY, &OPTIONS).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        let (pack, mut decoder) = Pack::open(&bytes, &DEV_KEY).unwrap();
        let mut out = vec![0u8; 256];
        let results: Vec<_> = (0..pack.frames().len()).map(|i| decoder.decode(&pack, i, &mut out)).collect();
        assert!(results.contains(&Err(PackError::Decrypt)));
    }

    #[test]
    fn broken_files_are_rejected() {
        let (bytes, _) = write_pack(&source(), &DEV_KEY, &OPTIONS).unwrap();
        assert_eq!(Pack::open(&bytes[..20], &DEV_KEY).err(), Some(PackError::BadFormat));
        assert_eq!(Pack::open(&bytes[..crate::HEADER_LEN + 10], &DEV_KEY).err(), Some(PackError::BadFormat));
        let mut other_magic = bytes.clone();
        other_magic[0] = b'X';
        assert_eq!(Pack::open(&other_magic, &DEV_KEY).err(), Some(PackError::BadFormat));
        let mut other_version = bytes.clone();
        other_version[4] = 2;
        assert_eq!(Pack::open(&other_version, &DEV_KEY).err(), Some(PackError::UnsupportedVersion(2)));
        // 덩어리 구역이 잘린 팩은 장 표가 가리키는 자리가 없으므로 열리지 않는다.
        assert_eq!(Pack::open(&bytes[..bytes.len() - 1], &DEV_KEY).err(), Some(PackError::BadFormat));
    }

    #[test]
    fn nothing_readable_is_left_in_the_pack() {
        let source = source();
        let (bytes, _) = write_pack(&source, &DEV_KEY, &OPTIONS).unwrap();
        let contains = |needle: &[u8]| bytes.windows(needle.len()).any(|window| window == needle);
        assert!(!contains(b"stand") && !contains(b"landing"));
        let palette: Vec<u8> = source.motions[0].palette.iter().flatten().copied().take(64).collect();
        assert!(!contains(&palette));
        assert!(!contains(&source.motions[1].frames[0].indices[..32]));
    }

    #[test]
    fn bad_sources_are_rejected() {
        let mut unknown_next = source();
        unknown_next.motions[1].next = Some("nowhere".into());
        assert_eq!(
            write_pack(&unknown_next, &DEV_KEY, &OPTIONS).err(),
            Some(WriteError::UnknownNext { motion: "jump".into(), next: "nowhere".into() })
        );
        let mut outside = source();
        outside.motions[2].frames[1].x = 16;
        let bad = WriteError::BadFrame { motion: "landing".into(), frame: 1 };
        assert_eq!(write_pack(&outside, &DEV_KEY, &OPTIONS).err(), Some(bad));
        let mut short = source();
        short.motions[0].frames[0].indices.pop();
        let bad = WriteError::BadFrame { motion: "stand".into(), frame: 0 };
        assert_eq!(write_pack(&short, &DEV_KEY, &OPTIONS).err(), Some(bad));
    }

    #[test]
    fn dictionary_makes_small_frames_smaller() {
        // 닮은 장이 많은 동작. 장마다 같은 무늬가 자리만 조금씩 바뀌어 들어 있다.
        let frames: Vec<FrameSource> = (0..240u32)
            .map(|i| {
                let indices = (0..64 * 64u32)
                    .map(|p| {
                        let (x, y) = (p % 64, p / 64);
                        (((x + i) / 5 * 31 + y / 3 * 17 + (x * y + i * 7) % 11) % 251) as u8
                    })
                    .collect();
                FrameSource { x: 0, y: 0, w: 64, h: 64, duration_ms: 42, indices }
            })
            .collect();
        let motion = MotionSource {
            name: "wave".into(),
            looping: true,
            next: None,
            launch_ms: None,
            palette: palette(4),
            frames,
        };
        let source = PackSource {
            character: Character { cell: 64, foot: (32, 60), ledge_top: 0, ledge_front: 0 },
            placeholder: true,
            motions: vec![motion],
        };
        let plain = WriteOptions { key_id: 0, zstd_level: 19, dict_size: 0 };
        let trained = WriteOptions { dict_size: 16 * 1024, ..plain.clone() };
        let (_, without) = write_pack(&source, &DEV_KEY, &plain).unwrap();
        let (bytes, with) = write_pack(&source, &DEV_KEY, &trained).unwrap();
        assert!(with.dict_bytes > 0 && with.dict_bytes <= 16 * 1024);
        assert!(with.chunk_bytes < without.chunk_bytes, "{} < {}", with.chunk_bytes, without.chunk_bytes);

        // 사전으로 압축한 팩도 그대로 읽힌다.
        let (pack, mut decoder) = Pack::open(&bytes, &DEV_KEY).unwrap();
        let mut indices = vec![0u8; 64 * 64];
        for (id, frame) in source.motions[0].frames.iter().enumerate().step_by(37) {
            decoder.decode(&pack, id, &mut indices).unwrap();
            assert_eq!(indices, frame.indices);
        }
    }
}
