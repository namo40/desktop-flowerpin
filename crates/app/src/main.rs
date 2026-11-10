//! DesktopFlowerpin: 꽃핀이 바탕화면 위를 걸어 다니는 Windows 데스크탑 캐릭터.

// 콘솔 창을 띄우지 않는다.
#![windows_subsystem = "windows"]

mod character_window;
mod clock;
mod dev;
mod geom;
mod win;

use std::time::Instant;

use character_window::CharacterWindow;
use clock::WakeTimer;
use geom::Rect;

/// 시험 그림을 그릴 창의 한 변 길이.
const TEST_SIZE: i32 = 200;

fn main() {
    let options = dev::Options::from_args();
    let mut report = dev::Report::default();
    win::enable_per_monitor_dpi();

    // 작업표시줄 바로 위, 화면 가운데에 놓는다.
    let work = win::primary_work_area();
    let x = work.x + (work.w - TEST_SIZE) / 2;
    let y = work.bottom() - TEST_SIZE;
    let mut character = CharacterWindow::new(TEST_SIZE, TEST_SIZE, x, y);
    draw_test_pattern(character.pixels(), TEST_SIZE);
    character.present(Rect::new(0, 0, TEST_SIZE, TEST_SIZE));

    // 시험 그림은 가운데가 뚫린 고리다. 고리 위는 클릭을 받고, 뚫린 가운데와 바깥 모서리는 넘겨야 한다.
    win::pump_messages();
    let (px, py) = character.pos();
    let ring = dev::receives_click(character.hwnd(), px + TEST_SIZE / 2 + 50, py + TEST_SIZE / 2);
    let hole = dev::receives_click(character.hwnd(), px + TEST_SIZE / 2, py + TEST_SIZE / 2);
    let corner = dev::receives_click(character.hwnd(), px + 3, py + 3);
    report.add(format!("창 {}x{} 화면 ({px}, {py})", character.width(), character.height()));
    report.add(format!("클릭을 받는가: 고리 위 {ring}, 뚫린 가운데 {hole}, 모서리 {corner}"));

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

/// 창이 제대로 뜨는지 보려는 시험 그림. 가장자리가 부드러운 분홍 고리를 그린다.
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
