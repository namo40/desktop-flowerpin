//! 글자를 그림으로 만든다. 말풍선에 쓴다.
//!
//! 꽃핀과 말풍선의 창은 픽셀마다 투명도가 있는 창이라, Windows의 보통 글자 그리기(GDI)로 바로 그리면
//! 투명도가 채워지지 않는다. 그래서 따로 둔 흰 바탕에 검은 글자를 그리게 한 뒤, 얼마나 어두워졌는지를
//! "글자가 그 픽셀을 덮은 정도"로 읽어 온다. 색을 입히고 바탕과 섞는 일은 받아 간 쪽이 한다.
//!
//! 글자 가장자리는 회색 단계로 부드럽게 한다. 색을 나눠 쓰는 방식(ClearType)은 바탕이 정해져 있어야 해서
//! 쓰지 않는다.

use std::ffi::c_void;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::Graphics::Gdi::{
    ANTIALIASED_QUALITY, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, CreateFontW,
    DEFAULT_CHARSET, DIB_RGB_COLORS, DT_CALCRECT, DT_EDITCONTROL, DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE,
    DT_WORDBREAK, DeleteDC, DeleteObject, DrawTextW, GdiFlush, GetTextMetricsW, HDC, HFONT, SelectObject, SetBkMode,
    SetTextColor, TEXTMETRICW, TRANSPARENT,
};

use crate::win::wide;

/// 글꼴 이름과 크기(pt). 설정 창과 같은 글꼴이다.
const FACE: &str = "맑은 고딕";
const POINTS: i32 = 9;
/// 글자 굵기. 보통과 굵게.
const REGULAR: i32 = 400;
const BOLD: i32 = 700;

/// 제목 한 줄과 내용 몇 줄을 그린 것.
pub struct Label {
    pub width: i32,
    pub height: i32,
    /// 제목이 차지하는 높이. 이 아래부터가 내용이다. 제목이 없으면 0이다.
    pub title_height: i32,
    /// 픽셀마다 글자가 덮은 정도(0~255). 한 줄이 `width`개이고 위에서 아래로 놓인다.
    pub coverage: Vec<u8>,
}

/// 만들어 쓰고 버릴 GDI 물건들.
struct Canvas {
    dc: HDC,
    regular: HFONT,
    bold: HFONT,
}

impl Canvas {
    fn new(dpi: u32) -> Canvas {
        // 글꼴 높이는 음수로 주면 글자 높이(px)로 받아들인다.
        let height = -((POINTS * dpi as i32 + 36) / 72);
        let face = wide(FACE);
        let font = |weight: i32| unsafe {
            CreateFontW(
                height,
                0,
                0,
                0,
                weight,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                0,
                0,
                ANTIALIASED_QUALITY as u32,
                0,
                face.as_ptr(),
            )
        };
        Canvas { dc: unsafe { CreateCompatibleDC(null_mut()) }, regular: font(REGULAR), bold: font(BOLD) }
    }

    /// 글꼴의 한 줄 높이.
    fn line_height(&self, font: HFONT) -> i32 {
        let mut metrics: TEXTMETRICW = unsafe { std::mem::zeroed() };
        unsafe {
            SelectObject(self.dc, font);
            GetTextMetricsW(self.dc, &mut metrics);
        }
        metrics.tmHeight + metrics.tmExternalLeading
    }

    /// `text`를 너비 `width` 안에 그렸을 때 차지할 크기.
    fn measure(&self, font: HFONT, text: &[u16], width: i32, flags: u32) -> (i32, i32) {
        let mut rect = RECT { left: 0, top: 0, right: width, bottom: 0 };
        unsafe {
            SelectObject(self.dc, font);
            DrawTextW(self.dc, text.as_ptr(), -1, &mut rect, flags | DT_CALCRECT);
        }
        (rect.right.min(width), rect.bottom)
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            DeleteDC(self.dc);
            DeleteObject(self.regular);
            DeleteObject(self.bold);
        }
    }
}

/// 제목(굵게, 한 줄)과 내용(여러 줄)을 그린다.
///
/// 너비는 `max_width`를 넘지 않는다. 제목이 길면 끝을 "..."로 줄이고, 내용은 낱말 사이에서 줄을 바꾸다가
/// `max_lines`줄을 넘으면 마지막 줄의 끝을 "..."로 줄인다. `dpi`는 말풍선이 뜰 모니터의 DPI다.
pub fn label(title: &str, body: &str, dpi: u32, max_width: i32, max_lines: i32) -> Label {
    let canvas = Canvas::new(dpi);
    let (title_text, body_text) = (wide(title), wide(body));
    let title_flags = DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS;
    let body_flags = DT_WORDBREAK | DT_EDITCONTROL | DT_NOPREFIX | DT_END_ELLIPSIS;

    let (mut width, mut title_height) = (0, 0);
    if !title.is_empty() {
        (width, title_height) = canvas.measure(canvas.bold, &title_text, max_width, title_flags);
    }
    let mut body_height = 0;
    if !body.is_empty() {
        let (body_width, wanted) = canvas.measure(canvas.regular, &body_text, max_width, body_flags);
        width = width.max(body_width);
        body_height = wanted.min(canvas.line_height(canvas.regular) * max_lines);
    }
    // 제목과 내용 사이를 조금 띄운다.
    let gap = if title_height > 0 && body_height > 0 { (2 * dpi as i32 + 48) / 96 } else { 0 };
    let (width, height) = (width.max(1), (title_height + gap + body_height).max(1));

    // 위에서 아래로 놓이는 32비트 그림. 흰 바탕에 검은 글자를 그린다.
    let mut info: BITMAPINFO = unsafe { std::mem::zeroed() };
    info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth = width;
    info.bmiHeader.biHeight = -height;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB;
    let mut bits: *mut c_void = null_mut();
    let bitmap = unsafe { CreateDIBSection(canvas.dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0) };
    let mut coverage = vec![0u8; (width * height) as usize];
    if bitmap.is_null() || bits.is_null() {
        return Label { width, height, title_height: title_height + gap, coverage };
    }
    let pixels = unsafe { std::slice::from_raw_parts_mut(bits as *mut u32, (width * height) as usize) };
    pixels.fill(0x00FF_FFFF);
    unsafe {
        let old = SelectObject(canvas.dc, bitmap);
        SetBkMode(canvas.dc, TRANSPARENT as i32);
        SetTextColor(canvas.dc, 0);
        if title_height > 0 {
            let mut rect = RECT { left: 0, top: 0, right: width, bottom: title_height };
            SelectObject(canvas.dc, canvas.bold);
            DrawTextW(canvas.dc, title_text.as_ptr(), -1, &mut rect, title_flags);
        }
        if body_height > 0 {
            let top = title_height + gap;
            let mut rect = RECT { left: 0, top, right: width, bottom: top + body_height };
            SelectObject(canvas.dc, canvas.regular);
            DrawTextW(canvas.dc, body_text.as_ptr(), -1, &mut rect, body_flags);
        }
        GdiFlush();
        SelectObject(canvas.dc, old);
    }
    // 흰 바탕(255)에서 어두워진 만큼이 글자가 덮은 정도다. 회색 단계라 세 색이 같다.
    for (cover, pixel) in coverage.iter_mut().zip(pixels.iter()) {
        *cover = 255 - ((pixel >> 8) & 0xFF) as u8;
    }
    unsafe { DeleteObject(bitmap) };
    Label { width, height, title_height: title_height + gap, coverage }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 글자가 덮은 픽셀 수.
    fn inked(label: &Label, rows: std::ops::Range<i32>) -> usize {
        let (from, to) = ((rows.start * label.width) as usize, (rows.end * label.width) as usize);
        label.coverage[from..to].iter().filter(|&&cover| cover > 128).count()
    }

    #[test]
    fn a_title_and_a_body_are_both_drawn() {
        let label = label("방송 시작!", "오늘은 무엇을 할까요", 96, 220, 3);
        assert!(label.width > 40 && label.width <= 220, "{}", label.width);
        assert!(label.title_height > 8 && label.height > label.title_height + 8);
        assert_eq!(label.coverage.len(), (label.width * label.height) as usize);
        // 제목 자리와 내용 자리에 모두 글자가 있다.
        assert!(inked(&label, 0..label.title_height) > 30);
        assert!(inked(&label, label.title_height..label.height) > 30);
    }

    #[test]
    fn long_text_is_kept_within_the_width_and_the_line_limit() {
        let short = label("", "가나다", 96, 220, 3);
        let long = label("", &"아주 긴 내용입니다 ".repeat(40), 96, 220, 3);
        assert!(long.width <= 220);
        // 세 줄까지만 그린다. 한 줄짜리의 세 배를 넘지 않는다.
        assert!(long.height <= short.height * 3 + 2, "{} vs {}", long.height, short.height);
        assert!(long.height >= short.height * 3 - 2);
        // 제목은 길어도 한 줄이다.
        let title = label(&"긴 제목 ".repeat(30), "", 96, 220, 3);
        assert!(title.width <= 220 && title.height <= short.height + 4);
    }

    #[test]
    fn bigger_screen_scale_makes_bigger_letters() {
        let (normal, doubled) = (label("제목", "내용", 96, 220, 3), label("제목", "내용", 192, 440, 3));
        assert!(doubled.height > normal.height * 3 / 2);
    }

    #[test]
    fn nothing_to_say_still_gives_a_picture() {
        let empty = label("", "", 96, 220, 3);
        assert_eq!((empty.width, empty.height, empty.coverage.len()), (1, 1, 1));
    }
}
