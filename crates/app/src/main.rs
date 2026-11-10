//! DesktopFlowerpin: 꽃핀이 바탕화면 위를 걸어 다니는 Windows 데스크탑 캐릭터.

// 콘솔 창을 띄우지 않는다.
#![windows_subsystem = "windows"]

mod character;
mod character_window;
mod clock;
mod dev;
mod frames;
mod geom;
mod key;
mod scale;
mod win;

use std::time::Instant;

use character::Character;
use character_window::CharacterWindow;
use clock::WakeTimer;
use frames::Frames;
use geom::Rect;
use scale::Scale;

/// 그림 팩 없이 빌드했을 때 띄우는 시험 그림의 한 변 길이.
const TEST_SIZE: i32 = 200;

fn main() {
    let options = dev::Options::from_args();
    let mut report = dev::Report::default();
    win::enable_per_monitor_dpi();

    // 서 있을 바닥: 작업표시줄 바로 위.
    let work = win::primary_work_area();

    match Frames::load() {
        Ok(Some(mut frames)) => {
            if let Err(error) = run(&mut frames, work, &options, &mut report) {
                win::fatal(&format!("{error}."));
            }
        }
        Ok(None) => run_test_pattern((work.x + work.w / 2, work.bottom()), &options, &mut report),
        Err(error) => win::fatal(&format!("{error}.")),
    }
    report.write(&options);
}

/// 꽃핀을 띄우고, 끝내라고 할 때까지 idle2를 되풀이해 재생한다.
fn run(
    frames: &mut Frames,
    work: Rect,
    options: &dev::Options,
    report: &mut dev::Report,
) -> Result<(), dfp_pack::temp::PackError> {
    // 보여 주는 배율은 사용자가 고른 크기 × 모니터 배율이다.
    let dpi = options.dpi.unwrap_or_else(win::system_dpi);
    let scale = Scale::new(options.size, dpi);
    let idle2 = frames.motion_frames(frames::MOTION_IDLE2);

    // 바닥에 고르게 늘어세운다. 모두 같은 시각에 장을 넘기게 해서, 장마다 한 번만 깨어난다.
    // 꽃핀마다 다른 장에서 시작해 서로 다른 그림을 보여 준다.
    let started = Instant::now();
    let count = options.characters as i32;
    let mut characters = Vec::new();
    for i in 0..count {
        let floor = (work.x + work.w * (i + 1) / (count + 1), work.bottom());
        characters.push(Character::new(frames, idle2.clone(), i as usize * 5, scale, floor, started)?);
    }
    report.add(format!("꽃핀 {count}명, 배율 {scale} (크기 {}%, DPI {dpi})", options.size));
    report.add(format!("창 {}x{}", characters[0].window().width(), characters[0].window().height()));

    let timer = WakeTimer::new();
    let mut pace = dev::Pace::default();
    let mut screen_check = options.check_screen.then(|| dev::ScreenCheck::new(started));
    let mut usage = None;
    let deadline = options.run_for.map(|d| started + d);
    loop {
        if !win::pump_messages() {
            break;
        }
        let now = Instant::now();
        if deadline.is_some_and(|d| now >= d) {
            break;
        }
        if usage.is_none() && now >= started + dev::WARM_UP {
            usage = Some(dev::Usage::start());
        }
        // 다음 장을 그리기 바로 전에, 앞 장이 화면에 제대로 나와 있는지 본다. 첫 꽃핀만 본다.
        if let Some(check) = &mut screen_check
            && now >= characters[0].due()
        {
            check.sample(characters[0].window(), now);
        }
        let tick_started = Instant::now();
        let mut shown = false;
        for character in &mut characters {
            shown |= character.tick(frames, now)?;
        }
        if shown {
            pace.frame_shown(now, tick_started.elapsed());
        }
        // 다음 장 시각까지 잔다. 끝낼 시각이 더 이르면 그때 깬다.
        let due = characters.iter().map(Character::due).min().expect("꽃핀은 한 명 이상이다");
        timer.sleep_until(Some(deadline.map_or(due, |d| d.min(due))));
    }
    report.add(pace.summary());
    report.add(pace.cost_summary());
    if let Some(check) = &screen_check {
        report.add(check.summary());
    }
    if let Some(usage) = &usage {
        report.add(usage.summary());
    }
    Ok(())
}

/// 그림 팩이 없을 때 창이 제대로 뜨는지 보려는 시험 그림(분홍 고리)을 띄운다.
fn run_test_pattern(floor: (i32, i32), options: &dev::Options, report: &mut dev::Report) {
    let (x, y) = (floor.0 - TEST_SIZE / 2, floor.1 - TEST_SIZE);
    let mut window = CharacterWindow::new(TEST_SIZE, TEST_SIZE, x, y);
    draw_test_pattern(window.pixels(), TEST_SIZE);
    window.present(Rect::new(0, 0, TEST_SIZE, TEST_SIZE));

    // 고리 위는 클릭을 받고, 뚫린 가운데와 바깥 모서리는 뒤 창으로 넘겨야 한다.
    win::pump_messages();
    let ring = dev::receives_click(window.hwnd(), x + TEST_SIZE / 2 + 50, y + TEST_SIZE / 2);
    let hole = dev::receives_click(window.hwnd(), x + TEST_SIZE / 2, y + TEST_SIZE / 2);
    let corner = dev::receives_click(window.hwnd(), x + 3, y + 3);
    report.add(format!("시험 그림 창 {}x{} 화면 ({x}, {y})", window.width(), window.height()));
    report.add(format!("클릭을 받는가: 고리 위 {ring}, 뚫린 가운데 {hole}, 모서리 {corner}"));

    let timer = WakeTimer::new();
    let deadline = options.run_for.map(|d| Instant::now() + d);
    while win::pump_messages() && !deadline.is_some_and(|d| Instant::now() >= d) {
        timer.sleep_until(deadline);
    }
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
