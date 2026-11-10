//! DesktopFlowerpin: 꽃핀이 바탕화면 위를 걸어 다니는 Windows 데스크탑 캐릭터.

// 콘솔 창을 띄우지 않는다.
#![windows_subsystem = "windows"]

mod character_window;
mod clock;
mod dev;
mod frames;
mod geom;
mod win;

use std::time::Instant;

use character_window::CharacterWindow;
use clock::WakeTimer;
use frames::Frames;
use geom::Rect;

/// 꽃핀의 발바닥이 놓이는 점(칸 좌표). 이 점을 바닥 위의 서 있을 자리에 맞춘다.
const FOOT: (i32, i32) = (128, 232);

/// 그림 팩 없이 빌드했을 때 띄우는 시험 그림의 한 변 길이.
const TEST_SIZE: i32 = 200;

fn main() {
    let options = dev::Options::from_args();
    let mut report = dev::Report::default();
    win::enable_per_monitor_dpi();

    // 서 있을 자리: 작업표시줄 바로 위, 화면 가운데.
    let work = win::primary_work_area();
    let floor = (work.x + work.w / 2, work.bottom());

    let mut frames = match Frames::load() {
        Ok(frames) => frames,
        Err(error) => win::fatal(&format!("{error}.")),
    };
    let _character = match &mut frames {
        Some(frames) => show_first_frame(frames, floor, &mut report),
        None => show_test_pattern(floor, &mut report),
    };

    let timer = WakeTimer::new();
    let deadline = options.run_for.map(|d| Instant::now() + d);
    loop {
        if !win::pump_messages() {
            break;
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            break;
        }
        timer.sleep_until(deadline);
    }
    report.write(&options);
}

/// 꽃핀 창을 만들고 idle2의 첫 장을 그려 띄운다.
fn show_first_frame(frames: &mut Frames, floor: (i32, i32), report: &mut dev::Report) -> CharacterWindow {
    // 창은 모든 장이 들어가는 크기로 한 번만 만든다. 창의 왼쪽 위는 칸 안의 `bounds` 왼쪽 위와 같다.
    let bounds = frames.bounds();
    let x = floor.0 - (FOOT.0 - bounds.x);
    let y = floor.1 - (FOOT.1 - bounds.y);
    let mut character = CharacterWindow::new(bounds.w, bounds.h, x, y);

    let first = frames.motion_frames(frames::MOTION_IDLE2)[0];
    let drawn = match frames.draw(first, character.pixels(), bounds.w, (bounds.x, bounds.y)) {
        Ok(rect) => rect,
        Err(error) => win::fatal(&format!("{error}.")),
    };
    character.present(drawn);

    win::pump_messages();
    let body = dev::receives_click(character.hwnd(), floor.0, floor.1 - 70);
    let corner = dev::receives_click(character.hwnd(), x + 2, y + 2);
    report.add(format!("창 {}x{} 화면 ({x}, {y}), 칸 안의 범위 {bounds:?}", character.width(), character.height()));
    report.add(format!("idle2 첫 장 {first}번, 그린 범위 {drawn:?}"));
    report.add(format!("클릭을 받는가: 몸 {body}, 창 모서리 {corner}"));
    character
}

/// 그림 팩이 없을 때 창이 제대로 뜨는지 보려는 시험 그림(분홍 고리)을 띄운다.
fn show_test_pattern(floor: (i32, i32), report: &mut dev::Report) -> CharacterWindow {
    let (x, y) = (floor.0 - TEST_SIZE / 2, floor.1 - TEST_SIZE);
    let mut character = CharacterWindow::new(TEST_SIZE, TEST_SIZE, x, y);
    draw_test_pattern(character.pixels(), TEST_SIZE);
    character.present(Rect::new(0, 0, TEST_SIZE, TEST_SIZE));

    // 고리 위는 클릭을 받고, 뚫린 가운데와 바깥 모서리는 뒤 창으로 넘겨야 한다.
    win::pump_messages();
    let ring = dev::receives_click(character.hwnd(), x + TEST_SIZE / 2 + 50, y + TEST_SIZE / 2);
    let hole = dev::receives_click(character.hwnd(), x + TEST_SIZE / 2, y + TEST_SIZE / 2);
    let corner = dev::receives_click(character.hwnd(), x + 3, y + 3);
    report.add(format!("시험 그림 창 {}x{} 화면 ({x}, {y})", character.width(), character.height()));
    report.add(format!("클릭을 받는가: 고리 위 {ring}, 뚫린 가운데 {hole}, 모서리 {corner}"));
    character
}

/// 가장자리가 부드러운 분홍 고리를 그린다.
fn draw_test_pattern(pixels: &mut [u32], size: i32) {
    let center = size as f32 / 2.0;
    let (outer, inner) = (70.0_f32, 25.0_f32);
    for y in 0..size {
        for x in 0..size {
            let d = ((x as f32 + 0.5 - center).powi(2) + (y as f32 + 0.5 - center).powi(2)).sqrt();
            // 고리 안쪽이면 1, 바깥이면 0, 경계 1px에서는 그 사이 값.
            let cover = (outer - d).clamp(0.0, 1.0).min((d - inner).clamp(0.0, 1.0));
            let a = (cover * 255.0) as u32;
            let (r, g, b) = (255 * a / 255, 143 * a / 255, 177 * a / 255);
            pixels[(y * size + x) as usize] = a << 24 | r << 16 | g << 8 | b;
        }
    }
}
