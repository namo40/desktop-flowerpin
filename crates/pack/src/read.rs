//! 팩 읽기. 형식은 crate 문서에 있다.

use chacha20poly1305::aead::{AeadInOut, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

use crate::{
    FLAG_PLACEHOLDER, FRAME_ENTRY_LEN, HEADER_AAD_LEN, HEADER_LEN, KEY_LEN, MAGIC, MOTION_FLAG_LOOP, NO_MOTION,
    NONCE_LEN, PALETTE_LEN, PackError, TAG_LEN, VERSION, premultiply,
};

/// 팩의 머리. 암호화되어 있지 않아서 키 없이 읽을 수 있다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    /// 팩을 암호화한 키의 번호. 개발 키는 [`crate::DEV_KEY_ID`]다.
    pub key_id: u16,
    /// 진짜 그림이 아니라 코드로 그린 대체 그림이다.
    pub placeholder: bool,
    toc_len: usize,
    toc_nonce: [u8; NONCE_LEN],
}

impl Header {
    /// 팩의 앞부분을 읽는다.
    pub fn parse(bytes: &[u8]) -> Result<Header, PackError> {
        if bytes.len() < HEADER_LEN || &bytes[..4] != MAGIC {
            return Err(PackError::BadFormat);
        }
        let mut reader = Reader { bytes: &bytes[4..HEADER_LEN], at: 0 };
        let version = reader.u16()?;
        if version != VERSION {
            return Err(PackError::UnsupportedVersion(version));
        }
        let key_id = reader.u16()?;
        let flags = reader.u32()?;
        let toc_len = reader.u32()? as usize;
        let toc_nonce = reader.take(NONCE_LEN)?.try_into().map_err(|_| PackError::BadFormat)?;
        Ok(Header { key_id, placeholder: flags & FLAG_PLACEHOLDER != 0, toc_len, toc_nonce })
    }
}

/// 캐릭터 전체에 걸친 값. 좌표는 칸 안의 픽셀이다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Character {
    /// 칸 한 변의 길이. 모든 장은 이 칸 안의 좌표로 자리가 정해져 있다.
    pub cell: u16,
    /// 발바닥이 놓이는 점. 이 점을 바닥 위의 서 있을 자리에 맞춘다.
    pub foot: (u16, u16),
    /// 걸터앉는 그림이 올라앉은 단의 윗면이 시작하는 y.
    pub ledge_top: u16,
    /// 그 단의 앞 모서리 y.
    pub ledge_front: u16,
}

/// 동작 하나. 장들은 장 표에서 `first_frame`부터 `frame_count`개가 이어져 있다.
#[derive(Clone, Debug)]
pub struct Motion {
    pub name: String,
    /// 끝나면 처음부터 되풀이하는 동작이다.
    pub looping: bool,
    /// 이 동작이 끝난 뒤(반복 동작이면 그만둘 때) 이어지는 동작.
    pub next: Option<u16>,
    pub first_frame: usize,
    pub frame_count: usize,
    /// 몸을 띄우기 시작하는 시각(ms). 점프 동작에만 있다.
    pub launch_ms: Option<u16>,
    /// 이 동작의 팔레트. 미리 곱한 알파의 BGRA(0xAARRGGBB)라, 창 버퍼에 그대로 쓸 수 있다.
    pub palette: [u32; 256],
}

/// 장 하나가 칸 안에서 차지하는 자리와 보여 줄 시간.
#[derive(Clone, Debug)]
pub struct Frame {
    pub motion: u16,
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    pub duration_ms: u16,
    offset: u32,
    len: u32,
    nonce: [u8; NONCE_LEN],
}

/// 열어 둔 팩. 목차(동작, 팔레트, 장 표)만 풀어서 들고 있고, 장 덩어리는 팩 바이트를 빌려 쓴다.
pub struct Pack<'a> {
    header: Header,
    character: Character,
    motions: Vec<Motion>,
    frames: Vec<Frame>,
    chunks: &'a [u8],
}

/// 바이트 열을 앞에서부터 읽어 나가는 도구. 모자라면 [`PackError::BadFormat`]을 돌려준다.
struct Reader<'b> {
    bytes: &'b [u8],
    at: usize,
}

impl<'b> Reader<'b> {
    fn take(&mut self, len: usize) -> Result<&'b [u8], PackError> {
        let end = self.at.checked_add(len).filter(|&end| end <= self.bytes.len()).ok_or(PackError::BadFormat)?;
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, PackError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, PackError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32, PackError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn is_done(&self) -> bool {
        self.at == self.bytes.len()
    }
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

impl<'a> Pack<'a> {
    /// 팩을 열고, 장을 꺼내는 도구를 함께 만든다. 목차는 여기서 한 번만 푼다.
    pub fn open(bytes: &'a [u8], key: &[u8; KEY_LEN]) -> Result<(Pack<'a>, FrameDecoder), PackError> {
        let header = Header::parse(bytes)?;
        let toc_end =
            HEADER_LEN.checked_add(header.toc_len).filter(|&end| end <= bytes.len()).ok_or(PackError::BadFormat)?;
        let cipher = XChaCha20Poly1305::new(key.into());
        let mut toc = Vec::new();
        decrypt_into(&cipher, &header.toc_nonce, &bytes[..HEADER_AAD_LEN], &bytes[HEADER_LEN..toc_end], &mut toc)?;
        let chunks = &bytes[toc_end..];

        let mut reader = Reader { bytes: &toc, at: 0 };
        let character = Character {
            cell: reader.u16()?,
            foot: (reader.u16()?, reader.u16()?),
            ledge_top: reader.u16()?,
            ledge_front: reader.u16()?,
        };
        let motion_count = reader.u16()? as usize;
        let frame_count = reader.u32()? as usize;
        let dict_len = reader.u32()? as usize;

        let mut motions = Vec::with_capacity(motion_count);
        let mut first_frame = 0;
        for _ in 0..motion_count {
            let name_len = reader.u8()? as usize;
            let name = std::str::from_utf8(reader.take(name_len)?).map_err(|_| PackError::BadFormat)?.to_string();
            let flags = reader.u8()?;
            let next = Some(reader.u16()?).filter(|&next| next != NO_MOTION);
            let count = reader.u16()? as usize;
            let launch_ms = Some(reader.u16()?).filter(|&ms| ms != 0);
            if next.is_some_and(|next| next as usize >= motion_count) {
                return Err(PackError::BadFormat);
            }
            let looping = flags & MOTION_FLAG_LOOP != 0;
            motions.push(Motion { name, looping, next, first_frame, frame_count: count, launch_ms, palette: [0; 256] });
            first_frame += count;
        }
        // 동작들의 장 수를 더하면 장 표의 길이와 같아야 한다.
        if first_frame != frame_count {
            return Err(PackError::BadFormat);
        }
        for motion in &mut motions {
            let raw = reader.take(PALETTE_LEN)?;
            for (color, c) in motion.palette.iter_mut().zip(raw.as_chunks::<4>().0) {
                *color = premultiply(*c);
            }
        }
        let dictionary = reader.take(dict_len)?;

        let mut frames = Vec::with_capacity(frame_count);
        for (id, motion) in motions.iter().enumerate() {
            for _ in 0..motion.frame_count {
                let entry = reader.take(FRAME_ENTRY_LEN)?;
                let mut e = Reader { bytes: entry, at: 0 };
                let frame = Frame {
                    motion: id as u16,
                    x: e.u16()?,
                    y: e.u16()?,
                    w: e.u16()?,
                    h: e.u16()?,
                    duration_ms: e.u16()?,
                    offset: e.u32()?,
                    len: e.u32()?,
                    nonce: e.take(NONCE_LEN)?.try_into().map_err(|_| PackError::BadFormat)?,
                };
                let inside_cell = frame.w > 0
                    && frame.h > 0
                    && frame.x as u32 + frame.w as u32 <= character.cell as u32
                    && frame.y as u32 + frame.h as u32 <= character.cell as u32;
                let chunk_end = frame.offset as usize + frame.len as usize;
                if !inside_cell || (frame.len as usize) < TAG_LEN || chunk_end > chunks.len() {
                    return Err(PackError::BadFormat);
                }
                frames.push(frame);
            }
        }
        if !reader.is_done() {
            return Err(PackError::BadFormat);
        }

        // 사전은 압축 풀기 도구가 제 것으로 복사해 가므로, 팩 쪽에는 남기지 않는다.
        let zstd = if dictionary.is_empty() {
            zstd::bulk::Decompressor::new()
        } else {
            zstd::bulk::Decompressor::with_dictionary(dictionary)
        }
        .map_err(|_| PackError::BadFormat)?;
        let pack = Pack { header, character, motions, frames, chunks };
        Ok((pack, FrameDecoder { cipher, zstd, compressed: Vec::new() }))
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn character(&self) -> Character {
        self.character
    }

    pub fn motions(&self) -> &[Motion] {
        &self.motions
    }

    /// 이름으로 동작 번호를 찾는다.
    pub fn motion_id(&self, name: &str) -> Option<u16> {
        self.motions.iter().position(|motion| motion.name == name).map(|id| id as u16)
    }

    pub fn frames(&self) -> &[Frame] {
        &self.frames
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
    pub fn decode(&mut self, pack: &Pack<'_>, frame: usize, out: &mut [u8]) -> Result<usize, PackError> {
        let info = pack.frames.get(frame).ok_or(PackError::NoSuchFrame)?;
        let pixels = info.w as usize * info.h as usize;
        if out.len() < pixels {
            return Err(PackError::Decompress);
        }
        let sealed = &pack.chunks[info.offset as usize..info.offset as usize + info.len as usize];
        decrypt_into(&self.cipher, &info.nonce, &info.offset.to_le_bytes(), sealed, &mut self.compressed)?;
        let written =
            self.zstd.decompress_to_buffer(&self.compressed, &mut out[..pixels]).map_err(|_| PackError::Decompress)?;
        if written != pixels {
            return Err(PackError::Decompress);
        }
        Ok(pixels)
    }
}
