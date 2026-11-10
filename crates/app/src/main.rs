//! DesktopFlowerpin: 꽃핀이 바탕화면 위를 걸어 다니는 Windows 데스크탑 캐릭터.

// 콘솔 창을 띄우지 않는다.
#![windows_subsystem = "windows"]

mod actor;
mod character;
mod character_window;
mod clock;
mod dev;
mod frames;
mod geom;
mod key;
mod motions;
mod scale;
mod shadow;
mod stage;
mod tray;
mod win;

use std::collections::VecDeque;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use actor::{Habits, Mode};
use character::{Character, Look};
use character_window::WindowEvent;
use clock::WakeTimer;
use dfp_pack::PackError;
use frames::Frames;
use motions::{MotionId, Motions};
use stage::Stage;
use tray::{MenuItem, Tray, TrayEvent};
use windows_sys::Win32::Foundation::HWND;

/// 깨어난 김에 함께 넘기는 범위. 이 시간 안에 넘길 장은 조금 일찍 지금 넘긴다.
///
/// 장마다 보여 줄 시간이 41ms와 42ms로 섞여 있어서, 여러 명의 넘길 시각이 1ms씩 어긋난다.
/// 그때마다 따로 깨어나면 깨어나는 횟수가 꽃핀 수만큼 는다. 시간은 늘 이만큼 앞선 시각까지 흘리므로
/// 전체 빠르기는 그대로다.
const COALESCE: Duration = Duration::from_millis(3);

/// 누른 뒤 이만큼 움직이거나(px, 그림 1배 기준) 이만큼 누르고 있으면 잡는 것으로 본다.
/// 그 전에 떼면 클릭이다.
const CLICK_MOVE: f64 = 6.0;
const CLICK_HOLD: Duration = Duration::from_millis(250);

/// 마우스와 키보드를 이만큼 쓰지 않으면 자리를 비운 것으로 친다. 그러면 꽃핀도 잠든다.
const AWAY_AFTER: Duration = Duration::from_secs(5 * 60);
/// 자리를 비웠는지 확인하는 간격.
const AWAY_CHECK: Duration = Duration::from_secs(1);
/// 머리 위에서 움직인 것을 이어서 문지른 것으로 치는 한계. 이보다 오래 끊겼거나 한 번에 이보다 멀리(px, 그림
/// 1배 기준) 뛰었으면 커서가 꽃핀을 벗어났다 돌아온 것이라 세지 않는다.
const RUB_GAP: Duration = Duration::from_millis(250);
const RUB_JUMP: f64 = 120.0;

/// 끝내기를 고른 뒤 꽃핀들이 사라지기를 기다리는 한계. 공중에 있던 꽃핀이 내려와 인사할 시간이다.
const QUIT_LIMIT: Duration = Duration::from_secs(8);

/// 마우스로 꽃핀에게 하고 있는 일.
#[derive(Clone, Copy)]
enum Pointer {
    Idle,
    /// 눌렀지만 아직 잡기인지 클릭인지 모른다.
    Pressing {
        hwnd: HWND,
        at: (i32, i32),
        since: Instant,
    },
    Dragging {
        hwnd: HWND,
    },
}

fn main() {
    let options = dev::Options::from_args();
    let mut report = dev::Report::default();
    win::enable_per_monitor_dpi();

    // 앱은 하나만 실행한다. 이미 실행 중이면 그쪽에 알리고 끝낸다.
    // 정한 시간만 돌고 끝나는 확인용 실행은 막지 않는다. 재는 동안 다른 실행이 떠 있어도 되게 하려는 것이다.
    let _instance = if options.run_for.is_none() {
        let Some(instance) = tray::claim() else {
            report.add("이미 실행 중이라 끝냅니다");
            report.write(&options);
            return;
        };
        Some(instance)
    } else {
        None
    };

    let result = Frames::load().and_then(|mut frames| run(&mut frames, &options, &mut report));
    if let Err(error) = result {
        win::fatal(&format!("{error}."));
    }
    report.write(&options);
}

/// 처음 나타날 자리(발바닥). 주 모니터의 가운데 위쪽 공중이다. 여러 명이면 가로로 고르게 늘어놓는다.
fn start_spot(stage: &Stage, index: usize, count: usize) -> (f64, f64) {
    let work = stage.primary().work;
    let x = work.x + work.w * (index as i32 + 1) / (count as i32 + 1);
    (x as f64, (work.y + work.h * 3 / 10) as f64)
}

/// `--play`에 적은 이름들을 동작 번호로 바꾼다. `all`이면 팩의 모든 동작이다.
fn play_list(names: &[String], frames: &Frames, motions: &Motions) -> Result<VecDeque<MotionId>, PackError> {
    let mut list = VecDeque::new();
    for name in names {
        if name == "all" {
            list.extend(0..frames.pack().motions().len() as MotionId);
        } else {
            list.push_back(motions.find(name).ok_or_else(|| PackError::NoSuchMotion(name.clone()))?);
        }
    }
    Ok(list)
}

/// 꽃핀을 띄우고, 끝내라고 할 때까지 돌린다.
fn run(frames: &mut Frames, options: &dev::Options, report: &mut dev::Report) -> Result<(), PackError> {
    let motions = Motions::from_pack(frames.pack())?;
    let mut stage = Stage::read();
    let mut look = Look { size_percent: options.size, dpi_override: options.dpi };
    let mut play = play_list(&options.play, frames, &motions)?;
    let showing = !play.is_empty();

    let started = Instant::now();
    let mut seed =
        options.seed.unwrap_or_else(|| SystemTime::now().duration_since(UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64));
    let mut next_seed = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        seed
    };
    let mut characters = Vec::new();
    for i in 0..options.characters {
        let spot = start_spot(&stage, i, options.characters);
        // 동작을 차례로 보여 줄 때는 나타나는 과정 없이 바닥에 선 채로 시작한다.
        characters.push(if showing {
            Character::stand(frames, &motions, &stage, look, spot.0, (started, started), next_seed())?
        } else {
            Character::appear(frames, &motions, &stage, look, spot, (started, started), next_seed())?
        });
    }
    if options.still {
        let still = Habits { roam: false, whim_rate: 0.0, ..Habits::default() };
        characters.iter_mut().for_each(|character| character.actor_mut().set_habits(still));
    }

    let pack = frames.pack();
    report.add(format!(
        "그림 팩: {} (키 번호 {}), 동작 {}개, 장 {}개",
        if pack.header().placeholder { "대체 그림" } else { "진짜 그림" },
        pack.header().key_id,
        pack.motions().len(),
        pack.frames().len()
    ));
    for monitor in stage.monitors() {
        let (area, work) = (monitor.area, monitor.work);
        report.add(format!(
            "모니터{}: {}x{} ({}, {}), 작업 영역 아래 끝 {}, 배율 {}%",
            if monitor.primary { "(주)" } else { "" },
            area.w,
            area.h,
            area.x,
            area.y,
            work.bottom(),
            monitor.dpi * 100 / 96
        ));
    }
    report.add(format!("꽃핀 {}명, 배율 {} (크기 {}%)", characters.len(), characters[0].scale(), options.size));
    report.add(format!("창 {}x{}", characters[0].window().width(), characters[0].window().height()));
    report.add(format!("시작할 때 마지막 입력 뒤 {:.1}초", win::idle_seconds()));

    // 트레이 아이콘은 서 있는 그림(idle2의 첫 장)의 얼굴로 만든다.
    let icon_size = tray::icon_size();
    let face = frames.portrait(motions.clip(motions.idle2).first_frame, icon_size as usize)?;
    let tray = Tray::new(&face, icon_size);
    report.add(match tray.icon_rect() {
        Some(rect) => format!("트레이 아이콘 등록됨: 자리 ({}, {}) {}x{}", rect.x, rect.y, rect.w, rect.h),
        None => "트레이 아이콘이 등록되지 않음".to_string(),
    });

    let timer = WakeTimer::new();
    let mut pace = dev::Pace::default();
    let mut trace = dev::Trace::default();
    let mut screen_check = options.check_screen.then(|| dev::ScreenCheck::new(started));
    let mut usage = None;
    let mut clicked = false;
    let mut script: VecDeque<dev::Step> = options.script.iter().copied().collect();
    let mut script_origin = (0.0, 0.0);
    let mut pointer = Pointer::Idle;
    // 누르지 않은 채 꽃핀 위에서 마지막으로 움직인 곳. 문지른 거리를 재는 데 쓴다.
    let mut hover: Option<(HWND, (i32, i32), Instant)> = None;
    // 자리를 비운 것으로 치는 시간. 0이면 보지 않는다.
    let away_after = options.away_after.unwrap_or(AWAY_AFTER);
    let mut away = false;
    let mut away_forced = false;
    let mut next_away_check = started;
    let mut quitting: Option<Instant> = None;
    let mut hellos = 0;
    let deadline = options.run_for.map(|d| started + d);

    loop {
        if !win::pump_messages() {
            break;
        }
        let now = Instant::now();
        if deadline.is_some_and(|d| now >= d) || quitting.is_some_and(|limit| now >= limit) {
            break;
        }
        if usage.is_none() && now >= started + dev::WARM_UP {
            usage = Some(dev::Usage::start());
        }
        let find = |characters: &[Character], hwnd: HWND| {
            characters.iter().position(|character| character.window().hwnd() == hwnd)
        };
        let mut quit_asked = false;
        let mut stage_changed = false;
        let mut menu_at = None;

        // 1. 창에서 온 일: 누르기, 끌기, 떼기, 모니터가 바뀜.
        for event in character_window::take_events() {
            match event {
                WindowEvent::Press { hwnd, x, y } => {
                    let grabbable = find(&characters, hwnd).is_some_and(|i| characters[i].actor().can_grab());
                    if grabbable && matches!(pointer, Pointer::Idle) {
                        pointer = Pointer::Pressing { hwnd, at: (x, y), since: now };
                    } else if !grabbable {
                        character_window::stop_pressing();
                    }
                }
                WindowEvent::Move { hwnd, x, y } => match pointer {
                    Pointer::Pressing { hwnd: pressed, at, .. } if pressed == hwnd => {
                        let far = find(&characters, hwnd).is_some_and(|i| {
                            let limit = CLICK_MOVE * characters[i].scale().factor();
                            ((x - at.0) as f64).hypot((y - at.1) as f64) > limit
                        });
                        if far {
                            pointer = start_drag(&mut characters, &motions, &stage, hwnd, (x, y), now);
                        }
                    }
                    Pointer::Dragging { hwnd: dragged } if dragged == hwnd => {
                        if let Some(i) = find(&characters, hwnd) {
                            let target = characters[i].drag_target((x, y), &stage);
                            characters[i].actor_mut().drag_to(target);
                            characters[i].wake(now);
                        }
                    }
                    _ => {}
                },
                WindowEvent::Lift { hwnd } => {
                    match pointer {
                        Pointer::Dragging { hwnd: dragged } if dragged == hwnd => {
                            if let Some(i) = find(&characters, hwnd) {
                                characters[i].actor_mut().release(&motions);
                                characters[i].wake(now);
                            }
                            pointer = Pointer::Idle;
                        }
                        // 짧게 눌렀다 뗐다. 클릭이다.
                        Pointer::Pressing { hwnd: pressed, .. } if pressed == hwnd => {
                            if let Some(i) = find(&characters, hwnd) {
                                characters[i].actor_mut().poke(&motions);
                                characters[i].wake(now);
                            }
                            pointer = Pointer::Idle;
                        }
                        _ => {}
                    }
                }
                WindowEvent::Hover { hwnd, x, y } => {
                    if let (Some((last_hwnd, at, when)), Some(i)) = (hover, find(&characters, hwnd)) {
                        // 가로와 세로로 움직인 거리를 더한다(데모와 같은 셈법).
                        let distance = ((x - at.0).abs() + (y - at.1).abs()) as f64;
                        let connected = last_hwnd == hwnd
                            && now.duration_since(when) <= RUB_GAP
                            && distance <= RUB_JUMP * characters[i].scale().factor();
                        if connected && distance > 0.0 {
                            characters[i].rub(&motions, (x, y), distance, now);
                        }
                    }
                    hover = Some((hwnd, (x, y), now));
                }
                WindowEvent::Menu { x, y } => menu_at = Some((x, y)),
                WindowEvent::StageChanged => stage_changed = true,
            }
        }
        for event in tray::take_events() {
            match event {
                TrayEvent::Menu { x, y } => menu_at = Some((x, y)),
                TrayEvent::TaskbarCreated => tray.add(),
                // 앱을 한 번 더 켰다. 꽃핀이 어디 있는지 알려 주는 일은 아직 하지 않는다.
                TrayEvent::Hello => hellos += 1,
            }
        }
        // 꽃핀을 오른쪽 클릭해도 트레이 아이콘과 같은 메뉴가 뜬다. 메뉴가 떠 있는 동안에는 여기서 기다린다.
        if let Some((x, y)) = menu_at
            && tray.pick(x, y) == Some(MenuItem::Quit)
        {
            quit_asked = true;
        }
        // 누른 채로 가만히 있어도 시간이 지나면 잡는다.
        if let Pointer::Pressing { hwnd, since, .. } = pointer
            && now >= since + CLICK_HOLD
        {
            pointer = start_drag(&mut characters, &motions, &stage, hwnd, win::cursor_position(), now);
        }

        // 2. 모니터가 바뀌었으면 무대를 다시 읽는다. 화면 밖에 남은 꽃핀은 주 모니터에 다시 나타난다.
        if stage_changed {
            stage = Stage::read();
            for (i, character) in characters.iter_mut().enumerate() {
                let (x, y) = character.actor().position();
                if !stage.contains(x.round() as i32, y.round() as i32 - 1) {
                    let spot = start_spot(&stage, i, options.characters);
                    *character = Character::appear(frames, &motions, &stage, look, spot, (now, started), next_seed())?;
                }
                character.wake(now);
            }
        }

        // 사용자가 자리를 비웠는지 가끔 본다. 비웠으면 꽃핀도 잠들고, 돌아오면 깬다.
        if now >= next_away_check && !away_forced {
            next_away_check = now + AWAY_CHECK;
            let idle = Duration::from_secs_f64(win::idle_seconds());
            let now_away = !away_after.is_zero() && idle >= away_after;
            if now_away != away {
                away = now_away;
                for character in &mut characters {
                    character.actor_mut().set_away(away);
                    character.wake(now);
                }
            }
        }

        // 3. 만드는 동안 확인용: 정한 시각에 시킨 일과 차례로 보여 줄 동작.
        while script.front().is_some_and(|step| now >= started + step.at) {
            let Some(step) = script.pop_front() else { break };
            let character = &mut characters[0];
            match step.command {
                dev::Command::Grab => {
                    script_origin = character.actor().position();
                    character.actor_mut().grab(&motions, script_origin);
                }
                dev::Command::Drag(dx, dy) => {
                    let target = character.drag_to_foot((script_origin.0 + dx, script_origin.1 + dy), &stage);
                    character.actor_mut().drag_to(target);
                }
                dev::Command::Release => character.actor_mut().release(&motions),
                dev::Command::Leave => quit_asked = true,
                dev::Command::Roam(roam) => {
                    let habits = Habits { roam, ..character.actor().habits() };
                    character.actor_mut().set_habits(habits);
                }
                dev::Command::Dpi(dpi) => {
                    look.dpi_override = Some(dpi);
                    characters.iter_mut().for_each(|character| character.set_look(look, now));
                }
                dev::Command::Click => character.actor_mut().poke(&motions),
                dev::Command::Rub(distance) => character.actor_mut().rub(&motions, distance),
                dev::Command::Do(whim) => character.actor_mut().start(whim, &motions),
                dev::Command::Away(on) => {
                    away = on;
                    away_forced = true;
                    characters.iter_mut().for_each(|character| character.actor_mut().set_away(on));
                }
            }
            characters[0].wake(now);
        }
        if let Some(&motion) = play.front()
            && characters[0].actor().mode() == Mode::Ground
        {
            play.pop_front();
            characters[0].actor_mut().play(motion);
            characters[0].wake(now);
        }

        // 4. 끝내기: 꽃핀들이 손을 흔들고 사라진 뒤에 끝낸다. 한 번 더 고르면 바로 끝낸다.
        if quit_asked {
            if quitting.is_some() {
                break;
            }
            quitting = Some(now + QUIT_LIMIT);
            for character in &mut characters {
                character.actor_mut().leave();
                character.wake(now);
            }
        }

        // 5. 때가 된 꽃핀의 시간을 흘리고 화면에 넘긴다.
        let horizon = now + COALESCE;
        // 다음 장을 그리기 바로 전에, 앞 장이 화면에 제대로 나와 있는지 본다. 첫 꽃핀이 멈춰 있을 때만 본다.
        if let Some(check) = &mut screen_check
            && horizon >= characters[0].due()
            && !characters[0].moved()
        {
            check.sample(characters[0].window(), now);
        }
        let tick_started = Instant::now();
        let mut shown = false;
        for character in &mut characters {
            shown |= character.tick(frames, &motions, &stage, horizon)?;
        }
        if shown {
            pace.frame_shown(now, tick_started.elapsed());
            trace.note((now - started).as_secs_f64(), characters[0].actor(), &motions);
        }
        if !clicked && characters[0].actor().mode() == Mode::Ground {
            // 바닥에 선 뒤에 한 번, 몸은 클릭을 받고 투명한 곳은 뒤 창으로 넘기는지 본다.
            clicked = true;
            report.add(dev::click_check(characters[0].window()));
        }

        // 6. 다 사라진 꽃핀은 창을 닫는다. 한 명도 남지 않으면 끝낸다.
        characters.retain(|character| !character.actor().is_gone());
        if characters.is_empty() {
            break;
        }

        // 7. 다음에 할 일이 있을 때까지 잔다.
        let mut wake = characters.iter().map(Character::due).min().expect("꽃핀이 한 명 이상 남아 있다");
        if let Pointer::Pressing { since, .. } = pointer {
            wake = wake.min(since + CLICK_HOLD);
        }
        if !away_forced && !away_after.is_zero() {
            wake = wake.min(next_away_check);
        }
        for limit in [deadline, quitting, script.front().map(|step| started + step.at)].into_iter().flatten() {
            wake = wake.min(limit);
        }
        timer.sleep_until(Some(wake));
    }

    report.add(pace.summary());
    report.add(pace.cost_summary());
    if let Some(check) = &screen_check {
        report.add(check.summary());
    }
    if let Some(usage) = &usage {
        report.add(usage.summary());
    }
    if hellos > 0 {
        report.add(format!("실행 중에 앱을 한 번 더 켠 횟수 {hellos}"));
    }
    trace.lines().into_iter().for_each(|line| report.add(line));
    Ok(())
}

/// 눌린 꽃핀을 잡는다. 잡혔으면 끄는 중으로, 잡을 수 없게 됐으면 아무것도 안 하는 것으로 돌아간다.
fn start_drag(
    characters: &mut [Character],
    motions: &Motions,
    stage: &Stage,
    hwnd: HWND,
    cursor: (i32, i32),
    now: Instant,
) -> Pointer {
    if let Some(character) = characters.iter_mut().find(|character| character.window().hwnd() == hwnd) {
        let target = character.drag_target(cursor, stage);
        if character.actor_mut().grab(motions, target) {
            character.wake(now);
            return Pointer::Dragging { hwnd };
        }
    }
    character_window::stop_pressing();
    Pointer::Idle
}
