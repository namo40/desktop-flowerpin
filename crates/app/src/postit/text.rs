//! 글자와 아이콘을 그림으로 만든다.
//!
//! 포스트잇의 창은 픽셀마다 투명도가 있는 창이라 Windows의 글자 그리기(GDI)로 바로 그릴 수 없다.
//! 그래서 따로 둔 흰 바탕에 검은 글자를 그리게 한 뒤, 어두워진 정도를 "글자가 덮은 정도"로 읽어 온다.
//!
//! 포스트잇의 글자는 [`Painter`]가 그린다. 늘 반듯한 모양으로, 화면에 보이는 것보다 크게 그려 둔다.
//! 기울이고 키우는 것은 받아 간 쪽이 그 그림을 옮겨 찍으면서 한다.
//! - 작게 그려 둔 것을 늘리면 글자가 흐려지고 깨져 보인다. 그래서 크게 그려 두고 줄여 쓴다.
//! - 움직이는 모양마다 글자를 다시 그리면, Windows가 글자마다 자리를 픽셀에 맞추느라 글자들이 따로따로
//!   조금씩 움직여 꿀렁거려 보인다. 반듯한 모양으로 그리면 같은 글은 늘 같은 그림이 된다.
//!
//! 글꼴은 나눔고딕이다. 글꼴 파일을 실행 파일에 넣어 두고, 이 프로그램만 쓰게 Windows에 알린다.
//! 아이콘은 Windows에 들어 있는 아이콘 글꼴(Segoe MDL2 Assets)의 글자다.

use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::Once;

use windows_sys::Win32::Foundation::{RECT, SIZE};
use windows_sys::Win32::Graphics::Gdi::{
    ANTIALIASED_QUALITY, AddFontMemResourceEx, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CLEARTYPE_QUALITY,
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DEFAULT_CHARSET, DIB_RGB_COLORS, DT_CALCRECT, DT_CENTER,
    DT_NOCLIP, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW, FONT_QUALITY, GM_ADVANCED,
    GdiFlush, GetTextExtentPoint32W, GetTextMetricsW, HBITMAP, HDC, HFONT, HGDIOBJ, MWT_IDENTITY, ModifyWorldTransform,
    SelectObject, SetBkMode, SetGraphicsMode, SetTextColor, SetWorldTransform, TEXTMETRICW, TRANSPARENT, XFORM,
};

pub use windows_sys::Win32::Graphics::Gdi::{DT_EDITCONTROL, DT_WORDBREAK};

use crate::win::wide;

pub const FACE: &str = "NanumGothic";
const ICON_FACE: &str = "Segoe MDL2 Assets";

/// 나눔고딕(보통, 굵게). 네이버가 SIL Open Font License 1.1로 내놓은 글꼴이다.
/// 저작권 안내와 라이선스 전문은 글꼴 파일 옆의 LICENSE-NanumGothic.txt에 있다.
const NANUM: [&[u8]; 2] = [
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/res/fonts/NanumGothic.ttf")),
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/res/fonts/NanumGothicBold.ttf")),
];

/// 넣어 둔 글꼴을 이 프로그램만 쓸 수 있게 Windows에 알린다. 여러 번 불러도 한 번만 한다.
/// PC에 설치되는 것이 아니고, 프로그램이 끝나면 사라진다.
///
/// 글꼴이 9MB쯤 되므로 시작할 때 올리지 않고, 포스트잇을 처음 그릴 때 올린다.
pub fn load_fonts() {
    static LOADED: Once = Once::new();
    LOADED.call_once(|| {
        for data in NANUM {
            let count = 0u32;
            unsafe { AddFontMemResourceEx(data.as_ptr() as *const c_void, data.len() as u32, null_mut(), &count) };
        }
    });
}

/// 글자 높이가 `pixels`인 글꼴. 다 쓰면 `DeleteObject`로 없애야 한다.
pub fn font(face: &str, pixels: i32, bold: bool) -> HFONT {
    font_with(face, pixels, bold, ANTIALIASED_QUALITY)
}

/// ClearType으로 그리는 글꼴. 불투명한 바탕에 반듯하게 쓰는 글자(글을 적는 칸)에 쓴다.
pub fn clear_font(face: &str, pixels: i32, bold: bool) -> HFONT {
    font_with(face, pixels, bold, CLEARTYPE_QUALITY)
}

/// 가장자리를 다듬는 방식(`quality`)을 골라 글꼴을 만든다.
fn font_with(face: &str, pixels: i32, bold: bool, quality: FONT_QUALITY) -> HFONT {
    let face = wide(face);
    let weight = if bold { 700 } else { 400 };
    // 높이를 음수로 주면 글자 높이(px)로 받아들인다.
    unsafe {
        CreateFontW(-pixels, 0, 0, 0, weight, 0, 0, 0, DEFAULT_CHARSET as u32, 0, 0, quality as u32, 0, face.as_ptr())
    }
}

/// 포스트잇에 쓰는 글꼴. 크기는 화면 배율 100%일 때의 픽셀이다.
#[derive(Clone, Copy)]
pub enum Font {
    /// 메모의 글.
    Body,
    /// 알림 시각.
    Label,
    /// 답 단추의 글.
    Answer,
    /// 단추의 아이콘.
    Icon,
    /// 알림 시각 앞의 작은 종.
    Bell,
}

/// 포스트잇의 글자와 아이콘을 그리는 곳. 그림 버퍼 하나를 모든 포스트잇이 돌려 쓴다.
pub struct Painter {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u32,
    pub width: i32,
    pub height: i32,
    fonts: [HFONT; 5],
}

impl Painter {
    pub fn new(width: i32, height: i32) -> Painter {
        let dc = unsafe { CreateCompatibleDC(null_mut()) };
        unsafe {
            // 크게 늘려 그리려면 이 방식이어야 한다. 줄 나눔은 원래 크기에서 셈한 그대로다.
            SetGraphicsMode(dc, GM_ADVANCED);
            SetBkMode(dc, TRANSPARENT as i32);
            SetTextColor(dc, 0);
        }
        let fonts = [
            font(FACE, 15, false),
            font(FACE, 12, true),
            font(FACE, 13, true),
            font(ICON_FACE, 16, false),
            font(ICON_FACE, 13, false),
        ];
        let mut painter =
            Painter { dc, bitmap: null_mut(), old: null_mut(), bits: null_mut(), width: 0, height: 0, fonts };
        painter.fit(width, height);
        painter
    }

    /// 버퍼가 `width`×`height`보다 작으면 키운다.
    pub fn fit(&mut self, width: i32, height: i32) {
        if width <= self.width && height <= self.height {
            return;
        }
        let (width, height) = (width.max(self.width), height.max(self.height));
        let mut info: BITMAPINFO = unsafe { std::mem::zeroed() };
        info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = width;
        info.bmiHeader.biHeight = -height;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        let mut bits: *mut c_void = null_mut();
        unsafe {
            let bitmap = CreateDIBSection(self.dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            let old = SelectObject(self.dc, bitmap);
            if self.bitmap.is_null() {
                self.old = old;
            } else {
                DeleteObject(self.bitmap);
            }
            self.bitmap = bitmap;
        }
        (self.bits, self.width, self.height) = (bits as *mut u32, width, height);
    }

    /// 새 그림을 시작한다. 왼쪽 위 `width`×`height`를 흰 바탕으로 지운다.
    ///
    /// 그 뒤로 그리는 것은 포스트잇의 좌표로 적는다. 포스트잇의 `origin`이 그림의 왼쪽 위에 오고,
    /// 길이 1이 `density` 픽셀이 된다. 기울이지 않는다.
    pub fn begin(&mut self, width: i32, height: i32, density: f32, origin: (f32, f32)) {
        self.fit(width, height);
        let stride = self.width as usize;
        let pixels = unsafe { std::slice::from_raw_parts_mut(self.bits, stride * self.height as usize) };
        for row in 0..height as usize {
            pixels[row * stride..row * stride + width as usize].fill(0x00FF_FFFF);
        }
        let form = XFORM {
            eM11: density,
            eM12: 0.0,
            eM21: 0.0,
            eM22: density,
            eDx: -origin.0 * density,
            eDy: -origin.1 * density,
        };
        unsafe { SetWorldTransform(self.dc, &form) };
    }

    /// `rect`(왼쪽, 위, 오른쪽, 아래. 포스트잇의 좌표) 안에 글을 그린다.
    pub fn text(&self, font: Font, text: &str, rect: (f32, f32, f32, f32), flags: u32) {
        let text = wide(text);
        let round = |value: f32| value.round() as i32;
        let mut area = RECT { left: round(rect.0), top: round(rect.1), right: round(rect.2), bottom: round(rect.3) };
        unsafe {
            SelectObject(self.dc, self.fonts[font as usize]);
            // 기울어진 채로는 Windows가 범위 밖을 잘라 주지 못한다. 넘친 것은 그리는 쪽에서 가린다.
            DrawTextW(self.dc, text.as_ptr(), -1, &mut area, flags | DT_NOPREFIX | DT_NOCLIP);
        }
    }

    /// 한 줄짜리 글이나 아이콘을 `rect`의 한가운데에 그린다.
    pub fn centered(&self, font: Font, text: &str, rect: (f32, f32, f32, f32)) {
        self.text(font, text, rect, DT_SINGLELINE | DT_CENTER | DT_VCENTER);
    }

    /// 한 줄짜리 글의 너비(포스트잇의 좌표).
    pub fn measure(&self, font: Font, text: &str) -> f32 {
        let text: Vec<u16> = text.encode_utf16().collect();
        let mut size = SIZE { cx: 0, cy: 0 };
        unsafe {
            ModifyWorldTransform(self.dc, null_mut(), MWT_IDENTITY);
            SelectObject(self.dc, self.fonts[font as usize]);
            GetTextExtentPoint32W(self.dc, text.as_ptr(), text.len() as i32, &mut size);
        }
        size.cx as f32
    }

    /// 메모의 글 한 줄의 높이.
    pub fn line_height(&self) -> f32 {
        let mut metrics: TEXTMETRICW = unsafe { std::mem::zeroed() };
        unsafe {
            ModifyWorldTransform(self.dc, null_mut(), MWT_IDENTITY);
            SelectObject(self.dc, self.fonts[Font::Body as usize]);
            GetTextMetricsW(self.dc, &mut metrics);
        }
        (metrics.tmHeight + metrics.tmExternalLeading) as f32
    }

    /// 그린 것. 한 줄이 `width`개다. 흰 바탕(0xFFFFFF)에서 어두워진 만큼이 글자가 덮은 정도다.
    pub fn pixels(&self) -> &[u32] {
        unsafe {
            GdiFlush();
            std::slice::from_raw_parts(self.bits, (self.width * self.height) as usize)
        }
    }
}

impl Drop for Painter {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
            self.fonts.iter().for_each(|&font| {
                DeleteObject(font);
            });
        }
    }
}

/// 한 줄짜리 글의 너비(px)만 잰다. 칸에 맞춰 글을 줄일 때 쓴다.
pub fn width(text: &str, pixels: i32, bold: bool) -> i32 {
    let font = font_with(FACE, pixels, bold, CLEARTYPE_QUALITY);
    let text = wide(text);
    unsafe {
        let dc = CreateCompatibleDC(null_mut());
        let old_font = SelectObject(dc, font);
        let mut rect = RECT { left: 0, top: 0, right: 4096, bottom: 0 };
        DrawTextW(dc, text.as_ptr(), -1, &mut rect, DT_SINGLELINE | DT_NOPREFIX | DT_CALCRECT);
        SelectObject(dc, old_font);
        DeleteDC(dc);
        DeleteObject(font);
        rect.right
    }
}

/// 미리 그려 둔 글자. 픽셀마다 빨강·초록·파랑이 덮인 정도(0~255)가 들어 있다.
/// 기울이지 않는 곳(알림 설정 판 같은 곳)에 쓴다.
#[derive(Default)]
pub struct Block {
    pub w: i32,
    pub h: i32,
    pub cover: Vec<[u8; 3]>,
}

/// 한 줄짜리 글이나 아이콘 하나를 그린다. `icon`이면 아이콘 글꼴을 쓴다.
///
/// 글자는 ClearType으로 그린다(픽셀 안의 빨강·초록·파랑을 따로 쓴다). 판은 바탕이 불투명하고 기울지 않아서
/// 그렇게 그릴 수 있고, 작은 글씨가 또렷하다. 아이콘은 색 번짐 없이 회색 농도로만 가장자리를 다듬는다.
pub fn label(text: &str, pixels: i32, bold: bool, icon: bool) -> Block {
    let (face, quality) = if icon { (ICON_FACE, ANTIALIASED_QUALITY) } else { (FACE, CLEARTYPE_QUALITY) };
    let font = font_with(face, pixels, bold, quality);
    let text = wide(text);
    let flags = DT_SINGLELINE | DT_NOPREFIX;
    unsafe {
        let dc = CreateCompatibleDC(null_mut());
        let old_font = SelectObject(dc, font);
        let mut rect = RECT { left: 0, top: 0, right: 4096, bottom: 0 };
        DrawTextW(dc, text.as_ptr(), -1, &mut rect, flags | DT_CALCRECT);
        let (width, height) = (rect.right.max(1), rect.bottom.max(1));
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = width;
        info.bmiHeader.biHeight = -height;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        let mut bits: *mut c_void = null_mut();
        let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        let mut block = Block { w: width, h: height, cover: vec![[0; 3]; (width * height) as usize] };
        if !bitmap.is_null() && !bits.is_null() {
            let pixels = std::slice::from_raw_parts_mut(bits as *mut u32, (width * height) as usize);
            pixels.fill(0x00FF_FFFF);
            let old_bitmap = SelectObject(dc, bitmap);
            SetBkMode(dc, TRANSPARENT as i32);
            SetTextColor(dc, 0);
            let mut rect = RECT { left: 0, top: 0, right: width, bottom: height };
            DrawTextW(dc, text.as_ptr(), -1, &mut rect, flags);
            GdiFlush();
            // 흰 바탕에 검은 글자로 그렸다. 색마다 어두워진 만큼이 글자가 덮은 정도다.
            let dark = |pixel: u32, shift: u32| (255 - ((pixel >> shift) & 0xFF)) as u8;
            for (cover, &pixel) in block.cover.iter_mut().zip(pixels.iter()) {
                *cover = [dark(pixel, 16), dark(pixel, 8), dark(pixel, 0)];
            }
            SelectObject(dc, old_bitmap);
            DeleteObject(bitmap);
        }
        SelectObject(dc, old_font);
        DeleteDC(dc);
        DeleteObject(font);
        block
    }
}
