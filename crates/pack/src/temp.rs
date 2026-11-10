//! 임시 팩 읽기.
//!
//! 진짜 팩 형식과 팩 만드는 도구가 생기기 전에 시제품이 그림을 읽으려고 쓰는 형식이다.
//! 장마다 보여 줄 시간과 동작 이름이 들어 있지 않다. 진짜 형식이 생기면 이 파일은 지운다.
//!
//! 팩은 머리 36바이트만 빼고 모두 암호화되어 있다. 키가 없으면 팔레트도, 장도 읽을 수 없다.
//!
//! ```text
//! "DFPR", 판(u32) = 2, 목차 길이(u32, 암호문 + 태그), nonce 24바이트
//! 목차 (XChaCha20-Poly1305, 덧붙인 값은 "DFPR-toc")
//!   동작 수(u32), 장 수(u32), 사전 길이(u32)
//!   팔레트: 동작 수 × 256 × u32 (미리 곱한 알파 BGRA)
//!   zstd 사전
//!   장 표: 장 수 × 44바이트 [동작 u16, x u16, y u16, w u16, h u16, 0 u16, 덩어리 위치 u32, 덩어리 길이 u32, nonce 24바이트]
//! 덩어리들: 장마다 zstd로 압축한 팔레트 번호를 XChaCha20-Poly1305로 암호화한 것 + 태그 16바이트.
//!           덧붙인 값은 덩어리 위치(u32)
//! ```
//!
//! 숫자는 모두 리틀 엔디언이다.

use chacha20poly1305::aead::{AeadInOut, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

const MAGIC: &[u8; 4] = b"DFPR";
const VERSION: u32 = 2;
const HEADER_LEN: usize = 4 + 4 + 4 + NONCE_LEN;
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;
const TOC_AAD: &[u8] = b"DFPR-toc";
const TOC_COUNTS_LEN: usize = 12;
const PALETTE_LEN: usize = 256 * 4;
const FRAME_ENTRY_LEN: usize = 44;

#[derive(Debug, PartialEq, Eq)]
pub enum PackError {
    /// 팩 파일의 모양이 맞지 않는다.
    BadFormat,
    /// 없는 장 번호를 달라고 했다.
    NoSuchFrame,
    /// 암호를 풀지 못했다. 키가 다르거나 파일이 바뀌었다.
    Decrypt,
    /// 압축을 풀지 못했거나, 풀린 크기가 장 크기와 다르다.
    Decompress,
}

impl std::fmt::Display for PackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            PackError::BadFormat => "그림 팩의 형식이 맞지 않습니다",
            PackError::NoSuchFrame => "그림 팩에 없는 장입니다",
            PackError::Decrypt => "그림 팩의 암호를 풀지 못했습니다",
            PackError::Decompress => "그림 팩의 압축을 풀지 못했습니다",
        })
    }
}

impl std::error::Error for PackError {}

/// 장 하나가 칸(256×256) 안에서 차지하는 자리와, 팩 안에서 그 장을 찾는 데 쓰는 값.
#[derive(Clone, Debug)]
pub struct FrameInfo {
    pub motion: u16,
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    offset: u32,
    len: u32,
    nonce: [u8; NONCE_LEN],
}

/// 열어 둔 임시 팩. 목차(팔레트, 장 표)만 풀어서 들고 있고, 장 덩어리는 팩 바이트를 빌려 쓴다.
pub struct TempPack<'a> {
    palettes: Vec<[u32; 256]>,
    frames: Vec<FrameInfo>,
    blobs: &'a [u8],
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// 암호문과 태그가 붙어 있는 덩어리를 `out`에 푼다.
fn decrypt_into(
    cipher: &XChaCha20Poly1305,
    nonce: &[u8; NONCE_LEN],
    aad: &[u8],
    sealed: &[u8],
    out: &mut Vec<u8>,
) -> Result<(), PackError> {
    if sealed.len() < TAG_LEN {
        return Err(PackError::BadFormat);
    }
    let (ciphertext, tag) = sealed.split_at(sealed.len() - TAG_LEN);
    let tag: [u8; TAG_LEN] = tag.try_into().map_err(|_| PackError::BadFormat)?;
    let nonce: &XNonce = nonce.into();
    out.clear();
    out.extend_from_slice(ciphertext);
    cipher.decrypt_inout_detached(nonce, aad, out.as_mut_slice().into(), (&tag).into()).map_err(|_| PackError::Decrypt)
}

impl<'a> TempPack<'a> {
    /// 팩을 열고, 장을 꺼내는 도구를 함께 만든다. 목차는 여기서 한 번만 푼다.
    pub fn open(bytes: &'a [u8], key: &[u8; 32]) -> Result<(TempPack<'a>, FrameDecoder), PackError> {
        if bytes.len() < HEADER_LEN || &bytes[..4] != MAGIC || u32_at(bytes, 4) != VERSION {
            return Err(PackError::BadFormat);
        }
        let toc_len = u32_at(bytes, 8) as usize;
        let toc_nonce: [u8; NONCE_LEN] = bytes[12..HEADER_LEN].try_into().map_err(|_| PackError::BadFormat)?;
        let toc_end = HEADER_LEN.checked_add(toc_len).filter(|&end| end <= bytes.len()).ok_or(PackError::BadFormat)?;
        let cipher = XChaCha20Poly1305::new(key.into());
        let mut toc = Vec::new();
        decrypt_into(&cipher, &toc_nonce, TOC_AAD, &bytes[HEADER_LEN..toc_end], &mut toc)?;

        if toc.len() < TOC_COUNTS_LEN {
            return Err(PackError::BadFormat);
        }
        let motions = u32_at(&toc, 0) as usize;
        let frame_count = u32_at(&toc, 4) as usize;
        let dict_len = u32_at(&toc, 8) as usize;
        let palettes_end = TOC_COUNTS_LEN + motions * PALETTE_LEN;
        let dict_end = palettes_end + dict_len;
        if toc.len() != dict_end + frame_count * FRAME_ENTRY_LEN {
            return Err(PackError::BadFormat);
        }

        let (palette_chunks, _) = toc[TOC_COUNTS_LEN..palettes_end].as_chunks::<PALETTE_LEN>();
        let palettes = palette_chunks
            .iter()
            .map(|raw| {
                let mut palette = [0u32; 256];
                for (color, c) in palette.iter_mut().zip(raw.as_chunks::<4>().0) {
                    *color = u32::from_le_bytes(*c);
                }
                palette
            })
            .collect();
        let blobs = &bytes[toc_end..];
        let mut frames = Vec::with_capacity(frame_count);
        for e in toc[dict_end..].as_chunks::<FRAME_ENTRY_LEN>().0 {
            let info = FrameInfo {
                motion: u16_at(e, 0),
                x: u16_at(e, 2),
                y: u16_at(e, 4),
                w: u16_at(e, 6),
                h: u16_at(e, 8),
                offset: u32_at(e, 12),
                len: u32_at(e, 16),
                nonce: e[20..44].try_into().map_err(|_| PackError::BadFormat)?,
            };
            let end = info.offset as usize + info.len as usize;
            if info.motion as usize >= motions || (info.len as usize) < TAG_LEN || end > blobs.len() {
                return Err(PackError::BadFormat);
            }
            frames.push(info);
        }

        // 사전은 압축 풀기 도구가 제 것으로 복사해 가므로, 팩 쪽에는 남기지 않는다.
        let zstd = zstd::bulk::Decompressor::with_dictionary(&toc[palettes_end..dict_end])
            .map_err(|_| PackError::BadFormat)?;
        Ok((TempPack { palettes, frames, blobs }, FrameDecoder { cipher, zstd, compressed: Vec::new() }))
    }

    pub fn frames(&self) -> &[FrameInfo] {
        &self.frames
    }

    /// 동작의 팔레트. 색은 미리 곱한 알파의 BGRA(0xAARRGGBB)라, 창 버퍼에 그대로 쓸 수 있다.
    pub fn palette(&self, motion: u16) -> &[u32; 256] {
        &self.palettes[motion as usize]
    }
}

/// 팩에서 장을 꺼내는 도구. 버퍼와 압축 풀기 상태를 다시 쓰려고 하나를 만들어 계속 쓴다.
pub struct FrameDecoder {
    cipher: XChaCha20Poly1305,
    zstd: zstd::bulk::Decompressor<'static>,
    compressed: Vec<u8>,
}

impl FrameDecoder {
    /// `frame`번 장의 팔레트 번호(한 픽셀에 1바이트, `w × h`개)를 `out` 앞쪽에 풀고, 푼 개수를 돌려준다.
    pub fn decode(&mut self, pack: &TempPack<'_>, frame: usize, out: &mut [u8]) -> Result<usize, PackError> {
        let info = pack.frames.get(frame).ok_or(PackError::NoSuchFrame)?;
        let pixels = info.w as usize * info.h as usize;
        if out.len() < pixels {
            return Err(PackError::Decompress);
        }
        let sealed = &pack.blobs[info.offset as usize..info.offset as usize + info.len as usize];
        decrypt_into(&self.cipher, &info.nonce, &info.offset.to_le_bytes(), sealed, &mut self.compressed)?;
        let written =
            self.zstd.decompress_to_buffer(&self.compressed, &mut out[..pixels]).map_err(|_| PackError::Decompress)?;
        if written != pixels {
            return Err(PackError::Decompress);
        }
        Ok(pixels)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 32] = [7; 32];

    fn seal(nonce: [u8; NONCE_LEN], aad: &[u8], plain: &[u8]) -> Vec<u8> {
        let cipher = XChaCha20Poly1305::new((&KEY).into());
        let mut buffer = plain.to_vec();
        let tag = cipher.encrypt_inout_detached((&nonce).into(), aad, buffer.as_mut_slice().into()).unwrap();
        buffer.extend_from_slice(&tag);
        buffer
    }

    /// 동작 하나, 2×2 크기의 장 하나가 들어 있는 팩을 만든다. 장의 팔레트 번호는 `indices`다.
    fn tiny_pack(indices: [u8; 4]) -> Vec<u8> {
        let blob_nonce = [3u8; NONCE_LEN];
        let blob = seal(blob_nonce, &0u32.to_le_bytes(), &zstd::bulk::compress(&indices, 3).unwrap());

        let mut toc = Vec::new();
        for count in [1u32, 1, 0] {
            toc.extend_from_slice(&count.to_le_bytes());
        }
        for color in 0..256u32 {
            toc.extend_from_slice(&(0xFF00_0000 | color).to_le_bytes());
        }
        for v in [0u16, 10, 20, 2, 2, 0] {
            toc.extend_from_slice(&v.to_le_bytes());
        }
        toc.extend_from_slice(&0u32.to_le_bytes());
        toc.extend_from_slice(&(blob.len() as u32).to_le_bytes());
        toc.extend_from_slice(&blob_nonce);

        let toc_nonce = [5u8; NONCE_LEN];
        let sealed_toc = seal(toc_nonce, TOC_AAD, &toc);
        let mut pack = Vec::new();
        pack.extend_from_slice(MAGIC);
        pack.extend_from_slice(&VERSION.to_le_bytes());
        pack.extend_from_slice(&(sealed_toc.len() as u32).to_le_bytes());
        pack.extend_from_slice(&toc_nonce);
        pack.extend_from_slice(&sealed_toc);
        pack.extend_from_slice(&blob);
        pack
    }

    #[test]
    fn reads_back_a_frame() {
        let bytes = tiny_pack([1, 2, 3, 250]);
        let (pack, mut decoder) = TempPack::open(&bytes, &KEY).unwrap();
        assert_eq!(pack.frames().len(), 1);
        let info = &pack.frames()[0];
        assert_eq!((info.motion, info.x, info.y, info.w, info.h), (0, 10, 20, 2, 2));
        assert_eq!(pack.palette(0)[250], 0xFF00_00FA);

        let mut out = [0u8; 8];
        assert_eq!(decoder.decode(&pack, 0, &mut out), Ok(4));
        assert_eq!(&out[..4], &[1, 2, 3, 250]);
        assert_eq!(decoder.decode(&pack, 1, &mut out), Err(PackError::NoSuchFrame));
    }

    #[test]
    fn wrong_key_cannot_open() {
        let bytes = tiny_pack([0; 4]);
        assert_eq!(TempPack::open(&bytes, &[8; 32]).err(), Some(PackError::Decrypt));
    }

    #[test]
    fn changed_bytes_are_detected() {
        // 목차가 한 비트라도 바뀌면 열리지 않는다.
        let mut bytes = tiny_pack([0; 4]);
        bytes[HEADER_LEN + 3] ^= 1;
        assert_eq!(TempPack::open(&bytes, &KEY).err(), Some(PackError::Decrypt));

        // 장 덩어리가 바뀌면 그 장을 꺼낼 때 걸린다.
        let mut bytes = tiny_pack([0; 4]);
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        let (pack, mut decoder) = TempPack::open(&bytes, &KEY).unwrap();
        assert_eq!(decoder.decode(&pack, 0, &mut [0u8; 4]), Err(PackError::Decrypt));
    }

    #[test]
    fn rejects_wrong_magic_and_short_input() {
        assert_eq!(TempPack::open(&[0u8; 64], &KEY).err(), Some(PackError::BadFormat));
        assert_eq!(TempPack::open(b"DFPR", &KEY).err(), Some(PackError::BadFormat));
    }

    #[test]
    fn plain_text_is_not_visible() {
        // 팔레트의 색(0xFF0000FA 등)이 팩 바이트에 그대로 보이면 안 된다.
        let bytes = tiny_pack([1, 2, 3, 4]);
        let needle = 0xFF00_00FAu32.to_le_bytes();
        assert!(!bytes.windows(4).any(|w| w == needle));
    }
}
