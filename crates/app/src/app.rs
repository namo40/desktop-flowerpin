//! 앱 전체. 꽃핀들, 포스트잇, 트레이 아이콘, 설정 창, 저장을 한데 묶어 메인 루프를 돌린다.
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

use crate::actor::{Habits, Mode, Rng, Show};
use crate::autostart;
use crate::bubble::{Bubble, Speech};
use crate::character::{self, Character, Look, Stay};
use crate::character_window::{self, WindowEvent};
use crate::checker::Checker;
use crate::clock::WakeTimer;
use crate::desktop::Desktop;
use crate::dev;
use crate::frames::Frames;
use crate::links::LINKS;
use crate::live::{self, Event, Live, Reply, Tracker, View};
use crate::memos::{COUNT_LIMIT, Memo, Memos, SNOOZE_MINUTES};
use crate::motions::{MotionId, Motions};
use crate::notices::{Click, Kind, Notice, Notices};
use crate::postit::{Outcome, Postits};
use crate::settings::{Key, Settings};
use crate::settings_window::{self, DialogEvent, SettingsWindow, TAB_NAMES};
use crate::stage::Stage;
use crate::store::{self, State, Store};
use crate::tray::{self, MenuItem, Tray, TrayEvent};
use crate::usage::{self, Meter};
use crate::watch::{self, Watch, WatchEvent};
use crate::when::When;
use crate::win::{self, Screen};

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
/// 보이는 다른 창들을 다시 읽는 간격. 꽃핀이 떠 있을 때(어디에 내려앉을지 알아야 한다), 창 위에 서 있을 때
/// (그 자리가 가려졌는지 알아야 한다), 바닥에 있을 때(뛰어오를 곳만 알면 된다)의 차례다.
const SCAN_IN_AIR: Duration = Duration::from_millis(100);
const SCAN_ON_PERCH: Duration = Duration::from_millis(250);
const SCAN_ON_FLOOR: Duration = Duration::from_millis(1000);
/// 설정 창의 "지금 쓰는 자원"을 고치는 간격.
const USAGE_EVERY: Duration = Duration::from_secs(1);
/// 설정 창 정보 탭에 보여 줄 그림의 한 변(px).
const ABOUT_ICON: i32 = 32;
/// 알릴 메모가 남아 있는 동안 시계를 보는 간격.
const MEMO_CHECK: Duration = Duration::from_secs(1);
/// 꽃핀이 들고 있는 메모를 보여 주는 말풍선의 이름과, 떠 있는 시간.
const HELD_TAG: &str = "held";
const HELD_LASTS: Duration = Duration::from_secs(10);
/// 방송 안내 말풍선의 이름. 방송이 끝나거나 새 방송이 시작되면 이 이름으로 버린다.
const LIVE_TAG: &str = "live";
/// 방송이 끝났다는 말풍선이 떠 있는 시간.
const LIVE_END_LASTS: Duration = Duration::from_secs(10);
/// 메뉴에 적는 방송 제목의 가장 긴 길이(글자 수).
const MENU_TITLE: usize = 40;
/// 처음 켰을 때 하는 안내의 이름. 뒤의 것은 인터넷을 쓴다는 안내다.
const GREET_TAG: &str = "greet";
const GREET_LIVE_TAG: &str = "greet-live";
/// 처음 켰을 때의 인사가 떠 있는 시간. 인터넷을 쓴다는 안내는 클릭할 때까지 떠 있다.
const GREET_LASTS: Duration = Duration::from_secs(12);
/// 손을 흔들며 돌아가는 중인 꽃핀까지 쳐서, 꽃핀은 이만큼만 둔다. 잇달아 불러도 창이 끝없이 늘지 않는다.
const WINDOW_LIMIT: usize = 18;
/// 더 부른 꽃핀이 나타나는 자리. 모니터 작업 영역의 너비와 높이를 1로 볼 때, 양 끝에서 조금 떨어진
/// 아무 데나의 위쪽 공중이다.
const GUEST_ACROSS: (f64, f64) = (0.1, 0.9);
const GUEST_DOWN: (f64, f64) = (0.2, 0.35);
/// 화면을 전체 화면 프로그램이 쓰고 있는지 확인하는 간격.
const SCREEN_CHECK: Duration = Duration::from_secs(5);

/// 사용자가 메뉴에서 고른 "잠깐 숨기기".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hide {
    Off,
    /// 이 시각까지 숨긴다.
    Until(Instant),
    /// 다시 보이라고 할 때까지 숨긴다.
    UntilAsked,
}

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
    /// 화면의 이 자리에 메뉴를 띄운다. 넣을 행동은 [`App::menu_shows`]로 받는다.
    Menu(i32, i32),
    /// 설정 창의 이 탭을 기본값으로 되돌릴지 묻는다.
    ConfirmReset(usize),
    /// 글이 적힌 이 메모를 지울지 묻는다.
    ConfirmDelete(u64),
    /// 이 메모의 포스트잇을 오른쪽 클릭했다. 화면의 이 자리에 메뉴를 띄운다.
    MemoMenu(u64, i32, i32),
}

/// 방송 상태 확인에 딸린 것들.
struct LiveWatch {
    tracker: Tracker,
    /// 치지직에 묻는 스레드. 처음 물을 때 만든다. 방송 상태 확인을 꺼 두면 만들지 않는다.
    checker: Option<Checker>,
    /// 마지막으로 물은 시각.
    asked: Option<Instant>,
    /// 물어 놓고 답을 기다리는 중인가.
    waiting: bool,
    /// 마지막으로 방송 상태를 알아낸, 이 PC의 시각.
    checked: Option<When>,
    /// 트레이 아이콘에 올려 둔 글. 방송 중이 아니면 비어 있다.
    tip: String,
    /// Windows 알림을 클릭하면 방송을 여는가. 방송 시작 알림을 띄운 뒤에만 그렇다.
    toast_opens: bool,
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
    /// 보이는 창들을 읽을 때마다 걸린 시간.
    scans: Vec<Duration>,
    /// 맨 앞 창이 바뀌었다는 알림을 받은 횟수.
    foregrounds: u32,
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
    /// 트레이 아이콘의 그림과 한 변. 방송 중 표시를 찍거나 지울 때 다시 쓴다.
    face: Vec<u32>,
    icon_size: i32,
    live: LiveWatch,
    /// 처음 켰을 때의 안내를 하는 중인가.
    greeting: bool,
    window: Option<SettingsWindow>,
    memos: Memos,
    /// 메모를 화면에 붙인 포스트잇들.
    postits: Postits,
    /// 알릴 메모가 있는지 다음에 볼 시각.
    next_memo_check: Instant,
    /// 꽃핀이 들고 있는 메모 가운데 다음에 보여 줄 차례.
    held_turn: usize,
    /// 설정 창에 보여 줄 CPU를 재는 도구와, 다음에 고칠 시각.
    meter: Meter,
    next_usage: Instant,
    started: Instant,
    seed: u64,
    timer: WakeTimer,
    /// 다른 창에서 일어난 일을 전해 듣는다. 들고 있는 동안만 알림이 온다.
    _watch: Watch,
    /// 메뉴를 띄운 꽃핀의 창. 메뉴에서 고른 행동을 이 꽃핀이 한다. 트레이에서 띄웠으면 `None`이고 첫 꽃핀이 한다.
    menu_target: Option<HWND>,
    /// 메뉴가 떠 있는 동안 멈춰 세워 둔 꽃핀의 창.
    held: Option<HWND>,
    /// 말풍선으로 알릴 것들의 줄과, 지금 떠 있는 말풍선.
    notices: Notices,
    bubble: Option<Bubble>,
    /// 화면에 보이는 다른 창들과, 마지막으로 읽은 시각.
    desktop: Desktop,
    scanned: Option<Instant>,
    /// 사용자가 고른 "잠깐 숨기기".
    hide: Hide,
    /// 화면을 누가 쓰고 있는지와, 다음에 확인할 시각. 스크립트가 정했으면 진짜 화면은 보지 않는다.
    screen: Screen,
    screen_forced: bool,
    next_screen_check: Instant,
    /// 화면이 잠겨 있는가, PC가 절전에 들어갔는가.
    locked: bool,
    asleep: bool,
    /// 지금 꽃핀을 숨겨 두었는가.
    out_of_sight: bool,
    /// 방해하면 안 되는 동안 알릴 때가 된 메모들. 그때가 지나면 포스트잇을 울린다.
    rings_waiting: Vec<u64>,
    dev: Dev,
}

/// 꽃핀들을 뒤에 설 것부터 앞에 설 것까지 차례로 늘어놓는다. `guests`는 꽃핀마다 더 부른 꽃핀인지다.
///
/// 더 부른 꽃핀은 먼저 부른 것일수록 뒤에 서고, 기본 꽃핀은 그 앞에 선다. 잡고 있는 꽃핀은 맨 앞이다.
fn stack_order(guests: &[bool], dragged: Option<usize>) -> Vec<usize> {
    let mut order: Vec<usize> = (0..guests.len()).filter(|&index| guests[index]).collect();
    order.extend((0..guests.len()).rev().filter(|&index| !guests[index]));
    if let Some(dragged) = dragged.filter(|&dragged| dragged < guests.len()) {
        order.retain(|&index| index != dragged);
        order.push(dragged);
    }
    order
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
        let stage = Stage::read().pretend_dpi(&options.monitor_dpi);
        let started = Instant::now();

        // 정한 시간만 돌고 끝나는 확인용 실행은 폴더를 따로 주지 않으면 파일을 읽지도 쓰지도 않는다.
        let folder =
            options.data_dir.clone().or_else(|| options.run_for.is_none().then(store::default_folder).flatten());
        let (store, settings, state, mut memos, mut notes) = Store::open(folder);
        let wall = When::from_seconds(options.clock.unwrap_or_else(win::local_seconds));
        memos.settle(wall);
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
        let mut tray = Tray::new(&face, icon_size);
        // Windows 알림에는 같은 얼굴을 더 크게 넣는다.
        let toast_size = tray::toast_icon_size();
        let toast_face = frames.portrait(motions.clip(motions.idle2).first_frame, toast_size as usize)?;
        tray.set_toast_icon(&toast_face, toast_size);
        let live = LiveWatch {
            tracker: Tracker::new(state.announced_live.clone()),
            checker: None,
            asked: None,
            waiting: false,
            checked: None,
            tip: String::new(),
            toast_opens: false,
        };

        let seed = options
            .seed
            .unwrap_or_else(|| SystemTime::now().duration_since(UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64));
        // 포스트잇은 놓인 모니터의 화면 배율을 따른다. 모니터를 알 수 없을 때는 주 모니터의 것을 쓴다.
        let postit_dpi = options.dpi.unwrap_or(stage.primary().dpi);
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
            face,
            icon_size,
            live,
            greeting: false,
            window: None,
            memos,
            // 확인용으로 돌릴 때는 글을 적는 칸을 띄워도 쓰던 창에서 키보드 입력을 가져오지 않는다.
            postits: Postits::new(postit_dpi, options.run_for.is_some(), seed, wall),
            next_memo_check: started,
            held_turn: 0,
            meter: Meter::start(),
            next_usage: started,
            started,
            seed,
            timer: WakeTimer::new(),
            _watch: Watch::start(),
            menu_target: None,
            held: None,
            notices: Notices::default(),
            bubble: None,
            desktop: Desktop::default(),
            scanned: None,
            hide: Hide::Off,
            screen: Screen::Free,
            screen_forced: false,
            next_screen_check: started,
            locked: false,
            asleep: false,
            out_of_sight: false,
            rings_waiting: Vec::new(),
            dev: Dev {
                pace: dev::Pace::default(),
                trace: dev::Trace::default(),
                screen_check: options.check_screen.then(|| dev::ScreenCheck::new(started)),
                usage: None,
                click_checked: false,
                script: options.script.iter().cloned().collect(),
                script_origin: (0.0, 0.0),
                play,
                hellos: 0,
                scans: Vec::new(),
                foregrounds: 0,
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
        app.tell_postits_monitors();
        app.describe();
        app.sync_autostart(started);
        app.greet();
        // 적어 둔 메모를 포스트잇으로 붙인다. 붙인 적이 없는 메모에 자리를 잡아 줬으면 저장한다.
        app.postits.attach(&mut app.memos);
        if !app.postits.commit(&mut app.memos).is_empty() {
            app.store.memos_changed(started);
        }
        if app.store.memos_lost() {
            // 사용자가 직접 쓴 것이라, 읽지 못했으면 조용히 넘어가지 않고 알린다.
            app.notices.push(Notice {
                kind: Kind::Note,
                speech: Speech {
                    title: "메모를 읽지 못했어요".to_string(),
                    body: "파일은 memos.bad.json으로 남겨 뒀어요. 메모는 빈 채로 시작해요.".to_string(),
                },
                lasts: None,
                click: Click::Close,
                tag: None,
            });
        }
        Ok(app)
    }

    /// 포스트잇에게 모니터마다의 범위와 화면 배율을 알려 준다. 포스트잇은 자기가 놓인 모니터의 배율로 그린다.
    fn tell_postits_monitors(&mut self) {
        let forced = self.dev.options.dpi;
        let monitors: Vec<_> =
            self.stage.monitors().iter().map(|monitor| (monitor.area, forced.unwrap_or(monitor.dpi))).collect();
        self.postits.set_monitors(&monitors);
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
        lines.push(if !self.live_enabled() {
            "방송 상태 확인: 꺼짐".to_string()
        } else if self.live_online() {
            format!("방송 상태 확인: {}분마다 {}에 물음", self.settings.number(Key::LiveCheckInterval), live::HOST)
        } else {
            "방송 상태 확인: 켜짐. 확인용 실행이라 인터넷은 쓰지 않음".to_string()
        });
        let pending = self.memos.list().iter().filter(|memo| memo.next.is_some()).count();
        lines.push(format!("메모 {}개, 알릴 것 {pending}개", self.memos.list().len()));
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
        if let Some(bubble) = &self.bubble {
            bubble.window().set_topmost(look.topmost);
        }
        self.apply_habits();
        if !self.live_enabled() {
            // 방송 상태 확인을 껐다. 알던 것을 잊고, 방송 안내와 아직 띄우지 않은 인터넷 안내를 치운다.
            if *self.live.tracker.view() != View::Unknown {
                self.live.tracker = Tracker::new(self.live.tracker.announced().map(str::to_string));
                self.live.checked = None;
            }
            if self.notices.cancel(LIVE_TAG) | self.notices.cancel(GREET_LIVE_TAG) {
                self.close_bubble(now);
            }
        }
        self.show_live();
        self.sync_autostart(now);
        self.store.settings_changed(now);
    }

    /// "Windows 시작할 때 실행" 설정대로 레지스트리를 맞춘다. 켜 두었으면 이 실행 파일의 자리를 적고,
    /// 꺼 두었으면 적힌 것을 지운다. 정한 시간만 도는 확인용 실행은 적을 곳을 따로 줬을 때만 한다.
    fn sync_autostart(&mut self, now: Instant) {
        let key = match (&self.dev.options.run_key, self.dev.options.run_for) {
            (Some(key), _) => key.clone(),
            (None, None) => autostart::RUN_KEY.to_string(),
            (None, Some(_)) => return,
        };
        let wanted = self.settings.is_on(Key::StartWithWindows);
        let exe = std::env::current_exe().ok();
        // 켜 두었는데 이 실행 파일의 자리를 모르면, 적혀 있는 것을 건드리지 않는다.
        if wanted && exe.is_none() {
            return;
        }
        if autostart::set(&key, exe.as_deref().filter(|_| wanted)) {
            self.note(now, format!("Windows 시작할 때 실행: {}", if wanted { "적어 둠" } else { "지움" }));
        }
    }

    /// 꽃핀들이 잠깐 반짝여, 어디 있는지 알려 준다. 다른 창에 가려 있었으면 앞으로 나온다.
    /// 메뉴로 숨겨 둔 중이었으면 다시 보인다.
    fn blink_all(&mut self, now: Instant) {
        if self.hide != Hide::Off {
            self.hide = Hide::Off;
            self.apply_hiding(now);
        }
        if self.out_of_sight {
            return;
        }
        self.note(now, format!("꽃핀 {}명이 반짝임", self.characters.len()));
        self.characters.iter_mut().for_each(|character| character.blink(now));
        if !self.look.topmost {
            // "항상 위"가 꺼져 있으면 다른 창 뒤에 있을 수 있다. 앞에 설 꽃핀을 올리고 나머지를 그 뒤에 세운다.
            if let Some(front) = self.home().or(self.characters.last()) {
                front.window().raise();
            }
        }
        self.restack();
    }

    /// 처음 켰을 때 한 번, 쓰는 법과 인터넷을 쓴다는 것을 말풍선으로 알린다.
    fn greet(&mut self) {
        // 정한 시간만 도는 확인용 실행에서는 시켰을 때만 한다.
        if self.state.greeted || (self.dev.options.run_for.is_some() && !self.dev.options.greet) {
            return;
        }
        let note = |title: &str, body: String, lasts, tag| Notice {
            kind: Kind::Note,
            speech: Speech { title: title.to_string(), body },
            lasts,
            click: Click::Close,
            tag: Some(tag),
        };
        let hello = "나를 오른쪽 클릭하면 메뉴가 나와요.".to_string();
        self.notices.push(note("안녕!", hello, Some(GREET_LASTS), GREET_TAG));
        if self.live_enabled() {
            let body = format!(
                "방송이 켜지면 알려 주려고 {}분마다 치지직에 물어봐요. 치지직의 공식 API는 아니에요. 설정에서 끌 수 있어요.",
                self.settings.number(Key::LiveCheckInterval)
            );
            self.notices.push(note("방송 알림", body, None, GREET_LIVE_TAG));
        }
        self.greeting = true;
    }

    /// 방송 상태 확인이 켜져 있는가.
    fn live_enabled(&self) -> bool {
        self.settings.is_on(Key::LiveCheck)
    }

    /// 치지직에 진짜로 물어도 되는가. 정한 시간만 도는 확인용 실행은 시켰을 때만 묻는다.
    fn live_online(&self) -> bool {
        self.live_enabled()
            && self.quitting.is_none()
            && (self.dev.options.run_for.is_none() || self.dev.options.live_check)
    }

    /// 치지직에 다음으로 물을 시각. 묻지 않을 때는 `None`이다.
    fn live_due(&self) -> Option<Instant> {
        // 처음 켰을 때는 인터넷을 쓴다는 안내를 띄운 뒤부터 묻는다.
        if !self.live_online() || self.live.waiting || self.notices.is_waiting(GREET_LIVE_TAG) {
            return None;
        }
        let interval = Duration::from_secs(self.settings.number(Key::LiveCheckInterval).max(1) as u64 * 60);
        // 주소가 바뀌었으면 더 묻지 않는다. 실패했으면 간격이 달라진다.
        let wait = self.live.tracker.wait(interval)?;
        Some(self.live.asked.map_or(self.started, |asked| asked + wait))
    }

    /// 치지직에서 온 답을 받고, 때가 됐으면 다시 묻는다.
    fn check_live(&mut self, now: Instant) {
        while let Some(reply) = self.live.checker.as_ref().and_then(Checker::take) {
            self.live.waiting = false;
            self.live_reply(reply, now);
        }
        if self.live_due().is_none_or(|due| now < due) {
            return;
        }
        if self.live.checker.is_none() {
            self.live.checker = Checker::start(self.tray.hwnd());
        }
        // 스레드를 만들지 못했어도 물은 것으로 적어, 다음 때까지 기다리게 한다.
        self.live.asked = Some(now);
        if let Some(checker) = &self.live.checker {
            checker.ask();
            self.live.waiting = true;
            self.note(now, "방송 상태를 물음".to_string());
        }
    }

    /// 방송 상태에 대한 답 하나를 받아, 알릴 일이 있으면 알린다.
    fn live_reply(&mut self, reply: Reply, now: Instant) {
        // 방송 상태 확인을 끈 뒤에 온 답은 버린다.
        if !self.live_enabled() {
            return;
        }
        self.note(now, format!("방송 상태의 답: {reply:?}"));
        if matches!(reply, Reply::Open(_) | Reply::Closed) {
            self.live.checked = Some(self.wall(now));
        }
        let event = self.live.tracker.feed(reply);
        // 안내한 방송을 적어 둔다. 앱을 다시 켜도 같은 방송을 또 안내하지 않는다.
        if self.state.announced_live.as_deref() != self.live.tracker.announced() {
            self.state.announced_live = self.live.tracker.announced().map(str::to_string);
            self.store.state_changed(now);
        }
        match event {
            Some(Event::Started(live)) => self.announce_live(&live, true, now),
            Some(Event::AlreadyLive(live)) => self.announce_live(&live, false, now),
            Some(Event::Ended) => self.announce_end(now),
            None => {}
        }
        self.show_live();
    }

    /// 방송이 켜져 있다고 알린다. `fresh`는 앱이 켜져 있는 동안 방송이 시작됐다는 뜻이다.
    fn announce_live(&mut self, live: &Live, fresh: bool, now: Instant) {
        let how = self.settings.word(Key::LiveStartNotice);
        if how == "off" {
            return;
        }
        // 앞선 방송의 안내가 아직 남아 있으면 치운다.
        self.drop_live_notices(now);
        let title = if fresh { "방송 시작!" } else { "지금 방송 중이에요!" };
        self.notices.push(Notice {
            kind: Kind::Live,
            speech: Speech { title: title.to_string(), body: live.title.clone() },
            lasts: None,
            click: Click::Open(LINKS[0].url.to_string()),
            tag: Some(LIVE_TAG),
        });
        if !fresh {
            return;
        }
        // 나와 있는 꽃핀이 모두 함께 놀란다.
        self.surprise_all(now);
        if how == "bubble_toast" {
            self.toast("꽃핀 방송 시작!", &live.title, true, now);
        }
    }

    /// 방송이 끝났다고 알린다.
    fn announce_end(&mut self, now: Instant) {
        // 시작 안내가 아직 떠 있거나 줄에 있으면 때늦은 것이므로 버린다.
        self.drop_live_notices(now);
        let how = self.settings.word(Key::LiveEndNotice);
        if how == "off" {
            return;
        }
        // 꽃핀을 숨겨 둔 동안에는 말풍선을 줄에 세우지 않는다. 다시 보일 때 띄우면 때늦은 말이 된다.
        if !self.out_of_sight {
            self.notices.push(Notice {
                kind: Kind::Live,
                speech: Speech { title: "오늘 방송 끝!".to_string(), body: "수고했어요.".to_string() },
                lasts: Some(LIVE_END_LASTS),
                click: Click::Close,
                tag: Some(LIVE_TAG),
            });
        }
        if how == "bubble_toast" {
            self.toast("꽃핀 방송 끝", "오늘 방송이 끝났어요.", false, now);
        }
    }

    /// 떠 있거나 줄에 있는 방송 안내를 치운다.
    fn drop_live_notices(&mut self, now: Instant) {
        if self.notices.cancel(LIVE_TAG) {
            self.close_bubble(now);
        }
    }

    /// Windows 알림을 띄운다. `opens`는 클릭하면 방송을 여는지다.
    fn toast(&mut self, title: &str, body: &str, opens: bool, now: Instant) {
        self.live.toast_opens = opens;
        self.note(now, format!("Windows 알림: {title} / {body}"));
        // 정한 시간만 도는 확인용 실행에서는 시켰을 때만 진짜로 띄운다.
        if self.dev.options.run_for.is_none() || self.dev.options.toast {
            let taken = self.tray.notify(title, body);
            self.note(now, format!("Windows 알림을 띄움: {}", if taken { "받아 줌" } else { "받아 주지 않음" }));
        }
    }

    /// Windows 알림을 클릭했다.
    fn toast_clicked(&mut self, now: Instant) {
        if self.live.toast_opens {
            self.watch(now);
        }
    }

    /// 방송을 브라우저로 연다. 보러 갔으니 방송 안내 말풍선은 치운다.
    fn watch(&mut self, now: Instant) {
        self.drop_live_notices(now);
        self.open(LINKS[0].url);
    }

    /// 방송에 대해 아는 것을 트레이 아이콘과 설정 창에 보여 준다.
    fn show_live(&mut self) {
        let enabled = self.live_enabled();
        let tip = self.live.tracker.live().filter(|_| enabled).map(live::tip).unwrap_or_default();
        if tip != self.live.tip {
            // 방송 중이면 아이콘에 빨간 점을 찍는다.
            if tip.is_empty() {
                self.tray.set(&self.face, self.icon_size, "");
            } else {
                self.tray.set(&tray::with_dot(&self.face, self.icon_size), self.icon_size, &tip);
            }
            if self.dev.options.report.is_some() {
                self.dev
                    .notes
                    .push(format!("트레이 아이콘: {}", if tip.is_empty() { "방송 중 표시를 지움" } else { &tip }));
            }
            self.live.tip = tip;
        }
        if let Some(window) = &self.window {
            window.set_live_status(&live::status_line(self.live.tracker.view(), enabled, self.live.checked));
        }
    }

    /// 방송 중이면 메뉴 맨 위에 넣을 방송 제목과 방송 정보 한 줄.
    pub fn live_menu(&self) -> Option<(String, String)> {
        let live = self.live.tracker.live().filter(|_| self.live_enabled())?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs() as i64);
        Some((live::shorten(&live.title, MENU_TITLE), live::details(live, now)))
    }

    /// 확인용 보고서에 일어난 일을 시각과 함께 적는다. 보고서를 쓰지 않으면 아무것도 하지 않는다.
    fn note(&mut self, now: Instant, text: String) {
        if self.dev.options.report.is_some() {
            let at = now.saturating_duration_since(self.started).as_secs_f64();
            self.dev.notes.push(format!("{at:.2}초 {text}"));
        }
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

    /// 기본 꽃핀. 끝내는 중에 먼저 사라졌으면 없다.
    fn home(&self) -> Option<&Character> {
        self.characters.first().filter(|character| character.stay() == Stay::Home)
    }

    /// 머무르고 있는, 더 부른 꽃핀들이 몇 번째인지. 먼저 부른 것부터다. 돌아가는 중인 꽃핀은 치지 않는다.
    fn guests(&self) -> impl Iterator<Item = usize> + '_ {
        let staying =
            |character: &Character| matches!(character.stay(), Stay::Guest(_)) && !character.actor().is_leaving();
        (0..self.characters.len()).filter(move |&index| staying(&self.characters[index]))
    }

    /// 머무르고 있는 꽃핀의 수. 기본 꽃핀도 세고, 돌아가는 중인 꽃핀은 세지 않는다.
    fn staying(&self) -> usize {
        self.characters.iter().filter(|character| !character.actor().is_leaving()).count()
    }

    /// 한 번에 나와 있을 수 있는 꽃핀의 수. 기본 꽃핀도 센다.
    fn crowd_limit(&self) -> usize {
        self.settings.number(Key::MaxCharacters).clamp(1, 9) as usize
    }

    /// 더 부른 꽃핀이 머무르는 시간. 보낼 때까지 머무르면 `None`이다.
    fn stay_time(&self) -> Option<Duration> {
        let seconds = self.settings.number(Key::Stay);
        (seconds > 0).then(|| Duration::from_secs(seconds as u64))
    }

    /// 머무를 시간이 가장 먼저 끝나는 꽃핀의 그 시각.
    fn guests_due(&self) -> Option<Instant> {
        let stay = self.stay_time()?;
        let leaves = |index: usize| match self.characters[index].stay() {
            Stay::Guest(since) => Some(since + stay),
            Stay::Home => None,
        };
        self.guests().filter_map(leaves).min()
    }

    /// 화면을 쓰지 않는 때인가. 잠겨 있거나, 절전 중이거나, 화면 보호기가 떠 있으면 그렇다.
    /// 이때는 방송 상태도 묻지 않고 알림 시각도 보지 않는다. 돌아오면 그때 한꺼번에 본다.
    fn is_paused(&self) -> bool {
        self.locked || self.asleep || self.screen == Screen::Absent
    }

    /// 방해하면 안 되는 때인가. 전체 화면 프로그램(게임, 발표, 영상)이 떠 있거나 화면을 쓰지 않는 때다.
    /// 꽃핀을 숨기고, 메모 알림의 포스트잇도 이때가 지난 뒤에 울린다.
    fn is_quiet(&self) -> bool {
        self.is_paused() || (self.screen == Screen::Busy && self.settings.is_on(Key::HideOnFullscreen))
    }

    /// 전체 화면 프로그램이 떠 있는지를 때맞춰 보는가. 정한 시간만 도는 확인용 실행은 시켰을 때만 본다.
    fn watches_screen(&self) -> bool {
        !self.screen_forced && (self.dev.options.run_for.is_none() || self.dev.options.screen_check)
    }

    /// 꽃핀을 숨길 때가 됐으면 숨기고, 다시 보일 때가 됐으면 보인다. 숨겨 둔 동안에는 꽃핀의 시간이 흐르지 않는다.
    fn apply_hiding(&mut self, now: Instant) {
        if let Hide::Until(at) = self.hide
            && now >= at
        {
            self.hide = Hide::Off;
        }
        let hidden = self.hide != Hide::Off || self.is_quiet();
        if hidden != self.out_of_sight {
            self.out_of_sight = hidden;
            if hidden {
                let why = if self.is_paused() {
                    "화면을 쓰지 않음"
                } else if self.is_quiet() {
                    "전체 화면"
                } else {
                    "잠깐 숨기기"
                };
                self.note(now, format!("꽃핀을 숨김: {why}"));
                // 잡고 있던 꽃핀은 놓는다.
                if let Pointer::Dragging { hwnd } = self.pointer
                    && let Some(index) = self.find(hwnd)
                {
                    self.characters[index].actor_mut().release(&self.motions);
                }
                if !matches!(self.pointer, Pointer::Idle) {
                    self.pointer = Pointer::Idle;
                    character_window::stop_pressing();
                }
                self.hover = None;
                // 떠 있던 말풍선은 닫는다. 클릭해야 닫히는 것은 줄에 다시 세워, 다시 보일 때 띄운다.
                if let Some(notice) = self.notices.dismiss()
                    && notice.lasts.is_none()
                {
                    self.notices.push(notice);
                }
                self.close_bubble(now);
            } else {
                self.note(now, "꽃핀을 다시 보임".to_string());
                // 창들의 앞뒤가 바뀌었을 것이다.
                self.scanned = None;
            }
            for character in &mut self.characters {
                character.set_visible(!hidden, now);
            }
            if !hidden {
                self.restack();
            }
        }
        // 방해하면 안 되는 동안 미뤄 둔 알림을 울린다.
        if !self.is_quiet() && !self.rings_waiting.is_empty() {
            for id in std::mem::take(&mut self.rings_waiting) {
                self.postits.ring(&mut self.memos, id);
            }
            self.surprise_all(now);
        }
    }

    /// 나와 있는 꽃핀이 모두 놀란다. 떠 있거나 잡혀 있는 꽃핀은 건너뛴다. 숨겨 둔 동안에는 아무도 놀라지 않는다.
    fn surprise_all(&mut self, now: Instant) {
        if self.out_of_sight {
            return;
        }
        for character in &mut self.characters {
            character.show(Show::Surprise, &self.motions, &self.stage, now);
        }
    }

    /// 꽃핀을 하나 더 부를 수 있는가. 자리가 없어도 먼저 부른 꽃핀을 돌려보내면 부를 수 있다.
    /// 한 번에 한 명만 두기로 했으면 부를 수 없다. 숨겨 둔 동안에도 부를 수 없다.
    pub fn can_call(&self) -> bool {
        self.quitting.is_none()
            && !self.out_of_sight
            && self.characters.len() < WINDOW_LIMIT
            && (self.staying() < self.crowd_limit() || self.guests().next().is_some())
    }

    /// 돌려보낼 꽃핀이 있는가.
    pub fn can_send(&self) -> bool {
        self.guests().next().is_some()
    }

    /// 메뉴를 띄운 꽃핀이 더 부른 꽃핀인가.
    pub fn menu_on_guest(&self) -> bool {
        self.menu_guest().is_some()
    }

    /// 메뉴를 띄운 꽃핀이 머무르고 있는, 더 부른 꽃핀이면 그 꽃핀이 몇 번째인지.
    fn menu_guest(&self) -> Option<usize> {
        let index = self.menu_target.and_then(|hwnd| self.find(hwnd))?;
        self.guests().find(|&guest| guest == index)
    }

    /// 꽃핀을 하나 더 부른다. 메뉴를 띄운 꽃핀이 있는 모니터의 위쪽 공중에 나타나 떨어진다.
    /// 트레이에서 불렀으면 기본 꽃핀이 있는 모니터다.
    fn call_guest(&mut self, now: Instant) -> Result<(), PackError> {
        if !self.can_call() {
            return Ok(());
        }
        // 자리가 없으면 가장 먼저 부른 꽃핀부터 손을 흔들며 돌아간다.
        while self.staying() >= self.crowd_limit() {
            let Some(oldest) = self.guests().next() else { break };
            self.send(oldest, now);
        }
        let work = match self.characters.get(self.menu_character()).map(|character| character.actor().position()) {
            Some((x, y)) => self.stage.nearest(x.round() as i32, y.round() as i32 - 1).work,
            None => self.stage.primary().work,
        };
        let mut rng = Rng::new(self.next_seed());
        let spot = (
            work.x as f64 + work.w as f64 * rng.range(GUEST_ACROSS),
            work.y as f64 + work.h as f64 * rng.range(GUEST_DOWN),
        );
        let seed = self.next_seed();
        let beats = (now, self.started);
        let mut guest = Character::appear(&mut self.frames, &self.motions, &self.stage, self.look, spot, beats, seed)?;
        guest.set_guest(now);
        guest.actor_mut().set_away(self.away);
        self.characters.push(guest);
        self.apply_habits();
        self.note(
            now,
            format!("꽃핀을 더 부름: 자리 ({:.0}, {:.0}), 모두 {}명", spot.0, spot.1, self.characters.len()),
        );
        Ok(())
    }

    /// 꽃핀 하나를 돌려보낸다. 손을 흔들고 사라진다. 공중에 있으면 내려와 선 뒤에 사라진다.
    fn send(&mut self, index: usize, now: Instant) {
        self.characters[index].actor_mut().leave(&self.motions);
        self.characters[index].wake(now);
        self.note(now, format!("꽃핀을 돌려보냄: {index}번"));
    }

    /// 더 부른 꽃핀을 모두 돌려보낸다.
    fn send_all(&mut self, now: Instant) {
        for index in self.guests().collect::<Vec<_>>() {
            self.send(index, now);
        }
    }

    /// 머무를 시간이 다 된 꽃핀을 돌려보낸다. 정해 둔 수보다 많이 나와 있으면 먼저 부른 꽃핀부터 돌려보낸다.
    fn tend_guests(&mut self, now: Instant) {
        if let Some(stay) = self.stay_time() {
            for index in self.guests().collect::<Vec<_>>() {
                if matches!(self.characters[index].stay(), Stay::Guest(since) if now >= since + stay) {
                    self.send(index, now);
                }
            }
        }
        while self.staying() > self.crowd_limit() {
            let Some(oldest) = self.guests().next() else { break };
            self.send(oldest, now);
        }
    }

    /// 꽃핀끼리의 앞뒤 차례를 맞춘다. 차례는 [`stack_order`]가 정한다.
    fn restack(&mut self) {
        let dragged = match self.pointer {
            Pointer::Dragging { hwnd } => self.find(hwnd),
            _ => None,
        };
        let guests: Vec<bool> =
            self.characters.iter().map(|character| matches!(character.stay(), Stay::Guest(_))).collect();
        let order = stack_order(&guests, dragged);
        if self.look.topmost {
            // 뒤에 설 꽃핀부터 차례로 맨 앞으로 올린다. 마지막에 올린 꽃핀이 맨 앞에 온다.
            for &index in &order {
                self.characters[index].keep_on_top();
            }
            if let Some(bubble) = &self.bubble {
                bubble.window().set_topmost(true);
            }
        } else {
            // 다른 창들 사이에서의 높이는 건드리지 않는다. 맨 앞에 설 꽃핀의 뒤로 나머지를 차례로 세운다.
            for pair in order.windows(2).rev() {
                let front = self.characters[pair[1]].window().hwnd();
                self.characters[pair[0]].window().place_behind(front);
            }
        }
        for character in &mut self.characters {
            character.take_raised();
        }
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
            // 포스트잇에 글을 적는 중이면 Ctrl+A를 먼저 처리하게 한다.
            if self.postits.handles(&message) {
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
                WindowEvent::Press { hwnd, .. } if self.is_bubble(hwnd) => {
                    // 말풍선은 누르면 바로 클릭으로 친다. 끌 것이 없으므로 누르고 있기를 그만둔다.
                    character_window::stop_pressing();
                    self.bubble_clicked();
                }
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
                        // 잡고 있던 꽃핀이 제 차례로 돌아간다.
                        self.restack();
                    }
                    // 짧게 눌렀다 뗐다. 클릭이다.
                    Pointer::Pressing { hwnd: pressed, .. } if pressed == hwnd => {
                        if let Some(i) = self.find(hwnd) {
                            self.clicked(i, now);
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
                // 말풍선을 오른쪽 클릭하면 딸린 일(방송 열기 같은 것)은 하지 않고 닫는다.
                WindowEvent::Menu { hwnd, .. } if self.is_bubble(hwnd) => self.bubble_dismissed(now),
                // 꽃핀을 오른쪽 클릭해도 트레이 아이콘과 같은 메뉴가 뜬다. 행동은 오른쪽 클릭한 꽃핀이 한다.
                WindowEvent::Menu { hwnd, x, y } => {
                    self.menu_target = Some(hwnd);
                    flow = Flow::Menu(x, y);
                }
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
                TrayEvent::Menu { x, y } => {
                    self.menu_target = None;
                    flow = Flow::Menu(x, y);
                }
                TrayEvent::Open => self.open_settings(),
                TrayEvent::TaskbarCreated => self.tray.add(),
                // 앱을 한 번 더 켰다. 새로 켠 쪽은 끝났고, 여기서는 꽃핀이 반짝여 이미 켜져 있다고 알린다.
                TrayEvent::Hello => {
                    self.dev.hellos += 1;
                    self.blink_all(now);
                }
                // 트레이 아이콘을 클릭했다. 꽃핀이 반짝여 어디 있는지 알려 준다.
                TrayEvent::Click => self.blink_all(now),
                TrayEvent::ToastClicked => self.toast_clicked(now),
                TrayEvent::Locked(locked) => {
                    self.locked = locked;
                    self.note(now, format!("화면 잠금: {locked}"));
                }
                TrayEvent::Asleep(asleep) => {
                    self.asleep = asleep;
                    self.note(now, format!("절전: {asleep}"));
                }
            }
        }
        for event in watch::take_events() {
            match event {
                WatchEvent::Foreground => {
                    // 작업표시줄을 누르면 작업표시줄이 "항상 위" 창들 가운데 맨 앞으로 나온다. 그 뒤에 있으면
                    // 걸터앉은 다리가 가려지므로, 맨 앞 창이 바뀔 때마다 꽃핀을 다시 맨 앞으로 올린다.
                    self.dev.foregrounds += 1;
                    if self.dev.options.report.is_some() {
                        let at = now.saturating_duration_since(self.started).as_secs_f64();
                        self.dev.notes.push(format!("{at:.2}초 맨 앞 창이 바뀌었다는 알림"));
                    }
                    if self.look.topmost {
                        self.restack();
                    }
                    // 창들의 앞뒤가 바뀌었을 것이다. 다음 걸음에서 바로 다시 읽는다.
                    self.scanned = None;
                    // 전체 화면 프로그램으로 넘어갔거나 거기서 나왔을 수 있다. 바로 다시 본다.
                    self.next_screen_check = now;
                }
            }
        }
        for event in settings_window::take_events() {
            if let Some(tab) = self.dialog_event(event, now) {
                flow = Flow::ConfirmReset(tab);
            }
        }

        // 3. 모니터가 바뀌었으면 무대를 다시 읽는다. 화면 밖에 남은 꽃핀은 주 모니터에 다시 나타난다.
        if stage_changed {
            self.stage = Stage::read().pretend_dpi(&self.dev.options.monitor_dpi);
            self.tell_postits_monitors();
            // 화면 밖에 남은 포스트잇도 가까운 화면 안으로 데려온다.
            self.postits.rescue();
            for i in 0..self.characters.len() {
                let (x, y) = self.characters[i].actor().position();
                if !self.stage.contains(x.round() as i32, y.round() as i32 - 1) {
                    let work = self.stage.primary().work;
                    let spot = ((work.x + work.w / 2) as f64, (work.y + work.h * 3 / 10) as f64);
                    let seed = self.next_seed();
                    let stay = self.characters[i].stay();
                    let mut character = Character::appear(
                        &mut self.frames,
                        &self.motions,
                        &self.stage,
                        self.look,
                        spot,
                        (now, self.started),
                        seed,
                    )?;
                    // 더 부른 꽃핀이었으면 머무를 시간은 그대로 이어진다.
                    if let Stay::Guest(since) = stay {
                        character.set_guest(since);
                    }
                    self.characters[i] = character;
                    self.apply_habits();
                }
                self.characters[i].wake(now);
            }
            // 새로 만든 창은 보이는 채로 생긴다. 숨겨 둔 중이었으면 다시 숨긴다.
            if self.out_of_sight {
                self.characters.iter_mut().for_each(|character| character.set_visible(false, now));
            }
        }

        // 전체 화면 프로그램이 떠 있는지 가끔 본다. 떠 있으면 꽃핀을 숨기고 멈춘다. 화면이 잠겼을 때도 그렇다.
        if self.watches_screen() && now >= self.next_screen_check {
            self.next_screen_check = now + SCREEN_CHECK;
            let screen = win::screen();
            if screen != self.screen {
                self.screen = screen;
                self.note(now, format!("화면을 쓰는 쪽이 바뀜: {screen:?}"));
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

        // 더 부른 꽃핀 가운데 머무를 시간이 다 된 것을 돌려보낸다.
        if self.quitting.is_none() {
            self.tend_guests(now);
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

        // 꽃핀을 숨기거나 다시 보인다. 스크립트가 시킨 것도 여기서 반영된다.
        self.apply_hiding(now);

        // 6. 치지직에서 온 답을 받고, 때가 됐으면 방송 상태를 다시 묻는다. 화면을 쓰지 않는 동안에는 쉰다.
        if !self.is_paused() {
            self.check_live(now);
        }

        // 7. 알릴 때가 된 메모를 알린다. 그리고 포스트잇에서 온 일을 처리하고 움직이는 포스트잇을 그린다.
        if !self.is_paused() {
            self.check_memos(now);
        }
        let wall = self.wall(now);
        for outcome in self.postits.frame(&mut self.memos, wall, now) {
            if let Some(next) = self.memo_outcome(outcome, now) {
                flow = next;
            }
        }

        // 8. 끝내기: 꽃핀들이 손을 흔들고 사라진 뒤에 끝낸다. 한 번 더 고르면 바로 끝낸다.
        if quit_asked && self.ask_quit(now) {
            return Ok(Flow::Quit);
        }

        // 9. 때가 된 꽃핀의 시간을 흘리고 화면에 넘긴다. 한 명도 남지 않으면 끝낸다.
        if !self.tick_characters(now)? {
            return Ok(Flow::Quit);
        }

        // 10. 기본 꽃핀이 내려앉은 자리를 기억해 둔다. 다시 켜면 그 위에 나타난다.
        let landed = self.home().map(Character::actor).filter(|home| home.mode() == Mode::Ground);
        if let Some(spot) = landed.map(|home| (home.home().round() as i32, home.position().1.round() as i32))
            && self.state.character != Some(spot)
        {
            self.state.character = Some(spot);
            self.store.state_changed(now);
        }

        // 11. 설정 창이 열려 있으면 "지금 쓰는 자원"을 1초마다 고친다.
        if let Some(window) = &self.window
            && now >= self.next_usage
        {
            window.set_usage(usage::memory().0 as f64 / usage::MIB, self.meter.cpu_percent());
            self.meter = Meter::start();
            self.next_usage = now + USAGE_EVERY;
        }

        // 12. 모아 둔 것을 저장한다.
        self.store.save(Some(now), &self.settings, &self.state, &self.memos);
        Ok(flow)
    }

    /// 때가 된 꽃핀의 시간을 흘리고 화면에 넘긴다. 다 사라진 꽃핀은 창을 닫는다.
    /// 한 명도 남지 않았으면 `false`를 돌려준다.
    ///
    /// 메뉴나 물음 창이 떠 있는 동안에도 이것만은 계속 불린다.
    pub fn tick_characters(&mut self, now: Instant) -> Result<bool, PackError> {
        // 숨겨 둔 동안에는 꽃핀의 시간이 흐르지 않는다.
        if self.out_of_sight {
            return Ok(true);
        }
        // 깨어난 김에 곧 올 박자까지 함께 밟는다.
        let horizon = now + character::LOOKAHEAD;
        // 다음 장을 그리기 바로 전에, 앞 장이 화면에 제대로 나와 있는지 본다. 첫 꽃핀이 멈춰 있을 때만 본다.
        if let (Some(check), Some(first)) = (&mut self.dev.screen_check, self.characters.first())
            && horizon >= first.due()
            && !first.moved()
        {
            check.sample(first.window(), now);
        }
        self.read_desktop(now);
        let started = Instant::now();
        let mut shown = false;
        let held = self.held;
        for character in &mut self.characters {
            // 메뉴에서 행동을 고르는 동안에는 그 꽃핀을 그대로 세워 둔다.
            if held == Some(character.window().hwnd()) {
                continue;
            }
            shown |= character.tick(&mut self.frames, &self.motions, &self.stage, &self.desktop, horizon)?;
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
        let before = self.characters.len();
        self.characters.retain(|character| !character.actor().is_gone());
        if self.characters.len() != before {
            self.note(
                now,
                format!("꽃핀 {}명이 사라져 {}명 남음", before - self.characters.len(), self.characters.len()),
            );
        }
        // 새로 나타났거나 걸터앉으려고 앞으로 나온 꽃핀이 있으면, 꽃핀끼리의 앞뒤 차례를 다시 맞춘다.
        if self.characters.iter_mut().fold(false, |any, character| character.take_raised() | any) {
            self.restack();
        }
        self.update_bubble(now);
        Ok(!self.characters.is_empty())
    }

    fn is_bubble(&self, hwnd: HWND) -> bool {
        self.bubble.as_ref().is_some_and(|bubble| bubble.window().hwnd() == hwnd)
    }

    /// 말풍선을 때맞춰 띄우고 닫는다. 떠 있으면 기본 꽃핀의 머리를 따라가게 한다.
    fn update_bubble(&mut self, now: Instant) {
        if self.notices.is_over(now) {
            self.notices.dismiss();
            self.close_bubble(now);
        }
        let Some(first) = self.home() else {
            self.bubble = None;
            return;
        };
        let (head, half_body, bounds, dpi) = first.head(&self.stage);
        // 꽃핀이 떠 있거나 잡혀 있는 동안에는 띄우지 않고 기다린다. 끝내는 중에는 새로 띄우지 않는다.
        let ready = first.actor().can_speak() && self.quitting.is_none() && !self.out_of_sight;
        if self.bubble.is_none()
            && let Some(notice) = self.notices.next(now, ready)
        {
            self.bubble = Some(Bubble::new(&notice.speech, dpi, self.look.topmost));
            if self.dev.options.report.is_some() {
                let at = now.saturating_duration_since(self.started).as_secs_f64();
                self.dev.notes.push(format!("{at:.2}초 말풍선 띄움: {} / {}", notice.speech.title, notice.speech.body));
            }
        }
        if let Some(bubble) = &mut self.bubble {
            bubble.follow(head, half_body, bounds);
        }
        // 처음 켰을 때의 안내를 다 띄웠으면(또는 띄울 까닭이 없어졌으면) 다음부터는 하지 않게 적어 둔다.
        if self.greeting && !self.notices.is_waiting(GREET_TAG) && !self.notices.is_waiting(GREET_LIVE_TAG) {
            self.greeting = false;
            self.state.greeted = true;
            self.store.state_changed(now);
        }
    }

    fn close_bubble(&mut self, now: Instant) {
        if let Some(bubble) = self.bubble.take()
            && self.dev.options.report.is_some()
        {
            let (at, rect) = (now.saturating_duration_since(self.started).as_secs_f64(), bubble.window().screen_rect());
            self.dev.notes.push(format!("{at:.2}초 말풍선 닫음: 자리 ({}, {}) {}x{}", rect.x, rect.y, rect.w, rect.h));
        }
    }

    /// 떠 있는 말풍선을 클릭했다. 닫고, 그 말풍선에 딸린 일을 한다. 다음 것은 다음 걸음에서 뜬다.
    fn bubble_clicked(&mut self) {
        let now = Instant::now();
        self.close_bubble(now);
        match self.notices.dismiss().map(|notice| notice.click) {
            Some(Click::Open(target)) => self.open(&target),
            Some(Click::Memos) => self.postits.show_all(),
            Some(Click::Close) | None => {}
        }
    }

    /// 떠 있는 말풍선을 오른쪽 클릭했다. 딸린 일(방송 열기 같은 것)은 하지 않고 닫는다.
    fn bubble_dismissed(&mut self, now: Instant) {
        self.close_bubble(now);
        self.notices.dismiss();
    }

    /// 꽃핀을 짧게 클릭했다.
    fn clicked(&mut self, index: usize, now: Instant) {
        // 말풍선은 기본 꽃핀의 것이다. 더 부른 꽃핀을 클릭했을 때는 그 꽃핀이 반응하기만 한다.
        let home = index == 0 && self.home().is_some();
        if home && self.bubble.is_some() {
            // 말풍선이 떠 있으면 클릭은 "봤다"는 뜻이다. 딸린 일(방송 열기 같은 것)은 하지 않고 닫는다.
            self.close_bubble(now);
            self.notices.dismiss();
            return;
        }
        self.characters[index].actor_mut().poke(&self.motions);
        self.characters[index].wake(now);
        if home {
            self.show_held_memo();
        }
    }

    /// 꽃핀이 들고 있는 메모를 말풍선으로 보여 준다. 여럿이면 클릭할 때마다 다음 것을 보여 준다.
    fn show_held_memo(&mut self) {
        let held: Vec<&Memo> = self.memos.held().collect();
        if held.is_empty() {
            return;
        }
        let turn = self.held_turn % held.len();
        let title = if held.len() == 1 { "메모".to_string() } else { format!("메모 {}/{}", turn + 1, held.len()) };
        let notice = Notice {
            kind: Kind::Memo,
            speech: Speech { title, body: held[turn].text.trim().to_string() },
            lasts: Some(HELD_LASTS),
            click: Click::Close,
            tag: Some(HELD_TAG),
        };
        self.held_turn = turn + 1;
        // 잇달아 클릭해도 줄에 쌓이지 않게, 앞서 세워 둔 것은 버린다.
        self.notices.cancel(HELD_TAG);
        self.notices.push(notice);
    }

    /// 지금 이 PC의 벽시계 시각. 확인용으로 시계를 정해 줬으면 그 시각에서부터 흐른다.
    fn wall(&self, now: Instant) -> When {
        When::from_seconds(match self.dev.options.clock {
            Some(start) => start + now.saturating_duration_since(self.started).as_secs() as i64,
            None => win::local_seconds(),
        })
    }

    /// 알릴 때가 된 메모를 알린다. 알릴 것이 남아 있는 동안에만, 1초마다 시계를 본다.
    ///
    /// 따로 타이머를 걸지 않고 시계를 직접 보므로, 시계를 고치거나 절전에서 돌아와도 따로 할 일이 없다.
    fn check_memos(&mut self, now: Instant) {
        if now < self.next_memo_check || !self.memos.has_pending() || self.quitting.is_some() {
            return;
        }
        self.next_memo_check = now + MEMO_CHECK;
        let due = self.memos.take_due(self.wall(now));
        if due.on_time.is_empty() && due.missed.is_empty() {
            return;
        }
        self.store.memos_changed(now);
        // 포스트잇에 적힌 알림 시각을 맞춘다. 한 번 알리는 것은 사라지고, 날마다 알리는 것은 그대로다.
        self.postits.refresh(&self.memos);
        for memo in &due.on_time {
            self.remind(memo, now);
        }
        if !due.missed.is_empty() {
            self.tell_missed(&due.missed, now);
        }
    }

    /// 메모 하나를 알린다. 그 메모의 포스트잇이 커지고 빛나며 앞으로 나오고, 나와 있는 꽃핀이 모두 놀란다.
    /// Windows 알림으로도 보여 준다. 포스트잇 아래의 단추로 확인하거나 조금 뒤로 미룬다.
    ///
    /// 전체 화면 프로그램을 쓰는 중이면 포스트잇은 그것이 끝난 뒤에 울린다. 그동안에는 Windows 알림만 간다.
    fn remind(&mut self, memo: &Memo, now: Instant) {
        self.note(now, format!("알림: {}", memo.title()));
        if self.is_quiet() {
            self.rings_waiting.push(memo.id);
        } else {
            self.surprise_all(now);
            self.postits.ring(&mut self.memos, memo.id);
        }
        if self.settings.is_on(Key::ReminderToast) {
            self.toast("꽃핀 알림", &memo.title(), false, now);
        }
    }

    /// 앱이나 PC가 꺼져 있던 동안 지나간 알림을 한 번에 알린다. 클릭하면 포스트잇이 모두 앞으로 나온다.
    fn tell_missed(&mut self, missed: &[Memo], now: Instant) {
        let titles = missed.iter().map(Memo::title).collect::<Vec<_>>().join(", ");
        self.note(now, format!("놓친 알림 {}개: {titles}", missed.len()));
        let title =
            if missed.len() == 1 { "놓친 알림".to_string() } else { format!("놓친 알림 {}개", missed.len()) };
        self.notices.push(Notice {
            kind: Kind::Reminder,
            speech: Speech { title, body: titles },
            lasts: None,
            click: Click::Memos,
            tag: None,
        });
    }

    /// 포스트잇이 알려 온 일을 한다. 메뉴나 물음 창을 띄워야 하면 그것을 돌려준다.
    fn memo_outcome(&mut self, outcome: Outcome, now: Instant) -> Option<Flow> {
        match outcome {
            Outcome::Changed => self.store.memos_changed(now),
            Outcome::Menu { id, x, y } => return Some(Flow::MemoMenu(id, x, y)),
            Outcome::AskDelete(id) => return Some(Flow::ConfirmDelete(id)),
            Outcome::Answered { id, snooze: true } => {
                if self.memos.snooze(id, self.wall(now)) {
                    self.store.memos_changed(now);
                    self.note(now, format!("알림을 {SNOOZE_MINUTES}분 뒤로 미룸: 메모 {id}"));
                    self.postits.refresh(&self.memos);
                }
            }
            Outcome::Answered { id, snooze: false } => self.note(now, format!("알림을 확인함: 메모 {id}")),
            Outcome::Create { x, y } => self.new_memo((x, y)),
        }
        None
    }

    /// 그 메모의 포스트잇 창. 물음 창을 그 위에 띄우는 데 쓴다.
    pub fn memo_hwnd(&self, id: u64) -> Option<HWND> {
        self.postits.hwnd(id)
    }

    /// 새 메모를 `near`(화면 좌표) 가까이에 포스트잇으로 붙인다. 붙으면 바로 글을 적을 수 있다.
    fn new_memo(&mut self, near: (i32, i32)) {
        if self.postits.create(&mut self.memos, near, true).is_none() {
            // 더 둘 수 없다. 까닭을 말풍선으로 알린다.
            self.notices.push(Notice {
                kind: Kind::Note,
                speech: Speech {
                    title: "메모가 가득 찼어요".to_string(),
                    body: format!("메모는 {COUNT_LIMIT}개까지 둘 수 있어요. 다 본 메모를 지워 주세요."),
                },
                lasts: Some(HELD_LASTS),
                click: Click::Close,
                tag: None,
            });
        }
    }

    /// 포스트잇을 오른쪽 클릭했을 때의 메뉴. 항목은 번호, 이름, 표시가 되어 있는지다.
    pub fn memo_menu(&self, id: u64) -> Vec<(u32, &'static str, bool)> {
        let held = self.memos.get(id).is_some_and(|memo| memo.held);
        vec![
            (1, "새 메모", false),
            // 맡긴 메모는 꽃핀을 클릭하면 말풍선으로 보여 준다.
            (2, "꽃핀에게 맡기기", held),
            (0, "", false),
            (5, "메모 목록...", false),
            (3, "모두 앞으로 가져오기", false),
            (4, "다시 줄 세우기", false),
        ]
    }

    /// 포스트잇의 메뉴에서 고른 것을 한다. `at`은 메뉴를 띄운 자리다.
    pub fn memo_picked(&mut self, id: u64, picked: u32, at: (i32, i32), now: Instant) {
        match picked {
            1 => self.new_memo(at),
            2 => {
                if let Some(memo) = self.memos.get_mut(id) {
                    memo.held = !memo.held;
                    self.store.memos_changed(now);
                }
            }
            3 => self.postits.show_all(),
            4 => self.postits.tidy(),
            5 => self.postits.open_list(at),
            _ => {}
        }
    }

    /// 물음 창의 답을 받아, "예"였으면 그 메모를 지운다. 포스트잇은 구겨져 꽃잎으로 흩어진다.
    pub fn delete_confirmed(&mut self, id: u64, yes: bool, now: Instant) {
        if yes {
            self.note(now, format!("메모를 지움: 메모 {id}"));
            self.postits.remove(&mut self.memos, id);
        }
    }

    /// 보이는 다른 창들을 때맞춰 다시 읽는다. 꽃핀이 올라서 있는 창은 걸음마다 다시 본다.
    fn read_desktop(&mut self, now: Instant) {
        if !self.look.topmost {
            // "항상 위"가 꺼져 있으면 창 위에 올라서지 않으므로 읽을 까닭이 없다.
            self.desktop.clear();
            self.scanned = None;
            return;
        }
        let actors = || self.characters.iter().map(Character::actor);
        let every = if actors().any(|actor| actor.is_airborne()) {
            SCAN_IN_AIR
        } else if actors().any(|actor| actor.support().is_some()) {
            SCAN_ON_PERCH
        } else {
            SCAN_ON_FLOOR
        };
        if self.scanned.is_none_or(|at| now >= at + every) {
            let started = Instant::now();
            self.desktop.scan();
            self.dev.scans.push(started.elapsed());
            self.scanned = Some(now);
        }
        for window in self.characters.iter().filter_map(|character| character.actor().support()) {
            self.desktop.track(window);
        }
    }

    /// 다음에 할 일이 있을 때까지 잔다. 창 메시지가 오면 그 전에 깬다.
    pub fn sleep(&self) {
        // 숨겨 둔 동안에는 꽃핀 때문에 깨어날 일이 없다.
        let mut wake = if self.out_of_sight { None } else { self.characters.iter().map(Character::due).min() };
        let mut sooner = |limit: Option<Instant>| {
            if let Some(limit) = limit {
                wake = Some(wake.map_or(limit, |wake| wake.min(limit)));
            }
        };
        if let Pointer::Pressing { since, .. } = self.pointer {
            sooner(Some(since + CLICK_HOLD));
        }
        if !self.away_forced && self.away_after().is_some() && !self.out_of_sight {
            sooner(Some(self.next_away_check));
        }
        if self.watches_screen() {
            sooner(Some(self.next_screen_check));
        }
        if let Hide::Until(at) = self.hide {
            sooner(Some(at));
        }
        let paused = self.is_paused();
        sooner(self.window.is_some().then_some(self.next_usage));
        sooner(self.store.due());
        sooner(self.notices.closes_at());
        sooner(self.live_due().filter(|_| !paused));
        sooner((self.memos.has_pending() && self.quitting.is_none() && !paused).then_some(self.next_memo_check));
        sooner(self.postits.due());
        sooner(self.guests_due());
        sooner(self.dev.deadline);
        // 자원을 재기 시작할 때를 놓치지 않는다. 숨겨 둔 동안에는 달리 깨어날 일이 없다.
        sooner((self.dev.usage.is_none() && self.dev.options.report.is_some()).then_some(self.started + dev::WARM_UP));
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
    pub fn picked(&mut self, item: Option<MenuItem>, now: Instant) -> Result<bool, PackError> {
        match item {
            Some(MenuItem::Quit) => return Ok(self.ask_quit(now)),
            Some(MenuItem::CallOne) => self.call_guest(now)?,
            Some(MenuItem::SendAll) => self.send_all(now),
            // 고르는 사이에 그 꽃핀이 돌아갔으면 아무 일도 하지 않는다.
            Some(MenuItem::SendThis) => {
                if let Some(index) = self.menu_guest() {
                    self.send(index, now);
                }
            }
            Some(MenuItem::Hide(index)) => {
                self.hide = match tray::HIDES[index].1 {
                    Some(seconds) => Hide::Until(now + Duration::from_secs(seconds)),
                    None => Hide::UntilAsked,
                };
                self.apply_hiding(now);
            }
            Some(MenuItem::Unhide) => {
                self.hide = Hide::Off;
                self.apply_hiding(now);
            }
            Some(MenuItem::Settings) => self.open_settings(),
            Some(MenuItem::Watch) => self.watch(now),
            Some(MenuItem::MemoNew) => self.new_memo(win::cursor_position()),
            Some(MenuItem::MemoShow) => self.postits.show_all(),
            Some(MenuItem::MemoTidy) => self.postits.tidy(),
            Some(MenuItem::MemoList) => self.postits.open_list(win::cursor_position()),
            Some(MenuItem::Link(index)) => self.open(LINKS[index].url),
            Some(MenuItem::Show(show)) => {
                let target = self.menu_character();
                if let Some(character) = self.characters.get_mut(target) {
                    character.show(show, &self.motions, &self.stage, now);
                }
            }
            None => {}
        }
        Ok(false)
    }

    /// 메뉴에서 고른 행동을 할 꽃핀. 메뉴를 띄운 꽃핀이고, 트레이에서 띄웠거나 그 꽃핀이 없어졌으면 첫 꽃핀이다.
    fn menu_character(&self) -> usize {
        self.menu_target.and_then(|hwnd| self.find(hwnd)).unwrap_or(0)
    }

    /// 메뉴가 떠 있는 동안, 메뉴에서 고른 행동을 할 꽃핀을 멈춰 세운다. `shows`는 [`App::menu_shows`]가 준 것이다.
    ///
    /// 계속 움직이게 두면 고르는 사이에 걷거나 다른 행동을 시작해서, 메뉴에 보이는 것과 실제로 할 수 있는
    /// 것이 달라진다. 고를 수 있는 행동이 하나도 없으면(떠 있거나 잡혀 있으면) 멈추지 않는다.
    pub fn hold_for_menu(&mut self, shows: &[(Show, bool)]) {
        if shows.iter().any(|&(_, possible)| possible) {
            self.held = self.characters.get(self.menu_character()).map(|character| character.window().hwnd());
        }
    }

    /// 멈춰 세워 둔 꽃핀을 다시 움직이게 한다.
    pub fn release_held(&mut self, now: Instant) {
        if let Some(hwnd) = self.held.take()
            && let Some(index) = self.find(hwnd)
        {
            self.characters[index].resume(now);
        }
    }

    /// 메뉴의 "행동" 아래에 넣을 행동과, 지금 고를 수 있는지. 숨겨 둔 동안에는 아무것도 고를 수 없다.
    pub fn menu_shows(&self) -> Vec<(Show, bool)> {
        let mut shows = self
            .characters
            .get(self.menu_character())
            .map(|character| character.showable(&self.stage))
            .unwrap_or_default();
        if self.out_of_sight {
            shows.iter_mut().for_each(|(_, possible)| *possible = false);
        }
        shows
    }

    /// 사용자가 꽃핀을 잠깐 숨겨 둔 중인가. 메뉴에 "다시 보이기"를 넣는다.
    pub fn is_tucked(&self) -> bool {
        self.hide != Hide::Off
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
        // 숨겨 둔 동안에는 인사할 꽃핀이 보이지 않는다. 바로 끝낸다.
        if self.quitting.is_some() || self.out_of_sight {
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
                // 잡은 꽃핀이 다른 꽃핀들보다 앞에 보인다.
                self.restack();
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
        // 창의 아이콘은 트레이 아이콘과 같은 얼굴이다. 큰 것은 정보 탭에도 보인다.
        let small_size = tray::icon_size();
        let Ok(small_face) = self.frames.portrait(first, small_size as usize) else { return };
        let icons = (tray::make_icon(&face, ABOUT_ICON), tray::make_icon(&small_face, small_size));
        self.window = SettingsWindow::open(&self.settings, self.state.settings_tab, center, icons, &version);
        self.next_usage = Instant::now();
        self.meter = Meter::start();
        self.show_live();
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
            DialogEvent::Tidy => {
                if let Some(window) = &self.window {
                    window.tidy(&self.settings);
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
        // 잡기, 끌기, 놓기, 클릭은 `target`으로 고른 꽃핀에게 한다. 고르지 않았으면 첫 꽃핀이다.
        let who = self.menu_character();
        match command {
            dev::Command::Grab => {
                self.dev.script_origin = self.characters[who].actor().position();
                let origin = self.dev.script_origin;
                if self.characters[who].actor_mut().grab(&self.motions, origin) {
                    self.pointer = Pointer::Dragging { hwnd: self.characters[who].window().hwnd() };
                    self.restack();
                }
            }
            dev::Command::Drag(dx, dy) => {
                let wanted = (self.dev.script_origin.0 + dx, self.dev.script_origin.1 + dy);
                let target = self.characters[who].drag_to_foot(wanted, &self.stage);
                self.characters[who].actor_mut().drag_to(target);
            }
            dev::Command::Release => {
                self.characters[who].actor_mut().release(&self.motions);
                self.pointer = Pointer::Idle;
                self.restack();
            }
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
            dev::Command::Click => self.clicked(who, now),
            dev::Command::Rub(distance) => self.characters[0].actor_mut().rub(&self.motions, distance),
            dev::Command::Do(whim) => self.characters[0].actor_mut().start(whim, &self.motions),
            dev::Command::Away(on) => {
                self.away_forced = true;
                self.set_away(on, now);
            }
            dev::Command::Fullscreen(on) => {
                self.screen_forced = true;
                self.screen = if on { Screen::Busy } else { Screen::Free };
            }
            dev::Command::Lock(on) => self.locked = on,
            dev::Command::Hello => self.blink_all(now),
            dev::Command::Hide(seconds) => {
                self.hide = Hide::Until(now + Duration::from_secs_f64(seconds));
            }
            dev::Command::Say(lasts, title, body) => self.notices.push(Notice {
                kind: Kind::Note,
                speech: Speech { title, body },
                lasts: (lasts > 0.0).then(|| Duration::from_secs_f64(lasts)),
                click: Click::Close,
                tag: None,
            }),
            dev::Command::BubbleClick => {
                if self.bubble.is_some() {
                    self.bubble_clicked();
                }
            }
            dev::Command::Live(reply) => {
                let reply = match reply {
                    dev::LiveReply::Open { opened, title } => {
                        Reply::Open(Live { title, category: "종합 게임".to_string(), viewers: 1234, opened })
                    }
                    dev::LiveReply::Close => Reply::Closed,
                    dev::LiveReply::Fail => Reply::Failed,
                    dev::LiveReply::Moved => Reply::Moved,
                    dev::LiveReply::Refused => Reply::Refused,
                };
                self.live_reply(reply, now);
            }
            dev::Command::ToastClick => self.toast_clicked(now),
            dev::Command::BubbleDismiss => {
                if self.bubble.is_some() {
                    self.bubble_dismissed(now);
                }
            }
            dev::Command::Memo(step) => {
                let mut shot = None;
                if step == dev::MemoStep::Snap {
                    self.dev.snaps += 1;
                    shot = self.dev.options.settings_shot.as_deref().map(|path| dev::numbered(path, self.dev.snaps));
                    self.dev.notes.extend(self.postits.describe(&self.memos));
                }
                self.postits.script(&step, &mut self.memos, shot.as_deref());
            }
            dev::Command::Menu(id) => {
                if self.picked(MenuItem::from_id(id), now)? {
                    flow = Flow::Quit;
                }
            }
            dev::Command::Target(index) => {
                self.menu_target = self.characters.get(index).map(|character| character.window().hwnd());
            }
            dev::Command::Crowd => {
                // 화면에서 앞에 있는 꽃핀부터 차례로 적는다. 창의 순서를 직접 읽은 것이다.
                let windows: Vec<HWND> = self.characters.iter().map(|character| character.window().hwnd()).collect();
                let front_first = dev::stacking(&windows);
                let names: Vec<String> = front_first
                    .iter()
                    .map(|&index| match self.characters[index].stay() {
                        Stay::Home => format!("{index}번(기본)"),
                        Stay::Guest(_) if self.characters[index].actor().is_leaving() => format!("{index}번(가는 중)"),
                        Stay::Guest(_) => format!("{index}번"),
                    })
                    .collect();
                let seen = self.characters.iter().filter(|character| character.window().is_visible()).count();
                self.note(
                    now,
                    format!("꽃핀 {}명(보이는 창 {seen}개), 앞에서부터 {}", self.characters.len(), names.join(" ")),
                );
                self.dev.snaps += 1;
                if let Some(path) = &self.dev.options.settings_shot {
                    let back_first: Vec<_> =
                        front_first.iter().rev().map(|&index| self.characters[index].window()).collect();
                    dev::crowd_shot(&back_first, &dev::numbered(path, self.dev.snaps));
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
                        dev::Dialog::Type(id, value) => window.type_text(id, &value.to_string()),
                        dev::Dialog::Press(id) => window.press(id),
                        dev::Dialog::Reset => flow = Flow::ConfirmReset(window.tab()),
                        dev::Dialog::Snap => {
                            self.dev.snaps += 1;
                            if let Some(path) = &self.dev.options.settings_dump {
                                window.dump(&dev::numbered(path, self.dev.snaps));
                            }
                            if let Some(path) = &self.dev.options.settings_shot {
                                window.shot(&dev::numbered(path, self.dev.snaps));
                            }
                        }
                    }
                }
            }
        }
        // 시킨 일이 바로 보이게 모두 깨운다.
        self.characters.iter_mut().for_each(|character| character.wake(now));
        Ok(flow)
    }

    /// 끝낸다. 미뤄 둔 저장을 하고, 보고서에 적을 줄을 돌려준다.
    pub fn finish(mut self) -> Vec<String> {
        // 끌 때 꽃핀이 있던 자리를 기억한다. 공중에 있었으면 마지막으로 내려앉은 자리를 둔다.
        if let Some(first) = self.home().map(Character::actor)
            && first.mode() == Mode::Ground
        {
            let (x, y) = first.position();
            self.state.character = Some((x.round() as i32, y.round() as i32));
        }
        self.store.state_changed(Instant::now());
        // 포스트잇에 적던 글이 있으면 메모에 옮겨 적고 저장한다.
        if !self.postits.commit(&mut self.memos).is_empty() {
            self.store.memos_changed(Instant::now());
        }
        self.store.save(None, &self.settings, &self.state, &self.memos);

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
        lines.push(format!("맨 앞 창이 바뀌었다는 알림 {}번", self.dev.foregrounds));
        if !self.dev.scans.is_empty() {
            let total: Duration = self.dev.scans.iter().sum();
            let longest = self.dev.scans.iter().max().copied().unwrap_or_default();
            lines.push(format!(
                "보이는 창 읽기 {}번, 평균 {:.0} / 최대 {:.0} µs, 마지막에 읽은 창 {}개",
                self.dev.scans.len(),
                total.as_secs_f64() * 1e6 / self.dev.scans.len() as f64,
                longest.as_secs_f64() * 1e6,
                self.desktop.panes().len()
            ));
        }
        lines.extend(self.dev.trace.lines());
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_home_character_stands_in_front_of_guests() {
        // 기본 꽃핀 하나와 더 부른 꽃핀 셋. 먼저 부른 꽃핀일수록 뒤에 서고, 기본 꽃핀이 맨 앞이다.
        let crowd = [false, true, true, true];
        assert_eq!(stack_order(&crowd, None), [1, 2, 3, 0]);
        // 잡고 있는 꽃핀은 누구든 맨 앞으로 나온다.
        assert_eq!(stack_order(&crowd, Some(2)), [1, 3, 0, 2]);
        assert_eq!(stack_order(&crowd, Some(0)), [1, 2, 3, 0]);
        // 없는 꽃핀을 잡고 있다고 해도 차례가 흐트러지지 않는다.
        assert_eq!(stack_order(&crowd, Some(9)), [1, 2, 3, 0]);
        assert_eq!(stack_order(&[false], None), [0]);
        assert!(stack_order(&[], None).is_empty());
    }

    #[test]
    fn everyone_appears_once_in_the_order() {
        // 확인용으로 처음부터 여러 명을 띄우면 모두 기본 꽃핀이다. 첫 꽃핀이 맨 앞에 선다.
        assert_eq!(stack_order(&[false, false, false], None), [2, 1, 0]);
        for dragged in [None, Some(0), Some(1), Some(4)] {
            let mut order = stack_order(&[false, true, false, true, true], dragged);
            assert_eq!(order.last().copied(), dragged.or(Some(0)));
            order.sort_unstable();
            assert_eq!(order, [0, 1, 2, 3, 4]);
        }
    }
}
