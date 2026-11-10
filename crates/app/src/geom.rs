//! 창 버퍼 안의 네모 범위.

/// 픽셀 단위의 네모 범위. 오른쪽 끝(`x + w`)과 아래 끝(`y + h`)은 범위에 들어가지 않는다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const EMPTY: Rect = Rect { x: 0, y: 0, w: 0, h: 0 };

    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    pub const fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    pub const fn right(&self) -> i32 {
        self.x + self.w
    }

    pub const fn bottom(&self) -> i32 {
        self.y + self.h
    }

    /// `(0, 0)`에서 `(width, height)`까지의 범위 안으로 자른다.
    pub fn clip(self, width: i32, height: i32) -> Rect {
        let x = self.x.clamp(0, width);
        let y = self.y.clamp(0, height);
        let r = Rect { x, y, w: self.right().clamp(0, width) - x, h: self.bottom().clamp(0, height) - y };
        if r.is_empty() { Rect::EMPTY } else { r }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_keeps_inside() {
        assert_eq!(Rect::new(-5, -5, 20, 20).clip(10, 10), Rect::new(0, 0, 10, 10));
        assert_eq!(Rect::new(20, 20, 5, 5).clip(10, 10), Rect::EMPTY);
    }
}
