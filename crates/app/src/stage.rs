//! 바탕화면 무대. 모니터마다 바닥과 벽이 어디인지 알려 준다.
//!
//! - 바닥은 모니터의 작업 영역 아래쪽, 곧 작업표시줄 바로 위다. 작업표시줄이 위나 옆에 있거나
//!   자동으로 숨어도 작업 영역 아래쪽을 바닥으로 쓴다.
//! - 벽은 작업 영역의 왼쪽과 오른쪽 끝이다.
//! - 꽃핀은 자기가 있는 모니터에서 지낸다. 어느 모니터에도 들어 있지 않으면 가장 가까운 모니터로 친다.

use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{LPARAM, RECT};
use windows_sys::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;
use windows_sys::core::BOOL;

use crate::actor::Surroundings;
use crate::geom::Rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Monitor {
    /// 모니터 전체.
    pub area: Rect,
    /// 작업표시줄을 뺀 범위.
    pub work: Rect,
    /// 이 모니터의 DPI. 96이 Windows 배율 100%다.
    pub dpi: u32,
    pub primary: bool,
}

impl Monitor {
    /// 이 모니터에 있는 꽃핀 둘레의 상황.
    ///
    /// `margin`은 벽에서 떨어져 설 거리(몸이 화면 밖으로 나가지 않게), `headroom`은 발바닥에서
    /// 화면 위 끝까지 남겨 둘 거리(들어 올려도 머리가 화면 위로 나가지 않게)다.
    pub fn surroundings(&self, margin: f64, headroom: f64) -> Surroundings<'static> {
        let (left, right) = (self.work.x as f64 + margin, self.work.right() as f64 - margin);
        let floor = self.work.bottom() as f64;
        Surroundings {
            floor,
            // 모니터가 아주 좁으면 여백끼리 겹친다. 그때는 가운데에 선다.
            left: left.min((left + right) / 2.0),
            right: right.max((left + right) / 2.0),
            top: (self.area.y as f64 + headroom).min(floor),
            // 작업 영역 아래에 작업표시줄이 있으면 그 윗변이 걸터앉을 수 있는 단이다.
            ledge: self.work.bottom() < self.area.bottom(),
            // 다른 창의 윗변은 꽃핀마다 몸 크기가 달라서 꽃핀 쪽에서 넣는다.
            perches: &[],
        }
    }

    /// 점에서 이 모니터까지의 거리(제곱). 점이 모니터 안에 있으면 0이다.
    fn distance_squared(&self, x: i32, y: i32) -> i64 {
        let dx = (self.area.x - x).max(x - (self.area.right() - 1)).max(0) as i64;
        let dy = (self.area.y - y).max(y - (self.area.bottom() - 1)).max(0) as i64;
        dx * dx + dy * dy
    }
}

pub struct Stage {
    monitors: Vec<Monitor>,
}

impl Stage {
    /// 모니터가 하나도 없으면(그럴 일은 거의 없다) 흔한 크기의 모니터 하나가 있는 것으로 친다.
    pub fn new(mut monitors: Vec<Monitor>) -> Stage {
        if monitors.is_empty() {
            let area = Rect::new(0, 0, 1920, 1080);
            monitors.push(Monitor { area, work: area, dpi: 96, primary: true });
        }
        Stage { monitors }
    }

    /// 지금 연결된 모니터를 읽는다.
    pub fn read() -> Stage {
        let mut monitors: Vec<Monitor> = Vec::new();
        unsafe {
            EnumDisplayMonitors(null_mut(), std::ptr::null(), Some(add_monitor), &mut monitors as *mut _ as LPARAM)
        };
        Stage::new(monitors)
    }

    pub fn monitors(&self) -> &[Monitor] {
        &self.monitors
    }

    /// 몇 번째 모니터의 DPI를 정해 준 값으로 친다. 화면 배율이 서로 다른 모니터가 없어도 그런 경우를
    /// 확인해 보려고 둔다.
    pub fn pretend_dpi(mut self, pretend: &[(usize, u32)]) -> Stage {
        for &(index, dpi) in pretend {
            if let Some(monitor) = self.monitors.get_mut(index) {
                monitor.dpi = dpi;
            }
        }
        self
    }

    /// 주 모니터. 표시가 없으면 첫 모니터다.
    pub fn primary(&self) -> &Monitor {
        self.monitors.iter().find(|monitor| monitor.primary).unwrap_or(&self.monitors[0])
    }

    /// 점이 들어 있는 모니터. 어디에도 들어 있지 않으면 가장 가까운 모니터다.
    pub fn nearest(&self, x: i32, y: i32) -> &Monitor {
        self.monitors.iter().min_by_key(|monitor| monitor.distance_squared(x, y)).unwrap_or(&self.monitors[0])
    }

    /// 점이 어느 모니터 안에 있는가. 모니터를 뺐을 때 화면 밖에 남은 꽃핀을 찾는 데 쓴다.
    pub fn contains(&self, x: i32, y: i32) -> bool {
        self.monitors.iter().any(|monitor| monitor.distance_squared(x, y) == 0)
    }
}

unsafe extern "system" fn add_monitor(monitor: HMONITOR, _: HDC, _: *mut RECT, list: LPARAM) -> BOOL {
    let monitors = unsafe { &mut *(list as *mut Vec<Monitor>) };
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = size_of::<MONITORINFO>() as u32;
    if unsafe { GetMonitorInfoW(monitor, &mut info) } != 0 {
        let rect = |r: RECT| Rect::new(r.left, r.top, r.right - r.left, r.bottom - r.top);
        let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
        // 실패하면(오래된 Windows) 100%로 친다.
        unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
        monitors.push(Monitor {
            area: rect(info.rcMonitor),
            work: rect(info.rcWork),
            dpi: dpi_x,
            primary: info.dwFlags & MONITORINFOF_PRIMARY != 0,
        });
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 주 모니터(1920×1080, 아래에 작업표시줄 48px)와 그 왼쪽의 세로로 긴 모니터(1080×1920, 150%).
    fn two_monitors() -> Stage {
        Stage::new(vec![
            Monitor { area: Rect::new(0, 0, 1920, 1080), work: Rect::new(0, 0, 1920, 1032), dpi: 96, primary: true },
            Monitor {
                area: Rect::new(-1080, -400, 1080, 1920),
                work: Rect::new(-1080, -400, 1080, 1860),
                dpi: 144,
                primary: false,
            },
        ])
    }

    #[test]
    fn floor_is_the_bottom_of_the_work_area() {
        let stage = two_monitors();
        let around = stage.primary().surroundings(64.0, 172.0);
        assert_eq!((around.floor, around.left, around.right, around.top), (1032.0, 64.0, 1856.0, 172.0));
        // 아래에 작업표시줄이 있으므로 걸터앉을 수 있다.
        assert!(around.ledge);
        // 왼쪽 모니터는 바닥도 벽도 다르다.
        let around = stage.nearest(-500, 0).surroundings(96.0, 258.0);
        assert_eq!((around.floor, around.left, around.right, around.top), (1460.0, -984.0, -96.0, -142.0));
    }

    #[test]
    fn taskbar_on_top_still_uses_the_bottom() {
        let monitor =
            Monitor { area: Rect::new(0, 0, 1920, 1080), work: Rect::new(0, 48, 1920, 1032), dpi: 96, primary: true };
        let around = monitor.surroundings(64.0, 172.0);
        assert_eq!(around.floor, 1080.0);
        // 바닥이 화면의 아래 끝이라 걸터앉을 단이 없다. 앉으면 다리가 화면 밖으로 나간다.
        assert!(!around.ledge);
    }

    #[test]
    fn point_picks_its_monitor() {
        let stage = two_monitors();
        assert!(stage.nearest(100, 100).primary);
        assert!(!stage.nearest(-1, 100).primary);
        assert_eq!(stage.nearest(-500, 1500).dpi, 144);
        // 경계: 오른쪽 끝과 아래 끝은 그 모니터에 들어가지 않는다.
        assert!(stage.nearest(1919, 1079).primary);
        assert!(stage.contains(1919, 1079) && !stage.contains(1920, 500));
    }

    #[test]
    fn point_outside_every_monitor_goes_to_the_nearest() {
        let stage = two_monitors();
        assert!(!stage.contains(3000, 500));
        assert!(stage.nearest(3000, 500).primary);
        // 왼쪽 모니터의 아래쪽 바깥.
        assert!(!stage.contains(-500, 1600));
        assert_eq!(stage.nearest(-500, 1600).dpi, 144);
    }

    #[test]
    fn narrow_monitor_keeps_the_walls_in_order() {
        let monitor =
            Monitor { area: Rect::new(0, 0, 100, 800), work: Rect::new(0, 0, 100, 760), dpi: 96, primary: true };
        let around = monitor.surroundings(64.0, 172.0);
        assert_eq!((around.left, around.right), (50.0, 50.0));
    }

    #[test]
    fn no_monitor_falls_back_to_a_default() {
        let stage = Stage::new(Vec::new());
        assert_eq!(stage.primary().work, Rect::new(0, 0, 1920, 1080));
    }

    #[test]
    fn this_computer_has_a_primary_monitor() {
        let stage = Stage::read();
        assert!(!stage.monitors().is_empty());
        let primary = stage.primary();
        assert!(primary.work.w > 0 && primary.work.h > 0 && primary.dpi >= 96);
    }
}
