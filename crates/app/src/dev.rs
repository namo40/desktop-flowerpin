//! 만드는 동안 확인하고 재는 데 쓰는 실행 인자. 배포판의 기능이 아니다.
//!
//! ```text
//! DesktopFlowerpin.exe [--for 초] [--report 파일] [--check-screen] [--size 퍼센트] [--dpi 값] [--characters 수]
//! ```
//!
//! - `--for 초`: 정한 시간이 지나면 스스로 끝낸다.
//! - `--report 파일`: 확인한 값과 잰 값을 파일에 적는다. 콘솔이 없는 프로그램이라 파일로 남긴다.
//! - `--check-screen`: 화면에 실제로 보이는 그림이 창 버퍼와 같은지 가끔 읽어 본다.
//!   화면을 읽는 일이 무거워서, 자원을 잴 때는 함께 쓰지 않는다.
//! - `--size 퍼센트`: 꽃핀 크기(75, 100, 125). 설정 창이 생기기 전까지 크기를 바꿔 보는 방법이다.
//! - `--dpi 값`: 모니터 DPI를 이 값으로 친다(144면 배율 150%). 배율이 큰 모니터가 없어도
//!   그 배율의 그림을 확인할 수 있다.
//! - `--characters 수`: 꽃핀을 여러 명 띄운다. 여러 명일 때의 메모리와 CPU를 재는 데 쓴다.
//!
//! `--for`를 [`WARM_UP`]보다 길게 주면, 그 뒤부터 끝날 때까지 쓴 메모리와 CPU를 보고서에 적는다.

use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr::null_mut;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, GetDC, ReleaseDC, SRCCOPY, SelectObject,
};
use windows_sys::Win32::System::ProcessStatus::{
    K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2,
};
use windows_sys::Win32::System::Threading::GetCurrentProcess;
use windows_sys::Win32::System::WindowsProgramming::QueryProcessCycleTime;
use windows_sys::Win32::UI::WindowsAndMessaging::WindowFromPoint;

use crate::character_window::CharacterWindow;
use crate::geom::Rect;

pub struct Options {
    pub run_for: Option<Duration>,
    pub report: Option<PathBuf>,
    pub check_screen: bool,
    pub size: u32,
    pub dpi: Option<u32>,
    pub characters: usize,
}

impl Options {
    pub fn from_args() -> Options {
        let args: Vec<String> = std::env::args().collect();
        let value = |key: &str| args.iter().position(|a| a == key).and_then(|i| args.get(i + 1)).cloned();
        Options {
            run_for: value("--for").and_then(|s| s.parse::<f64>().ok()).map(Duration::from_secs_f64),
            report: value("--report").map(PathBuf::from),
            check_screen: args.iter().any(|a| a == "--check-screen"),
            size: value("--size").and_then(|s| s.parse().ok()).unwrap_or(100),
            dpi: value("--dpi").and_then(|s| s.parse().ok()),
            characters: value("--characters").and_then(|s| s.parse().ok()).unwrap_or(1).max(1),
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

/// 장이 제때 넘어가는지 재는 도구. 장을 넘길 때마다 시각과 걸린 시간을 받아 모은다.
/// 여러 명이면 모든 꽃핀의 장을 한 번 넘기는 것을 하나로 센다.
#[derive(Default)]
pub struct Pace {
    last: Option<Instant>,
    intervals_ms: Vec<f64>,
    /// 장을 꺼내 그려 화면에 넘기는 데 걸린 시간(µs). 여러 명이면 모든 꽃핀의 몫을 더한 값이다.
    costs_us: Vec<f64>,
}

impl Pace {
    /// `now`에 넘기기 시작한 장을 `took` 걸려 꺼내 그려 넘겼다고 적는다.
    pub fn frame_shown(&mut self, now: Instant, took: Duration) {
        if let Some(last) = self.last {
            self.intervals_ms.push((now - last).as_secs_f64() * 1000.0);
        }
        self.last = Some(now);
        self.costs_us.push(took.as_secs_f64() * 1e6);
    }

    /// 장 하나를 꺼내 그려 화면에 넘기는 데 걸린 시간(가운데 값과 평균)을 한 줄로 적는다.
    pub fn cost_summary(&self) -> String {
        if self.costs_us.is_empty() {
            return "넘긴 장 없음".to_string();
        }
        let mut sorted = self.costs_us.clone();
        sorted.sort_by(f64::total_cmp);
        let mean = sorted.iter().sum::<f64>() / sorted.len() as f64;
        format!(
            "장을 꺼내 그려 넘기는 데(모든 꽃핀의 몫) 가운데 값 {:.0} / 평균 {mean:.0} µs",
            sorted[sorted.len() / 2]
        )
    }

    /// 넘긴 장 수와 장 사이 간격(가장 짧은 것, 평균, 가장 긴 것)을 한 줄로 적는다.
    pub fn summary(&self) -> String {
        if self.intervals_ms.is_empty() {
            return "넘긴 장 없음".to_string();
        }
        let n = self.intervals_ms.len() as f64;
        let mean = self.intervals_ms.iter().sum::<f64>() / n;
        let min = self.intervals_ms.iter().copied().fold(f64::INFINITY, f64::min);
        let max = self.intervals_ms.iter().copied().fold(0.0, f64::max);
        let late = self.intervals_ms.iter().filter(|&&ms| ms > 50.0).count();
        format!(
            "넘긴 장 {}개, 장 사이 간격 최소 {min:.1} / 평균 {mean:.2} / 최대 {max:.1} ms, 50ms 넘게 걸린 장 {late}개",
            self.intervals_ms.len() + 1
        )
    }
}

/// 화면에 실제로 보이는 그림이 창 버퍼와 같은지 보는 도구.
///
/// 화면에 넘기는 호출은 성공했다고 돌려주면서도 화면을 바꾸지 않는 때가 있어서, 돌려준 값만으로는
/// 알 수 없다. 그래서 화면을 직접 읽어, 창 버퍼에서 완전히 불투명한 픽셀이 화면에서도 같은 색인지 센다.
/// 반투명하거나 투명한 픽셀은 뒤의 화면과 섞여 보이므로 견주지 않는다.
pub struct ScreenCheck {
    next: Instant,
    samples: u32,
    /// 지금까지 본 것 가운데 가장 낮은 일치 비율(0~1).
    worst: f64,
}

/// 화면을 읽어 보는 간격. 장을 넘기는 간격과 어긋나게 잡아서 여러 장을 고루 본다.
const CHECK_EVERY: Duration = Duration::from_millis(290);

impl ScreenCheck {
    pub fn new(now: Instant) -> ScreenCheck {
        ScreenCheck { next: now + CHECK_EVERY, samples: 0, worst: 1.0 }
    }

    /// 볼 때가 됐으면 화면을 읽어 창 버퍼와 견준다.
    ///
    /// 화면에 넘긴 그림은 다음 화면 갱신 때에야 보이므로, 넘긴 직후가 아니라 다음 장을 그리기
    /// 바로 전에 부른다.
    pub fn sample(&mut self, window: &CharacterWindow, now: Instant) {
        if now < self.next {
            return;
        }
        self.next = now + CHECK_EVERY;
        let Some(screen) = capture_screen(window.screen_rect()) else { return };
        let (mut total, mut same) = (0u32, 0u32);
        for (shown, seen) in window.shown().iter().zip(&screen) {
            if shown >> 24 == 255 {
                total += 1;
                same += u32::from(shown & 0x00FF_FFFF == seen & 0x00FF_FFFF);
            }
        }
        if total > 0 {
            self.samples += 1;
            self.worst = self.worst.min(f64::from(same) / f64::from(total));
        }
    }

    pub fn summary(&self) -> String {
        format!(
            "화면 확인 {}번, 불투명한 픽셀이 화면과 같은 비율은 가장 낮을 때 {:.1}%",
            self.samples,
            self.worst * 100.0
        )
    }
}

/// 화면의 `rect` 범위를 읽어 온다. 한 줄이 `rect.w`개이고 위에서 아래로 놓인다.
fn capture_screen(rect: Rect) -> Option<Vec<u32>> {
    let mut info: BITMAPINFO = unsafe { std::mem::zeroed() };
    info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth = rect.w;
    info.bmiHeader.biHeight = -rect.h;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB;
    unsafe {
        let screen = GetDC(null_mut());
        let mem = CreateCompatibleDC(screen);
        let mut bits: *mut c_void = null_mut();
        let bitmap = CreateDIBSection(mem, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        let mut pixels = None;
        if !bitmap.is_null() && !bits.is_null() {
            let old = SelectObject(mem, bitmap);
            // CAPTUREBLT를 줘야 꽃핀 창처럼 겹쳐 그리는 창도 함께 읽힌다.
            if BitBlt(mem, 0, 0, rect.w, rect.h, screen, rect.x, rect.y, SRCCOPY | CAPTUREBLT) != 0 {
                pixels = Some(std::slice::from_raw_parts(bits as *const u32, (rect.w * rect.h) as usize).to_vec());
            }
            SelectObject(mem, old);
            DeleteObject(bitmap);
        }
        DeleteDC(mem);
        ReleaseDC(null_mut(), screen);
        pixels
    }
}

/// 시작하고 이만큼 지난 뒤부터 CPU를 잰다. 창을 만들고 처음 그리는 몫을 빼려는 것이다.
pub const WARM_UP: Duration = Duration::from_secs(3);

/// 앱이 쓴 메모리와 CPU를 재는 도구. 만든 때부터 [`Usage::summary`]를 부를 때까지를 잰다.
///
/// CPU는 프로세스가 실제로 쓴 CPU 사이클 수로 잰다. 흔히 쓰는 `GetProcessTimes`는 약 15.6ms마다
/// 그 순간 돌고 있던 프로세스에 시간을 매기는데, 이 앱은 타이머에 맞춰 깨어나 잠깐 일하고 바로
/// 자므로 그 순간에 거의 걸리지 않아 0%로 나온다.
pub struct Usage {
    since: Instant,
    process_cycles: u64,
    clock_cycles: u64,
}

impl Usage {
    pub fn start() -> Usage {
        Usage { since: Instant::now(), process_cycles: process_cycles(), clock_cycles: clock_cycles() }
    }

    /// 메모리는 작업 관리자의 "메모리(활성 개인 작업 집합)"와 같은 값이다. CPU는 한 코어를 100%로 본다.
    pub fn summary(&self) -> String {
        let (private, working_set) = memory();
        let elapsed = self.since.elapsed().as_secs_f64();
        // 같은 동안 프로세스가 쓴 사이클 수를 시계가 센 사이클 수로 나누면 한 코어에서 차지한 몫이다.
        let used = (process_cycles() - self.process_cycles) as f64;
        let passed = (clock_cycles() - self.clock_cycles).max(1) as f64;
        let cpu = used / passed * 100.0;
        const MIB: f64 = 1024.0 * 1024.0;
        format!(
            "메모리(개인 작업 집합) {:.2}MiB, 작업 집합 {:.2}MiB, CPU {cpu:.2}% (한 코어 대비, {elapsed:.1}초 동안)",
            private as f64 / MIB,
            working_set as f64 / MIB
        )
    }
}

/// 이 프로세스의 모든 스레드가 지금까지 쓴 CPU 사이클 수(커널에서 쓴 것도 들어간다).
fn process_cycles() -> u64 {
    let mut cycles = 0;
    unsafe { QueryProcessCycleTime(GetCurrentProcess(), &mut cycles) };
    cycles
}

/// CPU의 사이클 시계. 프로세스가 쓴 사이클 수와 같은 단위로 흐른다.
fn clock_cycles() -> u64 {
    unsafe { std::arch::x86_64::_rdtsc() }
}

/// 이 프로세스의 개인 작업 집합과 작업 집합(바이트).
fn memory() -> (usize, usize) {
    let mut counters: PROCESS_MEMORY_COUNTERS_EX2 = unsafe { std::mem::zeroed() };
    counters.cb = size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32;
    let pointer = &mut counters as *mut PROCESS_MEMORY_COUNTERS_EX2 as *mut PROCESS_MEMORY_COUNTERS;
    unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), pointer, counters.cb) };
    (counters.PrivateWorkingSetSize, counters.WorkingSetSize)
}
