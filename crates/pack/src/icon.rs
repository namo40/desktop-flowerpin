//! 그림 팩의 그림으로 아이콘을 만든다.
//!
//! 트레이 아이콘과 창의 아이콘은 실행 중에 [`portrait`]로 만든 작은 그림을 쓴다. 실행 파일 자체의 아이콘은
//! 빌드할 때 같은 그림을 여러 크기로 뽑아 [`encode`]로 아이콘 파일(.ico)을 만들어 넣는다. 그래서 그림은
//! 저장소에 두지 않아도 된다.

use crate::{FrameDecoder, Pack, PackError};

/// `frame`번 장으로 만들 수 있는 가장 큰 아이콘의 한 변. 장의 너비와 높이 가운데 작은 쪽이다.
/// 이보다 크게 만들면 그림을 늘리게 되어 거칠어진다.
pub fn side(pack: &Pack<'_>, frame: usize) -> usize {
    let info = &pack.frames()[frame];
    (info.w as usize).min(info.h as usize)
}

/// `frame`번 장의 윗부분을 한 변이 `size`인 작은 그림으로 줄인다.
///
/// 장의 너비만큼을 위에서부터 정사각형으로 잘라 쓴다. 서 있는 그림이면 얼굴이 들어온다.
/// `indices`는 장을 꺼낼 때 쓰는 버퍼로, 칸 하나가 다 들어가는 크기여야 한다.
/// 결과는 미리 곱한 알파의 BGRA다.
pub fn portrait(
    pack: &Pack<'_>,
    decoder: &mut FrameDecoder,
    indices: &mut [u8],
    frame: usize,
    size: usize,
) -> Result<Vec<u32>, PackError> {
    decoder.decode(pack, frame, indices)?;
    let info = &pack.frames()[frame];
    let palette = &pack.motions()[info.motion as usize].palette;
    let (w, h) = (info.w as usize, info.h as usize);
    let side = w.min(h);
    let left = (w - side) / 2;
    let mut out = vec![0u32; size * size];
    for (i, pixel) in out.iter_mut().enumerate() {
        // 작은 그림의 픽셀 하나가 덮는 원본 범위를 평균한다.
        let (ox, oy) = (i % size, i / size);
        let (x0, x1) = (left + ox * side / size, left + ((ox + 1) * side).div_ceil(size));
        let (y0, y1) = (oy * side / size, ((oy + 1) * side).div_ceil(size));
        let mut sum = [0u32; 4];
        for y in y0..y1 {
            for &index in &indices[y * w + x0..y * w + x1] {
                let color = palette[index as usize];
                for (channel, total) in sum.iter_mut().enumerate() {
                    *total += (color >> (channel * 8)) & 255;
                }
            }
        }
        let count = ((x1 - x0) * (y1 - y0)) as u32;
        let mean = sum.map(|total| (total + count / 2) / count);
        *pixel = mean[3] << 24 | mean[2] << 16 | mean[1] << 8 | mean[0];
    }
    Ok(out)
}

/// 여러 크기의 그림을 아이콘 파일(.ico) 하나로 묶는다. 그림마다 한 변의 길이와, 미리 곱한 알파의 BGRA 픽셀이다.
///
/// 아이콘 파일은 색에 알파를 곱하지 않은 채로 담는다. 그래서 여기서 되돌린다.
pub fn encode(images: &[(usize, Vec<u32>)]) -> Vec<u8> {
    // 머리: 예약(0), 종류(1 = 아이콘), 그림 수. 그 뒤에 그림마다 16바이트짜리 목차가 온다.
    let mut file = vec![0u8, 0, 1, 0];
    file.extend((images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len();
    let mut bodies = Vec::with_capacity(images.len());
    for (size, pixels) in images {
        let size = *size;
        assert_eq!(pixels.len(), size * size, "아이콘 그림의 크기가 맞지 않습니다");
        // 투명한 곳을 적는 1비트 그림의 한 줄. 4바이트 단위로 맞춘다.
        let mask_row = size.div_ceil(32) * 4;
        let mut body = Vec::with_capacity(40 + size * size * 4 + mask_row * size);
        // 그림의 머리(BITMAPINFOHEADER). 높이는 색 그림과 투명 그림을 더한 것이라 두 배로 적는다.
        body.extend(40u32.to_le_bytes());
        body.extend((size as i32).to_le_bytes());
        body.extend((2 * size as i32).to_le_bytes());
        body.extend(1u16.to_le_bytes());
        body.extend(32u16.to_le_bytes());
        body.extend([0u8; 24]);
        // 색 그림. 아래 줄부터 적는다.
        for row in pixels.chunks(size).rev() {
            for &pixel in row {
                let alpha = pixel >> 24;
                let plain = |shift: u32| match alpha {
                    0 => 0,
                    _ => ((((pixel >> shift) & 255) * 255 + alpha / 2) / alpha).min(255) as u8,
                };
                body.extend([plain(0), plain(8), plain(16), alpha as u8]);
            }
        }
        // 투명 그림. 아주 투명한 곳만 1이다. 반쯤 투명한 곳은 색 그림의 알파가 맡는다.
        for row in pixels.chunks(size).rev() {
            let mut line = vec![0u8; mask_row];
            for (x, &pixel) in row.iter().enumerate() {
                if pixel >> 24 == 0 {
                    line[x / 8] |= 0x80 >> (x % 8);
                }
            }
            body.extend(line);
        }
        // 목차: 너비, 높이(256은 0으로 적는다), 색 수(0), 예약(0), 판 수(1), 픽셀당 비트(32), 길이, 자리.
        let edge = if size >= 256 { 0 } else { size as u8 };
        file.extend([edge, edge, 0, 0]);
        file.extend(1u16.to_le_bytes());
        file.extend(32u16.to_le_bytes());
        file.extend((body.len() as u32).to_le_bytes());
        file.extend((offset as u32).to_le_bytes());
        offset += body.len();
        bodies.push(body);
    }
    bodies.into_iter().for_each(|body| file.extend(body));
    file
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DEV_KEY;

    /// 저장소에 들어 있는 대체 그림 팩. 코드로 그린 도형이다.
    const PLACEHOLDER: &[u8] = include_bytes!("../../../placeholder/flowerpin.dfpk");

    fn number(bytes: &[u8], at: usize, len: usize) -> usize {
        bytes[at..at + len].iter().rev().fold(0, |value, &byte| value << 8 | byte as usize)
    }

    #[test]
    fn the_top_of_a_standing_frame_becomes_a_small_picture() {
        let (pack, mut decoder) = Pack::open(PLACEHOLDER, &DEV_KEY).expect("대체 그림 팩");
        let frame = pack.motions()[pack.motion_id("idle2").expect("idle2") as usize].first_frame;
        let cell = pack.character().cell as usize;
        let mut indices = vec![0u8; cell * cell];
        assert!(side(&pack, frame) >= 32);
        for size in [16, 32] {
            let small = portrait(&pack, &mut decoder, &mut indices, frame, size).expect("작은 그림");
            assert_eq!(small.len(), size * size);
            // 그림이 들어 있고, 색에는 알파가 곱해져 있다.
            assert!(small.iter().any(|pixel| pixel >> 24 == 255));
            for pixel in &small {
                let alpha = pixel >> 24;
                assert!([0, 8, 16].iter().all(|shift| (pixel >> shift) & 255 <= alpha), "{pixel:08X}");
            }
        }
    }

    #[test]
    fn pictures_are_packed_into_an_icon_file() {
        // 2x2: 불투명한 빨강, 반투명한 초록(알파가 곱해진 값), 투명, 불투명한 파랑.
        let small = vec![0xFFFF_0000, 0x8000_8000, 0x0000_0000, 0xFF00_00FF];
        let big: Vec<u32> = vec![0xFF10_2030; 40 * 40];
        let file = encode(&[(2, small), (40, big)]);
        // 머리와 목차.
        assert_eq!(file[..6], [0, 0, 1, 0, 2, 0]);
        let entry = |at: usize| &file[6 + 16 * at..22 + 16 * at];
        assert_eq!(entry(0)[..8], [2, 2, 0, 0, 1, 0, 32, 0]);
        assert_eq!(entry(1)[..8], [40, 40, 0, 0, 1, 0, 32, 0]);
        // 2x2 그림: 머리 40 + 색 16 + 투명 그림 2줄(4바이트씩).
        let (len, at) = (number(entry(0), 8, 4), number(entry(0), 12, 4));
        assert_eq!((len, at), (40 + 16 + 8, 6 + 32));
        // 40x40 그림: 투명 그림의 한 줄은 8바이트다. 앞 그림 바로 뒤에 온다.
        assert_eq!((number(entry(1), 8, 4), number(entry(1), 12, 4)), (40 + 40 * 40 * 4 + 8 * 40, at + len));
        assert_eq!(file.len(), at + len + 40 + 40 * 40 * 4 + 8 * 40);
        // 그림의 머리: 크기 40, 너비 2, 높이는 두 배인 4, 32비트.
        let body = &file[at..at + len];
        assert_eq!((number(body, 0, 4), number(body, 4, 4), number(body, 8, 4), number(body, 14, 2)), (40, 2, 4, 32));
        // 색은 아래 줄부터, 알파를 곱하지 않은 값으로 들어 있다: (투명, 파랑), (빨강, 초록).
        assert_eq!(body[40..56], [0, 0, 0, 0, 255, 0, 0, 255, 0, 0, 255, 255, 0, 255, 0, 128]);
        // 투명 그림: 아랫줄의 첫 픽셀만 투명하다.
        assert_eq!(body[56..64], [0x80, 0, 0, 0, 0, 0, 0, 0]);
        // 256짜리는 목차에 0으로 적는다.
        let large = encode(&[(256, vec![0; 256 * 256])]);
        assert_eq!(large[6..8], [0, 0]);
    }
}
