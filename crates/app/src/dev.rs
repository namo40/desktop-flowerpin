//! 만드는 동안 확인하고 재는 데 쓰는 실행 인자. 배포판의 기능이 아니다.
//!
//! ```text
//! DesktopFlowerpin.exe [--for 초] [--report 파일]
//! ```
//!
//! - `--for 초`: 정한 시간이 지나면 스스로 끝낸다.
//! - `--report 파일`: 확인한 값과 잰 값을 파일에 적는다. 콘솔이 없는 프로그램이라 파일로 남긴다.

use std::path::PathBuf;
use std::time::Duration;

use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::UI::WindowsAndMessaging::WindowFromPoint;

#[derive(Default)]
pub struct Options {
    pub run_for: Option<Duration>,
    pub report: Option<PathBuf>,
}

impl Options {
    pub fn from_args() -> Options {
        let args: Vec<String> = std::env::args().collect();
        let value = |key: &str| args.iter().position(|a| a == key).and_then(|i| args.get(i + 1)).cloned();
        Options {
            run_for: value("--for").and_then(|s| s.parse::<f64>().ok()).map(Duration::from_secs_f64),
            report: value("--report").map(PathBuf::from),
        }
    }
}

/// 보고서에 적을 줄을 모았다가 끝날 때 파일로 쓴다.
#[derive(Default)]
pub struct Report {
    lines: Vec<String>,
}

impl Report {
    pub fn add(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
    }

    pub fn write(&self, options: &Options) {
        if let Some(path) = &options.report {
            // 재는 데 쓰는 보조 기능이라, 쓰지 못해도 앱을 멈추지 않는다.
            let _ = std::fs::write(path, self.lines.join("\n") + "\n");
        }
    }
}

/// 화면의 한 점을 눌렀을 때 그 클릭을 `hwnd`가 받는지 알려 준다.
pub fn receives_click(hwnd: HWND, screen_x: i32, screen_y: i32) -> bool {
    unsafe { WindowFromPoint(POINT { x: screen_x, y: screen_y }) == hwnd }
}
