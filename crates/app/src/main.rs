//! DesktopFlowerpin: 꽃핀이 바탕화면 위를 걸어 다니는 Windows 데스크탑 캐릭터.

// 콘솔 창을 띄우지 않는다.
#![windows_subsystem = "windows"]

#[allow(dead_code)]
mod actor;
mod character;
mod character_window;
mod clock;
mod dev;
mod frames;
mod geom;
mod key;
#[allow(dead_code)]
mod motions;
mod scale;
mod win;

use std::time::{Duration, Instant};

use character::Character;
use clock::WakeTimer;
use dfp_pack::PackError;
use frames::Frames;
use geom::Rect;
use scale::Scale;

/// 깨어난 김에 함께 넘기는 범위. 이 시간 안에 넘길 장은 조금 일찍 지금 넘긴다.
///
/// 장마다 보여 줄 시간이 41ms와 42ms로 섞여 있어서, 여러 명의 넘길 시각이 1ms씩 어긋난다.
/// 그때마다 따로 깨어나면 깨어나는 횟수가 꽃핀 수만큼 는다. 다음 시각은 일찍 넘긴 만큼 당기지 않고
/// 원래 시각에서 이어 세므로, 전체 빠르기는 그대로다.
const COALESCE: Duration = Duration::from_millis(3);

fn main() {
    let options = dev::Options::from_args();
    let mut report = dev::Report::default();
    win::enable_per_monitor_dpi();

    // 서 있을 바닥: 작업표시줄 바로 위.
    let work = win::primary_work_area();
    let result = Frames::load().and_then(|mut frames| run(&mut frames, work, &options, &mut report));
    if let Err(error) = result {
        win::fatal(&format!("{error}."));
    }
    report.write(&options);
}

/// 꽃핀을 띄우고, 끝내라고 할 때까지 idle2를 되풀이해 재생한다.
fn run(frames: &mut Frames, work: Rect, options: &dev::Options, report: &mut dev::Report) -> Result<(), PackError> {
    // 보여 주는 배율은 사용자가 고른 크기 × 모니터 배율이다.
    let dpi = options.dpi.unwrap_or_else(win::system_dpi);
    let scale = Scale::new(options.size, dpi);
    let idle2 = frames.motion_frames("idle2")?;

    // 바닥에 고르게 늘어세운다. 꽃핀마다 다른 장에서 시작해 서로 다른 그림을 보여 준다.
    let started = Instant::now();
    let count = options.characters as i32;
    let mut characters = Vec::new();
    for i in 0..count {
        let floor = (work.x + work.w * (i + 1) / (count + 1), work.bottom());
        characters.push(Character::new(frames, idle2.clone(), i as usize * 5, scale, floor, started)?);
    }
    let pack = frames.pack();
    report.add(format!(
        "그림 팩: {} (키 번호 {}), 동작 {}개, 장 {}개",
        if pack.header().placeholder { "대체 그림" } else { "진짜 그림" },
        pack.header().key_id,
        pack.motions().len(),
        pack.frames().len()
    ));
    report.add(format!("꽃핀 {count}명, 배율 {scale} (크기 {}%, DPI {dpi})", options.size));
    report.add(format!("창 {}x{}", characters[0].window().width(), characters[0].window().height()));
    win::pump_messages();
    report.add(dev::click_check(characters[0].window()));

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
        let horizon = now + COALESCE;
        // 다음 장을 그리기 바로 전에, 앞 장이 화면에 제대로 나와 있는지 본다. 첫 꽃핀만 본다.
        if let Some(check) = &mut screen_check
            && horizon >= characters[0].due()
        {
            check.sample(characters[0].window(), now);
        }
        let tick_started = Instant::now();
        let mut shown = false;
        for character in &mut characters {
            shown |= character.tick(frames, horizon)?;
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
