//! 앱 전체. 꽃핀들, 트레이 아이콘, 설정 창, 저장을 한데 묶어 메인 루프를 돌린다.
//!
//! 메인 루프는 한 바퀴에 이렇게 한다.
//! 1. 쌓인 창 메시지를 처리한다([`App::pump`]).
//! 2. 그사이에 일어난 일을 꺼내 처리하고, 때가 된 꽃핀을 움직인다([`App::frame`]).
//! 3. 메뉴나 물음 창을 띄워야 하면 띄운다([`run_modal`]). 닫힐 때까지 돌아오지 않지만 꽃핀은 계속 움직인다.
//! 4. 다음에 할 일이 있을 때까지 잔다([`App::sleep`]).

use std::cell::Cell;
use std::collections::VecDeque;
use std::ptr::null_mut;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use dfp_pack::PackError;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage, WM_QUIT,
};

use crate::actor::{Habits, Mode};
use crate::character::{self, Character, Look};
use crate::character_window::{self, WindowEvent};
use crate::clock::WakeTimer;
use crate::dev;
use crate::frames::Frames;
use crate::links::LINKS;
use crate::motions::{MotionId, Motions};
use crate::settings::{Key, Settings};
use crate::settings_window::{self, DialogEvent, SettingsWindow, TAB_NAMES};
use crate::stage::Stage;
use crate::store::{self, State, Store};
use crate::tray::{self, MenuItem, Tray, TrayEvent};
use crate::usage::{self, Meter};
use crate::win;

/// 누른 뒤 이만큼 움직이거나(px, 그림 1배 기준) 이만큼 누르고 있으면 잡는 것으로 본다. 그 전에 떼면 클릭이다.
const CLICK_MOVE: f64 = 6.0;
const CLICK_HOLD: Duration = Duration::from_millis(250);
/// 자리를 비웠는지 확인하는 간격.
const AWAY_CHECK: Duration = Duration::from_secs(1);
/// 마우스와 키보드를 이만큼 쓰지 않으면 자리를 비운 것으로 친다. 그러면 꽃핀도 잠든다.
const AWAY_AFTER: Duration = Duration::from_secs(5 * 60);
/// 머리 위에서 움직인 것을 이어서 문지른 것으로 치는 한계. 이보다 오래 끊겼거나 한 번에 이보다 멀리(px, 그림
/// 1배 기준) 뛰었으면 커서가 꽃핀을 벗어났다 돌아온 것이라 세지 않는다.
const RUB_GAP: Duration = Duration::from_millis(250);
const RUB_JUMP: f64 = 120.0;
/// 끝내기를 고른 뒤 꽃핀들이 사라지기를 기다리는 한계. 공중에 있던 꽃핀이 내려와 인사할 시간이다.
const QUIT_LIMIT: Duration = Duration::from_secs(8);
/// 설정 창의 "지금 쓰는 자원"을 고치는 간격.
const USAGE_EVERY: Duration = Duration::from_secs(1);
/// 설정 창 정보 탭에 보여 줄 그림의 한 변(px).
const ABOUT_ICON: i32 = 32;

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

/// [`App::frame`]이 메인 루프에 시키는 일.
pub enum Flow {
    Continue,
    Quit,
    /// 화면의 이 자리에 메뉴를 띄운다.
    Menu(i32, i32),
    /// 설정 창의 이 탭을 기본값으로 되돌릴지 묻는다.
    ConfirmReset(usize),
}

/// 만드는 동안 확인하고 재는 데 쓰는 것들.
struct Dev {
    options: dev::Options,
    pace: dev::Pace,
    trace: dev::Trace,
    screen_check: Option<dev::ScreenCheck>,
    usage: Option<dev::Usage>,
    click_checked: bool,
    script: VecDeque<dev::Step>,
    script_origin: (f64, f64),
    play: VecDeque<MotionId>,
    hellos: u32,
    snaps: u32,
    deadline: Option<Instant>,
    /// 실행하면서 보고서에 적을 줄.
    notes: Vec<String>,
}

pub struct App {
    frames: Frames,
    motions: Motions,
    stage: Stage,
    settings: Settings,
    state: State,
    store: Store,
    look: Look,
    characters: Vec<Character>,
    pointer: Pointer,
    /// 누르지 않은 채 꽃핀 위에서 마지막으로 움직인 곳. 문지른 거리를 재는 데 쓴다.
    hover: Option<(HWND, (i32, i32), Instant)>,
    away: bool,
    /// 스크립트가 자리 비움을 정했다. 그 뒤로는 진짜 입력 시각을 보지 않는다.
    away_forced: bool,
    next_away_check: Instant,
    quitting: Option<Instant>,
    tray: Tray,
    window: Option<SettingsWindow>,
    /// 설정 창에 보여 줄 CPU를 재는 도구와, 다음에 고칠 시각.
    meter: Meter,
    next_usage: Instant,
    started: Instant,
    seed: u64,
    timer: WakeTimer,
    dev: Dev,
}

thread_local! {
    /// 메뉴나 물음 창이 떠 있는 동안 꽃핀을 움직일 앱. 그동안에만 들어 있다.
    static MODAL: Cell<*mut App> = const { Cell::new(null_mut()) };
}

/// 메뉴나 물음 창이 떠 있는 동안 타이머가 부른다.
fn modal_tick() {
    let app = MODAL.get();
    if !app.is_null() {
        // `run_modal`이 돌아올 때까지 이 포인터로만 앱을 건드린다(아래 설명).
        let _ = unsafe { (*app).tick_characters(Instant::now()) };
    }
}

/// 메뉴나 물음 창처럼 닫힐 때까지 돌아오지 않는 것을 띄운다. 떠 있는 동안에도 꽃핀은 계속 움직인다.
///
/// `show`가 도는 동안 앱은 타이머가 부르는 [`modal_tick`]에서만 쓰인다. `show`는 앱을 붙잡지 않고,
/// 이 함수도 `show`가 돌아올 때까지 `app`을 쓰지 않는다. 모두 같은 스레드에서 차례로 일어나는 일이다.
pub fn run_modal<T>(app: &mut App, show: impl FnOnce() -> T) -> T {
    let owner = app.tray.hwnd();
    MODAL.set(app);
    let result = tray::while_modal(owner, modal_tick, show);
    MODAL.set(null_mut());
    result
}

impl App {
    pub fn new(options: dev::Options) -> Result<App, PackError> {
        let mut frames = Frames::load()?;
        let motions = Motions::from_pack(frames.pack())?;
        let stage = Stage::read();
        let started = Instant::now();

        // 정한 시간만 돌고 끝나는 확인용 실행은 폴더를 따로 주지 않으면 파일을 읽지도 쓰지도 않는다.
        let folder =
            options.data_dir.clone().or_else(|| options.run_for.is_none().then(store::default_folder).flatten());
        let (store, settings, state, mut notes) = Store::open(folder);
        if let Some(folder) = store.folder() {
            notes.push(format!("설정 폴더 {}", folder.display()));
        }

        let look = Look {
            size_percent: if options.size != 100 { options.size } else { settings.size_percent() },
            dpi_override: options.dpi,
            topmost: settings.is_on(Key::AlwaysOnTop),
        };
        let mut play = VecDeque::new();
        for name in &options.play {
            if name == "all" {
                play.extend(0..frames.pack().motions().len() as MotionId);
            } else {
                play.push_back(motions.find(name).ok_or_else(|| PackError::NoSuchMotion(name.clone()))?);
            }
        }

        // 트레이 아이콘은 서 있는 그림(idle2의 첫 장)의 얼굴로 만든다.
        let icon_size = tray::icon_size();
        let face = frames.portrait(motions.clip(motions.idle2).first_frame, icon_size as usize)?;
        let tray = Tray::new(&face, icon_size);

        let mut app = App {
            frames,
            motions,
            stage,
            settings,
            state,
            store,
            look,
            characters: Vec::new(),
            pointer: Pointer::Idle,
            hover: None,
            away: false,
            away_forced: false,
            next_away_check: started,
            quitting: None,
            tray,
            window: None,
            meter: Meter::start(),
            next_usage: started,
            started,
            seed: options
                .seed
                .unwrap_or_else(|| SystemTime::now().duration_since(UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64)),
            timer: WakeTimer::new(),
            dev: Dev {
                pace: dev::Pace::default(),
                trace: dev::Trace::default(),
                screen_check: options.check_screen.then(|| dev::ScreenCheck::new(started)),
                usage: None,
                click_checked: false,
                script: options.script.iter().copied().collect(),
                script_origin: (0.0, 0.0),
                play,
                hellos: 0,
                snaps: 0,
                deadline: options.run_for.map(|d| started + d),
                notes,
                options,
            },
        };

        let showing = !app.dev.play.is_empty();
        for i in 0..app.dev.options.characters {
            let spot = app.start_spot(i);
            let seed = app.next_seed();
            // 동작을 차례로 보여 줄 때는 나타나는 과정 없이 바닥에 선 채로 시작한다.
            let character = if showing {
                Character::stand(&mut app.frames, &app.motions, &app.stage, app.look, spot.0, (started, started), seed)?
            } else {
                Character::appear(&mut app.frames, &app.motions, &app.stage, app.look, spot, (started, started), seed)?
            };
            app.characters.push(character);
        }
        app.apply_habits();
        app.describe();
        Ok(app)
    }

    fn next_seed(&mut self) -> u64 {
        self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.seed
    }

    /// 꽃핀이 나타날 자리(발바닥). 기본 꽃핀은 지난번에 있던 자리 위, 없으면 주 모니터의 가운데 위쪽 공중이다.
    /// 여러 명이면 가로로 고르게 늘어놓는다.
    fn start_spot(&self, index: usize) -> (f64, f64) {
        let count = self.dev.options.characters as i32;
        let remembered =
            self.state.character.filter(|&(x, y)| index == 0 && count == 1 && self.stage.contains(x, y - 1));
        if let Some((x, y)) = remembered {
            let work = self.stage.nearest(x, y - 1).work;
            return (x as f64, (work.y + work.h * 3 / 10) as f64);
        }
        let work = self.stage.primary().work;
        let x = work.x + work.w * (index as i32 + 1) / (count + 1);
        (x as f64, (work.y + work.h * 3 / 10) as f64)
    }

    /// 시작할 때의 상황을 보고서에 적는다.
    fn describe(&mut self) {
        let pack = self.frames.pack();
        let mut lines = vec![format!(
            "그림 팩: {} (키 번호 {}), 동작 {}개, 장 {}개",
            if pack.header().placeholder { "대체 그림" } else { "진짜 그림" },
            pack.header().key_id,
            pack.motions().len(),
            pack.frames().len()
        )];
        for monitor in self.stage.monitors() {
            lines.push(format!(
                "모니터{}: {}x{} ({}, {}), 작업 영역 아래 끝 {}, 배율 {}%",
                if monitor.primary { "(주)" } else { "" },
                monitor.area.w,
                monitor.area.h,
                monitor.area.x,
                monitor.area.y,
                monitor.work.bottom(),
                monitor.dpi * 100 / 96
            ));
        }
        let first = &self.characters[0];
        lines.push(format!(
            "꽃핀 {}명, 배율 {} (크기 {}%)",
            self.characters.len(),
            first.scale(),
            self.look.size_percent
        ));
        lines.push(format!("창 {}x{}", first.window().width(), first.window().height()));
        lines.push(format!("시작할 때 마지막 입력 뒤 {:.1}초", win::idle_seconds()));
        lines.push(match self.tray.icon_rect() {
            Some(rect) => format!("트레이 아이콘 등록됨: 자리 ({}, {}) {}x{}", rect.x, rect.y, rect.w, rect.h),
            None => "트레이 아이콘이 등록되지 않음".to_string(),
        });
        self.dev.notes.splice(0..0, lines);
    }

    /// 설정의 버릇을 모든 꽃핀에게 알린다.
    fn apply_habits(&mut self) {
        let habits =
            if self.dev.options.still { Habits { roam: false, whim_rate: 0.0 } } else { self.settings.habits() };
        self.characters.iter_mut().for_each(|character| character.actor_mut().set_habits(habits));
    }

    /// 바뀐 설정을 화면의 꽃핀에게 바로 반영하고, 저장을 예약한다.
    fn apply_settings(&mut self, now: Instant) {
        self.look.size_percent = self.settings.size_percent();
        self.look.topmost = self.settings.is_on(Key::AlwaysOnTop);
        let look = self.look;
        self.characters.iter_mut().for_each(|character| character.set_look(look, now));
        self.apply_habits();
        self.store.settings_changed(now);
    }

    /// 마우스와 키보드를 이만큼 쓰지 않으면 자리를 비운 것으로 친다. 확인용 인자로 껐으면 `None`이다.
    fn away_after(&self) -> Option<Duration> {
        match self.dev.options.away_after {
            Some(forced) => (!forced.is_zero()).then_some(forced),
            None => Some(AWAY_AFTER),
        }
    }

    fn find(&self, hwnd: HWND) -> Option<usize> {
        self.characters.iter().position(|character| character.window().hwnd() == hwnd)
    }

    /// 쌓여 있는 창 메시지를 모두 처리한다. 끝내라는 메시지를 받으면 `false`를 돌려준다.
    pub fn pump(&mut self) -> bool {
        let mut message: MSG = unsafe { std::mem::zeroed() };
        while unsafe { PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) } != 0 {
            if message.message == WM_QUIT {
                return false;
            }
            // 설정 창이 열려 있으면 키보드 이동(Tab, Alt+글자, Ctrl+Tab)을 먼저 처리하게 한다.
            if self.window.as_mut().is_some_and(|window| window.handles(&message)) {
                continue;
            }
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        true
    }

    /// 한 바퀴의 일을 한다.
    pub fn frame(&mut self, now: Instant) -> Result<Flow, PackError> {
        if self.dev.deadline.is_some_and(|d| now >= d) || self.quitting.is_some_and(|limit| now >= limit) {
            return Ok(Flow::Quit);
        }
        if self.dev.usage.is_none() && now >= self.started + dev::WARM_UP {
            self.dev.usage = Some(dev::Usage::start());
        }
        let mut quit_asked = false;
        let mut stage_changed = false;
        let mut flow = Flow::Continue;

        // 1. 꽃핀 창에서 온 일: 누르기, 끌기, 떼기, 문지르기, 모니터가 바뀜.
        for event in character_window::take_events() {
            match event {
                WindowEvent::Press { hwnd, x, y } => {
                    let grabbable = self.find(hwnd).is_some_and(|i| self.characters[i].actor().can_grab());
                    if grabbable && matches!(self.pointer, Pointer::Idle) {
                        self.pointer = Pointer::Pressing { hwnd, at: (x, y), since: now };
                    } else if !grabbable {
                        character_window::stop_pressing();
                    }
                }
                WindowEvent::Move { hwnd, x, y } => match self.pointer {
                    Pointer::Pressing { hwnd: pressed, at, .. } if pressed == hwnd => {
                        let far = self.find(hwnd).is_some_and(|i| {
                            let limit = CLICK_MOVE * self.characters[i].scale().factor();
                            ((x - at.0) as f64).hypot((y - at.1) as f64) > limit
                        });
                        if far {
                            self.start_drag(hwnd, (x, y), now);
                        }
                    }
                    Pointer::Dragging { hwnd: dragged } if dragged == hwnd => {
                        if let Some(i) = self.find(hwnd) {
                            let target = self.characters[i].drag_target((x, y), &self.stage);
                            self.characters[i].actor_mut().drag_to(target);
                            self.characters[i].wake(now);
                        }
                    }
                    _ => {}
                },
                WindowEvent::Lift { hwnd } => match self.pointer {
                    Pointer::Dragging { hwnd: dragged } if dragged == hwnd => {
                        if let Some(i) = self.find(hwnd) {
                            self.characters[i].actor_mut().release(&self.motions);
                            self.characters[i].wake(now);
                        }
                        self.pointer = Pointer::Idle;
                    }
                    // 짧게 눌렀다 뗐다. 클릭이다.
                    Pointer::Pressing { hwnd: pressed, .. } if pressed == hwnd => {
                        if let Some(i) = self.find(hwnd) {
                            self.characters[i].actor_mut().poke(&self.motions);
                            self.characters[i].wake(now);
                        }
                        self.pointer = Pointer::Idle;
                    }
                    _ => {}
                },
                WindowEvent::Hover { hwnd, x, y } => {
                    if let (Some((last_hwnd, at, when)), Some(i)) = (self.hover, self.find(hwnd)) {
                        // 가로와 세로로 움직인 거리를 더한다(데모와 같은 셈법).
                        let distance = ((x - at.0).abs() + (y - at.1).abs()) as f64;
                        let connected = last_hwnd == hwnd
                            && now.duration_since(when) <= RUB_GAP
                            && distance <= RUB_JUMP * self.characters[i].scale().factor();
                        if connected && distance > 0.0 {
                            self.characters[i].rub(&self.motions, (x, y), distance, now);
                        }
                    }
                    self.hover = Some((hwnd, (x, y), now));
                }
                // 꽃핀을 오른쪽 클릭해도 트레이 아이콘과 같은 메뉴가 뜬다.
                WindowEvent::Menu { x, y } => flow = Flow::Menu(x, y),
                WindowEvent::StageChanged => stage_changed = true,
            }
        }
        // 누른 채로 가만히 있어도 시간이 지나면 잡는다.
        if let Pointer::Pressing { hwnd, since, .. } = self.pointer
            && now >= since + CLICK_HOLD
        {
            self.start_drag(hwnd, win::cursor_position(), now);
        }

        // 2. 트레이 아이콘과 설정 창에서 온 일.
        for event in tray::take_events() {
            match event {
                TrayEvent::Menu { x, y } => flow = Flow::Menu(x, y),
                TrayEvent::Open => self.open_settings(),
                TrayEvent::TaskbarCreated => self.tray.add(),
                // 앱을 한 번 더 켰다. 꽃핀이 어디 있는지 알려 주는 일은 아직 하지 않는다.
                TrayEvent::Hello => self.dev.hellos += 1,
            }
        }
        for event in settings_window::take_events() {
            if let Some(tab) = self.dialog_event(event, now) {
                flow = Flow::ConfirmReset(tab);
            }
        }

        // 3. 모니터가 바뀌었으면 무대를 다시 읽는다. 화면 밖에 남은 꽃핀은 주 모니터에 다시 나타난다.
        if stage_changed {
            self.stage = Stage::read();
            for i in 0..self.characters.len() {
                let (x, y) = self.characters[i].actor().position();
                if !self.stage.contains(x.round() as i32, y.round() as i32 - 1) {
                    let work = self.stage.primary().work;
                    let spot = ((work.x + work.w / 2) as f64, (work.y + work.h * 3 / 10) as f64);
                    let seed = self.next_seed();
                    let character = Character::appear(
                        &mut self.frames,
                        &self.motions,
                        &self.stage,
                        self.look,
                        spot,
                        (now, self.started),
                        seed,
                    )?;
                    self.characters[i] = character;
                    self.apply_habits();
                }
                self.characters[i].wake(now);
            }
        }

        // 4. 사용자가 자리를 비웠는지 가끔 본다. 비웠으면 꽃핀도 잠들고, 돌아오면 깬다.
        if now >= self.next_away_check && !self.away_forced {
            self.next_away_check = now + AWAY_CHECK;
            let idle = Duration::from_secs_f64(win::idle_seconds());
            let away = self.away_after().is_some_and(|after| idle >= after);
            if away != self.away {
                self.set_away(away, now);
            }
        }

        // 5. 만드는 동안 확인용: 정한 시각에 시킨 일과 차례로 보여 줄 동작.
        while self.dev.script.front().is_some_and(|step| now >= self.started + step.at) {
            let Some(step) = self.dev.script.pop_front() else { break };
            match self.scripted(step.command, now)? {
                Flow::Quit => quit_asked = true,
                Flow::Continue => {}
                other => flow = other,
            }
        }
        if let Some(&motion) = self.dev.play.front()
            && self.characters[0].actor().mode() == Mode::Ground
        {
            self.dev.play.pop_front();
            self.characters[0].actor_mut().play(motion);
            self.characters[0].wake(now);
        }

        // 6. 끝내기: 꽃핀들이 손을 흔들고 사라진 뒤에 끝낸다. 한 번 더 고르면 바로 끝낸다.
        if quit_asked && self.ask_quit(now) {
            return Ok(Flow::Quit);
        }

        // 7. 때가 된 꽃핀의 시간을 흘리고 화면에 넘긴다. 한 명도 남지 않으면 끝낸다.
        if !self.tick_characters(now)? {
            return Ok(Flow::Quit);
        }

        // 8. 기본 꽃핀이 내려앉은 자리를 기억해 둔다. 다시 켜면 그 위에 나타난다.
        let first = self.characters[0].actor();
        if first.mode() == Mode::Ground {
            let spot = Some((first.home().round() as i32, first.position().1.round() as i32));
            if self.state.character != spot {
                self.state.character = spot;
                self.store.state_changed(now);
            }
        }

        // 9. 설정 창이 열려 있으면 "지금 쓰는 자원"을 1초마다 고친다.
        if let Some(window) = &self.window
            && now >= self.next_usage
        {
            window.set_usage(usage::memory().0 as f64 / usage::MIB, self.meter.cpu_percent());
            self.meter = Meter::start();
            self.next_usage = now + USAGE_EVERY;
        }

        // 10. 모아 둔 것을 저장한다.
        self.store.save(Some(now), &self.settings, &self.state);
        Ok(flow)
    }

    /// 때가 된 꽃핀의 시간을 흘리고 화면에 넘긴다. 다 사라진 꽃핀은 창을 닫는다.
    /// 한 명도 남지 않았으면 `false`를 돌려준다.
    ///
    /// 메뉴나 물음 창이 떠 있는 동안에도 이것만은 계속 불린다.
    pub fn tick_characters(&mut self, now: Instant) -> Result<bool, PackError> {
        // 깨어난 김에 곧 올 박자까지 함께 밟는다.
        let horizon = now + character::LOOKAHEAD;
        // 다음 장을 그리기 바로 전에, 앞 장이 화면에 제대로 나와 있는지 본다. 첫 꽃핀이 멈춰 있을 때만 본다.
        if let (Some(check), Some(first)) = (&mut self.dev.screen_check, self.characters.first())
            && horizon >= first.due()
            && !first.moved()
        {
            check.sample(first.window(), now);
        }
        let started = Instant::now();
        let mut shown = false;
        for character in &mut self.characters {
            shown |= character.tick(&mut self.frames, &self.motions, &self.stage, horizon)?;
        }
        if let (true, Some(first)) = (shown, self.characters.first()) {
            self.dev.pace.frame_shown(now, started.elapsed());
            self.dev.trace.note((now - self.started).as_secs_f64(), first.actor(), &self.motions);
        }
        if let Some(first) = self.characters.first()
            && !self.dev.click_checked
            && first.actor().mode() == Mode::Ground
        {
            // 바닥에 선 뒤에 한 번, 몸은 클릭을 받고 투명한 곳은 뒤 창으로 넘기는지 본다.
            self.dev.click_checked = true;
            self.dev.notes.push(dev::click_check(first.window()));
        }
        self.characters.retain(|character| !character.actor().is_gone());
        Ok(!self.characters.is_empty())
    }

    /// 다음에 할 일이 있을 때까지 잔다. 창 메시지가 오면 그 전에 깬다.
    pub fn sleep(&self) {
        let mut wake = self.characters.iter().map(Character::due).min();
        let mut sooner = |limit: Option<Instant>| {
            if let Some(limit) = limit {
                wake = Some(wake.map_or(limit, |wake| wake.min(limit)));
            }
        };
        if let Pointer::Pressing { since, .. } = self.pointer {
            sooner(Some(since + CLICK_HOLD));
        }
        if !self.away_forced && self.away_after().is_some() {
            sooner(Some(self.next_away_check));
        }
        sooner(self.window.is_some().then_some(self.next_usage));
        sooner(self.store.due());
        sooner(self.dev.deadline);
        sooner(self.quitting);
        sooner(self.dev.script.front().map(|step| self.started + step.at));
        self.timer.sleep_until(wake);
    }

    /// 메뉴를 띄울 때 주인이 될 창.
    pub fn menu_owner(&self) -> HWND {
        self.tray.hwnd()
    }

    /// 설정 창. 물음 창을 그 위에 띄우는 데 쓴다.
    pub fn settings_hwnd(&self) -> Option<HWND> {
        self.window.as_ref().map(SettingsWindow::hwnd)
    }

    /// 메뉴에서 고른 것을 한다. 끝내야 하면 `true`를 돌려준다.
    pub fn picked(&mut self, item: Option<MenuItem>, now: Instant) -> bool {
        match item {
            Some(MenuItem::Quit) => return self.ask_quit(now),
            Some(MenuItem::Settings) => self.open_settings(),
            Some(MenuItem::Link(index)) => self.open(LINKS[index].url),
            None => {}
        }
        false
    }

    /// 물음 창의 답을 받아, "예"였으면 설정 창의 그 탭을 기본값으로 되돌린다.
    pub fn reset_confirmed(&mut self, tab: usize, yes: bool, now: Instant) {
        if yes {
            self.settings.reset_tab(tab);
            if let Some(window) = &self.window {
                window.fill(&self.settings);
            }
            self.apply_settings(now);
        }
    }

    /// 만드는 동안 확인용: 물음 창을 띄우지 않고 "예"라고 답한 것으로 치는가.
    pub fn answers_for_me(&self) -> bool {
        !self.dev.options.script.is_empty()
    }

    /// 끝내기를 고른 것으로 한다. 지금 바로 끝내야 하면 `true`를 돌려준다.
    fn ask_quit(&mut self, now: Instant) -> bool {
        if self.quitting.is_some() {
            return true;
        }
        self.quitting = Some(now + QUIT_LIMIT);
        for character in &mut self.characters {
            character.actor_mut().leave(&self.motions);
            character.wake(now);
        }
        false
    }

    fn set_away(&mut self, away: bool, now: Instant) {
        self.away = away;
        for character in &mut self.characters {
            character.actor_mut().set_away(away);
            character.wake(now);
        }
    }

    /// 눌린 꽃핀을 잡는다. 잡을 수 없게 됐으면 누르고 있던 것을 그만둔다.
    fn start_drag(&mut self, hwnd: HWND, cursor: (i32, i32), now: Instant) {
        self.pointer = Pointer::Idle;
        if let Some(i) = self.find(hwnd) {
            let target = self.characters[i].drag_target(cursor, &self.stage);
            if self.characters[i].actor_mut().grab(&self.motions, target) {
                self.characters[i].wake(now);
                self.pointer = Pointer::Dragging { hwnd };
                return;
            }
        }
        character_window::stop_pressing();
    }

    /// 주소나 폴더를 연다. 만드는 동안 확인용으로 열지 않고 적기만 하게 할 수 있다.
    fn open(&mut self, target: &str) {
        if self.dev.options.no_open {
            self.dev.notes.push(format!("열기: {target}"));
        } else {
            win::open(target);
        }
    }

    /// 설정 창을 연다. 이미 열려 있으면 앞으로 가져온다.
    fn open_settings(&mut self) {
        if let Some(window) = &self.window {
            window.bring_to_front();
            return;
        }
        // 마우스 커서가 있는 모니터의 가운데에 연다.
        let cursor = win::cursor_position();
        let work = self.stage.nearest(cursor.0, cursor.1).work;
        let center = (work.x + work.w / 2, work.y + work.h / 2);
        let first = self.motions.clip(self.motions.idle2).first_frame;
        let Ok(face) = self.frames.portrait(first, ABOUT_ICON as usize) else { return };
        let pack = self.frames.pack().header();
        let commit = env!("DFP_COMMIT");
        let version = format!(
            "{}{}{}",
            env!("CARGO_PKG_VERSION"),
            if commit.is_empty() { String::new() } else { format!(" (커밋 {commit})") },
            if pack.placeholder { " · 대체 그림" } else { "" }
        );
        let icon = tray::make_icon(&face, ABOUT_ICON);
        self.window = SettingsWindow::open(&self.settings, self.state.settings_tab, center, icon, &version);
        self.next_usage = Instant::now();
        self.meter = Meter::start();
    }

    fn close_settings(&mut self, now: Instant) {
        if let Some(window) = self.window.take() {
            self.remember_tab(window.tab(), now);
        }
    }

    fn remember_tab(&mut self, tab: usize, now: Instant) {
        if self.state.settings_tab != tab {
            self.state.settings_tab = tab;
            self.store.state_changed(now);
        }
    }

    /// 설정 창에서 일어난 일을 처리한다. 기본값으로 되돌릴지 물어야 하면 그 탭을 돌려준다.
    fn dialog_event(&mut self, event: DialogEvent, now: Instant) -> Option<usize> {
        match event {
            DialogEvent::Changed => {
                // 바꾸면 바로 적용한다. 확인이나 취소 버튼이 없다.
                if self.window.as_ref().is_some_and(|window| window.read(&mut self.settings)) {
                    self.apply_settings(now);
                }
            }
            DialogEvent::Reset => return self.window.as_ref().map(SettingsWindow::tab),
            DialogEvent::Close => self.close_settings(now),
            DialogEvent::Tab(tab) => {
                if let Some(window) = &mut self.window {
                    window.show_tab(tab);
                }
                self.remember_tab(tab, now);
            }
            DialogEvent::Link(index) => self.open(LINKS[index].url),
            DialogEvent::OpenFolder => {
                if let Some(folder) = self.store.folder().map(|folder| folder.to_path_buf()) {
                    // 아직 아무것도 저장하지 않았으면 폴더가 없다. 열기 전에 만든다.
                    let _ = std::fs::create_dir_all(&folder);
                    self.open(&folder.to_string_lossy());
                }
            }
            // 저장소 주소가 정해지면 연다.
            DialogEvent::OpenProject => {}
        }
        None
    }

    /// 스크립트가 시킨 일을 한다. 만드는 동안 확인용이다.
    fn scripted(&mut self, command: dev::Command, now: Instant) -> Result<Flow, PackError> {
        let mut flow = Flow::Continue;
        match command {
            dev::Command::Grab => {
                self.dev.script_origin = self.characters[0].actor().position();
                let origin = self.dev.script_origin;
                self.characters[0].actor_mut().grab(&self.motions, origin);
            }
            dev::Command::Drag(dx, dy) => {
                let wanted = (self.dev.script_origin.0 + dx, self.dev.script_origin.1 + dy);
                let target = self.characters[0].drag_to_foot(wanted, &self.stage);
                self.characters[0].actor_mut().drag_to(target);
            }
            dev::Command::Release => self.characters[0].actor_mut().release(&self.motions),
            dev::Command::Leave => flow = Flow::Quit,
            dev::Command::Roam(roam) => {
                let habits = Habits { roam, ..self.characters[0].actor().habits() };
                self.characters[0].actor_mut().set_habits(habits);
            }
            dev::Command::Dpi(dpi) => {
                self.look.dpi_override = Some(dpi);
                let look = self.look;
                self.characters.iter_mut().for_each(|character| character.set_look(look, now));
            }
            dev::Command::Click => self.characters[0].actor_mut().poke(&self.motions),
            dev::Command::Rub(distance) => self.characters[0].actor_mut().rub(&self.motions, distance),
            dev::Command::Do(whim) => self.characters[0].actor_mut().start(whim, &self.motions),
            dev::Command::Away(on) => {
                self.away_forced = true;
                self.set_away(on, now);
            }
            dev::Command::Menu(id) => {
                if self.picked(MenuItem::from_id(id), now) {
                    flow = Flow::Quit;
                }
            }
            dev::Command::Settings(dialog) => {
                if dialog == dev::Dialog::Open {
                    self.open_settings();
                } else if let Some(window) = &mut self.window {
                    match dialog {
                        dev::Dialog::Open => {}
                        dev::Dialog::Close => settings_window::push_close(),
                        dev::Dialog::Tab(tab) => {
                            window.show_tab(tab.min(TAB_NAMES.len() - 1));
                            let tab = window.tab();
                            self.remember_tab(tab, now);
                        }
                        dev::Dialog::Pick(id, index) => window.pick(id, index),
                        dev::Dialog::Toggle(id) => window.toggle(id),
                        dev::Dialog::Press(id) => window.press(id),
                        dev::Dialog::Reset => flow = Flow::ConfirmReset(window.tab()),
                        dev::Dialog::Snap => {
                            self.dev.snaps += 1;
                            let numbered = |path: &std::path::Path| {
                                let stem = path.file_stem().unwrap_or_default().to_string_lossy();
                                let extension = path.extension().unwrap_or_default().to_string_lossy();
                                path.with_file_name(format!("{stem}_{}.{extension}", self.dev.snaps))
                            };
                            if let Some(path) = &self.dev.options.settings_dump {
                                window.dump(&numbered(path));
                            }
                            if let Some(path) = &self.dev.options.settings_shot {
                                window.shot(&numbered(path));
                            }
                        }
                    }
                }
            }
        }
        if let Some(first) = self.characters.first_mut() {
            first.wake(now);
        }
        Ok(flow)
    }

    /// 끝낸다. 미뤄 둔 저장을 하고, 보고서에 적을 줄을 돌려준다.
    pub fn finish(mut self) -> Vec<String> {
        // 끌 때 꽃핀이 있던 자리를 기억한다. 공중에 있었으면 마지막으로 내려앉은 자리를 둔다.
        if let Some(first) = self.characters.first().map(Character::actor)
            && first.mode() == Mode::Ground
        {
            let (x, y) = first.position();
            self.state.character = Some((x.round() as i32, y.round() as i32));
        }
        self.store.state_changed(Instant::now());
        self.store.save(None, &self.settings, &self.state);

        let mut lines = std::mem::take(&mut self.dev.notes);
        lines.push(self.dev.pace.summary());
        lines.push(self.dev.pace.cost_summary());
        if let Some(check) = &self.dev.screen_check {
            lines.push(check.summary());
        }
        if let Some(usage) = &self.dev.usage {
            lines.push(usage.summary());
        }
        if self.dev.hellos > 0 {
            lines.push(format!("실행 중에 앱을 한 번 더 켠 횟수 {}", self.dev.hellos));
        }
        lines.extend(self.dev.trace.lines());
        lines
    }
}
