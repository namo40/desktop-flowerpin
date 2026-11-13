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

    /// 두 범위가 겹치는가. 맞닿기만 한 것은 겹친 것이 아니다. 빈 범위는 아무것과도 겹치지 않는다.
    pub fn overlaps(self, other: Rect) -> bool {
        !self.is_empty()
            && !other.is_empty()
            && self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    /// 두 범위를 모두 덮는 가장 작은 범위. 빈 범위는 없는 것으로 친다.
    pub fn union(self, other: Rect) -> Rect {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Rect { x, y, w: self.right().max(other.right()) - x, h: self.bottom().max(other.bottom()) - y }
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
    fn union_covers_both() {
        let a = Rect::new(10, 20, 30, 40);
        let b = Rect::new(0, 50, 20, 30);
        assert_eq!(a.union(b), Rect::new(0, 20, 40, 60));
    }

    #[test]
    fn union_ignores_empty() {
        let a = Rect::new(10, 20, 30, 40);
        assert_eq!(a.union(Rect::EMPTY), a);
        assert_eq!(Rect::EMPTY.union(a), a);
    }

    #[test]
    fn overlapping_needs_a_shared_pixel() {
        let a = Rect::new(10, 20, 30, 40);
        assert!(a.overlaps(Rect::new(39, 59, 5, 5)));
        // 맞닿기만 한 것과 빈 범위는 겹치지 않는다.
        assert!(!a.overlaps(Rect::new(40, 20, 5, 5)) && !a.overlaps(Rect::new(10, 60, 5, 5)));
        assert!(!a.overlaps(Rect::new(15, 25, 0, 5)) && !Rect::EMPTY.overlaps(a));
    }

    #[test]
    fn clip_keeps_inside() {
        assert_eq!(Rect::new(-5, -5, 20, 20).clip(10, 10), Rect::new(0, 0, 10, 10));
        assert_eq!(Rect::new(20, 20, 5, 5).clip(10, 10), Rect::EMPTY);
    }
}
