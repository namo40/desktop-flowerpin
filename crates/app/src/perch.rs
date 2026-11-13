//! 올라설 수 있는 창 윗변을 찾는다.
//!
//! 화면에 보이는 창들의 네모 범위만 보고 계산한다. Windows를 부르지 않으므로 단위 테스트로 확인할 수 있다.
//!
//! 올라설 수 있는 곳은 창의 윗변 가운데, 그 위에 선 몸 전체가 온전히 보이는 구간이다.
//! - 그 창보다 앞에 있는 다른 창과 몸이 조금이라도 겹치는 곳은 뺀다.
//! - 몸이 모니터의 위나 옆으로 나가는 곳은 뺀다.
//! - 윗변이 바닥과 거의 같은 높이면 뺀다. 올라설 까닭이 없다.

use crate::geom::Rect;

/// 윗변이 바닥보다 적어도 이만큼(몸 높이에 대한 비율) 높아야 올라설 곳으로 친다.
const MIN_RISE: f64 = 0.25;

/// 화면에 보이는 창 하나.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pane {
    /// 창을 가리키는 값. 같은 창인지 알아보는 데만 쓴다.
    pub id: isize,
    /// 화면에서 차지하는 범위.
    pub rect: Rect,
    /// 윗변에 올라설 수 있는 창인가. 작업표시줄은 가리기만 하고 올라설 수는 없다.
    pub standable: bool,
}

/// 선 몸이 발바닥 가운데에서 왼쪽, 오른쪽, 위로 차지하는 거리(화면 px).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Body {
    pub left: i32,
    pub right: i32,
    pub up: i32,
}

/// 올라설 수 있는 윗변 한 토막.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Perch {
    /// 어느 창의 윗변인지.
    pub window: isize,
    /// 윗변의 y. 발바닥이 여기에 놓인다.
    pub y: f64,
    /// 발바닥 가운데가 있을 수 있는 x의 범위.
    pub left: f64,
    pub right: f64,
    /// 창의 왼쪽 끝. 창이 옆으로 움직였을 때 얼마나 움직였는지 아는 데 쓴다.
    pub origin: f64,
}

impl Perch {
    /// 발바닥 가운데가 `x`일 때 이 토막 위인가.
    pub fn holds(&self, x: f64) -> bool {
        self.left - 0.5 <= x && x <= self.right + 0.5
    }

    /// `x`에서 이 토막까지의 가로 거리. 토막 위나 아래에 있으면 0이다.
    pub fn gap(&self, x: f64) -> f64 {
        (self.left - x).max(x - self.right).max(0.0)
    }
}

/// 올라설 수 있는 윗변을 모두 찾는다.
///
/// `panes`는 앞에 있는 창부터 차례로 준다. `stage`는 몸이 벗어나면 안 되는 범위(모니터)이고, 그 아래 끝이
/// 바닥이다.
pub fn find(panes: &[Pane], body: Body, stage: Rect) -> Vec<Perch> {
    let mut found = Vec::new();
    let lowest = stage.bottom() - (body.up as f64 * MIN_RISE).ceil() as i32;
    for (index, pane) in panes.iter().enumerate() {
        let y = pane.rect.y;
        // 머리가 모니터 위로 나가거나, 윗변이 바닥과 다름없으면 올라서지 않는다.
        if !pane.standable || y - body.up < stage.y || y > lowest {
            continue;
        }
        // 몸이 창의 양 끝과 모니터의 양 끝을 벗어나지 않는 범위.
        let mut spans = vec![(pane.rect.x.max(stage.x) + body.left, pane.rect.right().min(stage.right()) - body.right)];
        for front in &panes[..index] {
            // 앞에 있는 창이 선 몸의 높이(윗변 위로 `up`만큼)에 걸쳐 있으면, 그 창과 겹치는 자리를 뺀다.
            if front.rect.y <= y && front.rect.bottom() > y - body.up {
                let cut = (front.rect.x - body.right, front.rect.right() + body.left);
                spans = spans.into_iter().flat_map(|span| without(span, cut)).collect();
            }
        }
        for (left, right) in spans {
            if left <= right {
                found.push(Perch {
                    window: pane.id,
                    y: y as f64,
                    left: left as f64,
                    right: right as f64,
                    origin: pane.rect.x as f64,
                });
            }
        }
    }
    found
}

/// 범위 `span`에서 `cut`의 안쪽(양 끝은 남긴다)을 뺀 나머지.
fn without(span: (i32, i32), cut: (i32, i32)) -> impl Iterator<Item = (i32, i32)> {
    let (left, right) = span;
    let before = (left, right.min(cut.0));
    let after = (left.max(cut.1), right);
    [before, after].into_iter().filter(|(from, to)| from <= to)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 몸: 발바닥 가운데에서 왼쪽 70, 오른쪽 80, 위로 200.
    const BODY: Body = Body { left: 70, right: 80, up: 200 };
    /// 모니터 2560×1392(작업표시줄 위까지).
    const STAGE: Rect = Rect::new(0, 0, 2560, 1392);

    fn window(id: isize, x: i32, y: i32, w: i32, h: i32) -> Pane {
        Pane { id, rect: Rect::new(x, y, w, h), standable: true }
    }

    fn spans(perches: &[Perch]) -> Vec<(isize, f64, f64, f64)> {
        perches.iter().map(|perch| (perch.window, perch.y, perch.left, perch.right)).collect()
    }

    #[test]
    fn a_lone_window_can_be_stood_on_from_end_to_end() {
        let perches = find(&[window(1, 400, 500, 800, 600)], BODY, STAGE);
        // 몸이 창의 양 끝을 벗어나지 않는 범위다.
        assert_eq!(spans(&perches), [(1, 500.0, 470.0, 1120.0)]);
        assert_eq!(perches[0].origin, 400.0);
    }

    #[test]
    fn no_room_for_the_head_means_no_perch() {
        // 윗변이 모니터 위 끝에서 199px 아래다. 몸 높이 200이 들어가지 않는다.
        assert!(find(&[window(1, 400, 199, 800, 600)], BODY, STAGE).is_empty());
        assert_eq!(find(&[window(1, 400, 200, 800, 600)], BODY, STAGE).len(), 1);
        // 최대화한 창.
        assert!(find(&[window(1, 0, 0, 2560, 1392)], BODY, STAGE).is_empty());
    }

    #[test]
    fn an_edge_at_the_floor_is_not_a_perch() {
        // 바닥(1392)보다 몸 높이의 4분의 1(50px)은 높아야 한다.
        assert!(find(&[window(1, 400, 1343, 800, 600)], BODY, STAGE).is_empty());
        assert_eq!(find(&[window(1, 400, 1342, 800, 600)], BODY, STAGE).len(), 1);
    }

    #[test]
    fn a_window_in_front_cuts_out_where_the_body_would_overlap_it() {
        // 뒤 창(2)의 윗변은 500. 앞 창(1)이 x 900~1300, y 300~700에 있다.
        let panes = [window(1, 900, 300, 400, 400), window(2, 400, 500, 1600, 600)];
        let perches = find(&panes, BODY, STAGE);
        // 앞 창의 윗변 전체, 그리고 뒤 창 윗변에서 몸이 앞 창과 겹치지 않는 양쪽 토막.
        assert_eq!(spans(&perches), [(1, 300.0, 970.0, 1220.0), (2, 500.0, 470.0, 820.0), (2, 500.0, 1370.0, 1920.0)]);
    }

    #[test]
    fn a_window_in_front_but_below_the_edge_does_not_matter() {
        // 앞 창이 뒤 창의 윗변보다 아래에서 시작한다. 선 몸과 겹치지 않는다.
        let panes = [window(1, 900, 501, 400, 400), window(2, 400, 500, 1600, 600)];
        assert_eq!(spans(&find(&panes, BODY, STAGE))[1], (2, 500.0, 470.0, 1920.0));
        // 윗변과 같은 높이에서 시작하면 그 자리의 윗변이 가려진 것이다.
        let panes = [window(1, 900, 500, 400, 400), window(2, 400, 500, 1600, 600)];
        assert_eq!(spans(&find(&panes, BODY, STAGE))[1..], [(2, 500.0, 470.0, 820.0), (2, 500.0, 1370.0, 1920.0)]);
    }

    #[test]
    fn a_window_behind_does_not_cut() {
        let panes = [window(1, 400, 500, 1600, 600), window(2, 900, 300, 400, 400)];
        let perches = find(&panes, BODY, STAGE);
        assert_eq!(spans(&perches)[0], (1, 500.0, 470.0, 1920.0));
        // 뒤 창의 윗변은 앞 창보다 높이 있어서 가려지지 않는다.
        assert_eq!(spans(&perches)[1], (2, 300.0, 970.0, 1220.0));
    }

    #[test]
    fn a_window_high_above_does_not_cut() {
        // 앞 창이 윗변에서 200px 넘게 위에서 끝난다. 선 몸에 닿지 않는다.
        let panes = [window(1, 900, 100, 400, 200), window(2, 400, 500, 1600, 600)];
        assert_eq!(spans(&find(&panes, BODY, STAGE)), [(2, 500.0, 470.0, 1920.0)]);
        // 1px이라도 걸치면 뺀다.
        let panes = [window(1, 900, 100, 400, 201), window(2, 400, 500, 1600, 600)];
        assert_eq!(find(&panes, BODY, STAGE).len(), 2);
    }

    #[test]
    fn a_taskbar_covers_but_cannot_be_stood_on() {
        // 작업표시줄을 위쪽에 둔 것처럼, 올라설 수 없는 것이 윗변 위를 가리고 있다.
        let bar = Pane { id: 9, rect: Rect::new(900, 300, 400, 400), standable: false };
        let perches = find(&[bar, window(2, 400, 500, 1600, 600)], BODY, STAGE);
        assert_eq!(spans(&perches), [(2, 500.0, 470.0, 820.0), (2, 500.0, 1370.0, 1920.0)]);
    }

    #[test]
    fn the_body_stays_inside_the_monitor() {
        // 창이 모니터의 왼쪽과 오른쪽으로 삐져나가 있다.
        let perches = find(&[window(1, -300, 500, 800, 600), window(2, 2300, 600, 800, 600)], BODY, STAGE);
        assert_eq!(spans(&perches), [(1, 500.0, 70.0, 420.0), (2, 600.0, 2370.0, 2480.0)]);
        // 옆 모니터에 있는 창은 올라설 곳이 아니다.
        assert!(find(&[window(1, -2000, 500, 800, 600)], BODY, STAGE).is_empty());
    }

    #[test]
    fn a_window_too_narrow_for_the_body_is_not_a_perch() {
        assert!(find(&[window(1, 400, 500, 149, 300)], BODY, STAGE).is_empty());
        // 몸 너비와 꼭 같으면 한 점에 설 수 있다.
        assert_eq!(spans(&find(&[window(1, 400, 500, 150, 300)], BODY, STAGE)), [(1, 500.0, 470.0, 470.0)]);
    }

    #[test]
    fn gap_is_the_sideways_distance() {
        let perch = Perch { window: 1, y: 500.0, left: 470.0, right: 1120.0, origin: 400.0 };
        assert_eq!((perch.gap(300.0), perch.gap(800.0), perch.gap(1200.0)), (170.0, 0.0, 80.0));
        assert!(perch.holds(470.0) && perch.holds(1120.4) && !perch.holds(1121.0));
    }
}
