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
use std::sync::mpsc::{Receiver, TryRecvError};
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
use crate::character_window::{self, CharacterWindow, WindowEvent};
use crate::checker::{self, Checker};
use crate::clock::WakeTimer;
use crate::desktop::Desktop;
use crate::dev;
use crate::focus::{Focus, Phase, Plan, Turn};
use crate::frames::Frames;
use crate::geom::Rect;
use crate::links::{self, LINKS};
use crate::live::{self, Event, Live, Reply, Tracker, View};
use crate::memos::{COUNT_LIMIT, Memo, Memos, SNOOZE_MINUTES};
use crate::motions::{MotionId, Motions};
use crate::notices::{Click, Kind, Notice, Notices};
use crate::postit::{FanPick, FocusAct, FocusModel, Outcome, Postits};
use crate::settings::{CROWD_MAX, Key, Settings};
use crate::settings_window::{self, DialogEvent, SettingsWindow, TAB_NAMES};
use crate::stage::Stage;
use crate::store::{self, State, Store};
use crate::studio::{self, Studio, StudioAsk, StudioEvent, StudioPick};
use crate::together::{self, Cue, Member};
use crate::tray::{self, MenuItem, Tray, TrayEvent};
use crate::update;
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
///
/// 바닥에서는 드물게 읽는다. 맨 앞 창이 바뀌거나, 창을 옮기거나 줄이면 Windows가 알려 주고, 그때 바로
/// 다시 읽기 때문이다. 알림이 오지 않는 변화(창을 가득 채우기 같은 것)는 이 간격 안에 알게 된다.
const SCAN_IN_AIR: Duration = Duration::from_millis(100);
const SCAN_ON_PERCH: Duration = Duration::from_millis(250);
const SCAN_ON_FLOOR: Duration = Duration::from_millis(5000);
/// 커서 따라오기: 커서를 보는 간격, 머물렀다고 칠 시간, 그동안 움직여도 되는 거리(px).
const CURSOR_EVERY: Duration = Duration::from_millis(250);
const CURSOR_DWELL: Duration = Duration::from_millis(1500);
const CURSOR_SLOP: i32 = 24;
/// 커서가 꽃핀 발밑에서 키의 이만큼 배 높이 안에 있어야 "바닥 가까이"로 친다.
const CURSOR_BAND: f64 = 2.0;

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
/// 쉬는 시간 알림의 말풍선을 가리키는 이름.
const FOCUS_TAG: &str = "focus";
/// 새 버전이 나왔다는 안내의 이름.
const UPDATE_TAG: &str = "update";
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
const WINDOW_LIMIT: usize = CROWD_MAX as usize + 9;
/// 더 부른 꽃핀이 나타나는 자리. 모니터 작업 영역의 너비와 높이를 1로 볼 때, 양 끝에서 조금 떨어진
/// 아무 데나의 위쪽 공중이다.
const GUEST_ACROSS: (f64, f64) = (0.1, 0.9);
const GUEST_DOWN: (f64, f64) = (0.2, 0.35);
/// 여러 명을 한꺼번에 초대했을 때, 한 명이 나타나고 다음 한 명이 나타나기까지의 사이.
const CALL_GAP: Duration = Duration::from_millis(200);
/// 화면을 전체 화면 프로그램이 쓰고 있는지 확인하는 간격.
const SCREEN_CHECK: Duration = Duration::from_secs(5);
/// "다 같이"를 시켰을 때, 하던 것을 멈추고 서는 꽃핀들을 기다리는 가장 긴 시간. 그때까지 서지 못한 꽃핀은 빼고
/// 시작한다.
const GATHER_WAIT: Duration = Duration::from_secs(3);
/// 머리 위 "다 같이" 묶음 안의 단추들. 트레이 메뉴의 "다 같이"와 같은 차례다.
const TOGETHER_PICKS: [FanPick; 3] = [FanPick::Wave, FanPick::Stretch, FanPick::Yawn];

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
    /// 방송 촬영용 창을 오른쪽 클릭했다. 화면의 이 자리에 메뉴를 띄운다.
    StudioMenu(i32, i32),
}

/// 새 버전 확인에 딸린 것들.
struct UpdateWatch {
    /// 물어 놓고 답을 기다리는 끈. 묻는 중이 아니면 없다.
    waiting: Option<Receiver<Option<(u32, String)>>>,
    /// 저절로 다음에 물을 때.
    due: Instant,
    /// 지금 아는 것.
    status: update::Status,
}

/// 지금 시각(유닉스 초).
fn unix_now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs() as i64)
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
    /// `--watch-stack`이면 꽃핀끼리의 앞뒤가 잠깐이라도 거꾸로 되는지 센다.
    stack: Option<dev::StackWatch>,
    /// 스크립트가 정한 커서 자리. 있으면 진짜 커서 대신 쓴다.
    cursor: Option<(i32, i32)>,
    snaps: u32,
    deadline: Option<Instant>,
    /// 실행하면서 보고서에 적을 줄.
    notes: Vec<String>,
}

pub struct App {
    frames: Frames,
    motions: Motions,
    /// 꽃핀들이 지내는 무대. 보통은 모니터들이고, 방송 촬영용 창이 열려 있으면 그 창 안쪽이다.
    stage: Stage,
    /// 모니터들. 방송 촬영용 창이 열려 있어도 진짜 모니터들이다. 창, 말풍선, 단추를 띄울 범위로 쓴다.
    screens: Stage,
    settings: Settings,
    state: State,
    store: Store,
    look: Look,
    characters: Vec<Character>,
    pointer: Pointer,
    /// 누르지 않은 채 꽃핀 위에서 마지막으로 움직인 곳. 문지른 거리를 재는 데 쓴다.
    hover: Option<(HWND, (i32, i32), Instant)>,
    /// 줄줄이 오기로 한 꽃핀들.
    calls: Calls,
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
    update: UpdateWatch,
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
    /// 커서 따라오기: 다음에 커서를 볼 때와, 커서가 머물기 시작한 자리·시각·그 자리로 꽃핀을 불렀는지.
    next_cursor_check: Instant,
    dwell: Option<((i32, i32), Instant, bool)>,
    /// 화면이 잠겨 있는가, PC가 절전에 들어갔는가.
    locked: bool,
    asleep: bool,
    /// 화면이 꺼져 있다. 잠그지 않은 채 모니터만 꺼졌을 때도 그렇다.
    dark: bool,
    /// 지금 꽃핀을 숨겨 두었는가.
    out_of_sight: bool,
    /// 같이 하기: 때가 되면 꽃핀에게 시킬 행동들과, 다음에 저절로 파도를 탈 때.
    cues: Vec<Cue>,
    next_wave: Option<Instant>,
    /// 같이 하기에서 누가 따라 할지, 언제 파도를 탈지 고르는 난수.
    rng: Rng,
    /// 돌고 있는 쉬는 시간 알림.
    focus: Option<Focus>,
    /// 열려 있는 방송 촬영용 창.
    studio: Option<Studio>,
    /// "다 같이"를 시켜서 꽃핀들이 서기를 기다리는 중이면 있다.
    gathering: Option<Gathering>,
    /// 방송 촬영용 창이 열려 있는 동안 세워 둔 바탕화면 꽃핀들과 세운 시각. 창을 닫으면 그 자리에서 이어 간다.
    parked: Option<(Vec<Character>, Instant)>,
    /// 방해하면 안 되는 동안 알릴 때가 된 메모들. 그때가 지나면 포스트잇을 울린다.
    rings_waiting: Vec<u64>,
    dev: Dev,
}

/// "다 같이"를 시켜서, 꽃핀들이 하던 것을 멈추고 서기를 기다리는 중이다.
struct Gathering {
    /// [`tray::TOGETHER`]의 몇 번째인지.
    index: usize,
    /// 기다리는 꽃핀들([`Character::id`]).
    who: Vec<isize>,
    /// 이때까지 서지 못한 꽃핀은 빼고 시작한다.
    until: Instant,
    started: Instant,
}

/// 줄줄이 오기로 한 꽃핀들. 여러 명을 한꺼번에 초대하면 한 명씩 [`CALL_GAP`]만큼 사이를 두고 나타난다.
#[derive(Debug, Default, PartialEq)]
struct Calls {
    /// 아직 오지 않은 꽃핀의 수.
    waiting: usize,
    /// 이 꽃핀이 있는 모니터에 온다(머리 위 단추로 불렀을 때). 없거나 그 꽃핀이 없어졌으면 기본 꽃핀이 있는 모니터다.
    near: Option<HWND>,
    /// 다음 꽃핀이 올 때. 올 꽃핀이 없으면 없다.
    next: Option<Instant>,
}

impl Calls {
    /// `count`명을 더 부른다. 오고 있는 꽃핀이 없었으면 첫 꽃핀은 바로 온다. 기다리는 꽃핀은 `most`명을
    /// 넘지 않는다.
    fn add(&mut self, count: usize, most: usize, now: Instant) {
        self.waiting = (self.waiting + count).min(most);
        if self.waiting > 0 && self.next.is_none() {
            self.next = Some(now);
        }
    }

    /// 한 명이 왔다. 올 꽃핀이 남았으면 다음 때를 잡는다.
    fn came(&mut self, now: Instant) {
        self.waiting = self.waiting.saturating_sub(1);
        self.next = (self.waiting > 0).then(|| now + CALL_GAP);
    }

    /// 지금은 올 수 없다. `until`에 다시 본다.
    fn put_off(&mut self, until: Instant) {
        self.next = (self.waiting > 0).then_some(until);
    }

    /// 오기로 한 꽃핀들이 오지 않게 한다.
    fn clear(&mut self) {
        *self = Calls::default();
    }
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
            offscreen: false,
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
        // 지난번에 알아낸 것이 지금 쓰는 것보다 새 버전이면 알고 시작한다. 켜고 조금 뒤에, 그리고 마지막으로
        // 물은 지 하루가 지났을 때 묻는다.
        let first = update::wait(state.update_checked, unix_now()).max(update::FIRST);
        let update = UpdateWatch {
            waiting: None,
            due: started + Duration::from_secs(first),
            status: update::Status::remembered(state.update_latest.as_deref(), update::Version::current()),
        };

        let seed = options
            .seed
            .unwrap_or_else(|| SystemTime::now().duration_since(UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64));
        // 포스트잇은 놓인 모니터의 화면 배율을 따른다. 모니터를 알 수 없을 때는 주 모니터의 것을 쓴다.
        let postit_dpi = options.dpi.unwrap_or(stage.primary().dpi);
        let mut app = App {
            frames,
            motions,
            screens: stage.clone(),
            stage,
            settings,
            state,
            store,
            look,
            characters: Vec::new(),
            pointer: Pointer::Idle,
            hover: None,
            calls: Calls::default(),
            away: false,
            away_forced: false,
            next_away_check: started,
            quitting: None,
            tray,
            face,
            icon_size,
            live,
            update,
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
            next_cursor_check: started,
            dwell: None,
            locked: false,
            asleep: false,
            dark: false,
            out_of_sight: false,
            cues: Vec::new(),
            next_wave: None,
            rng: Rng::new(seed ^ 0x5EED),
            focus: None,
            studio: None,
            parked: None,
            gathering: None,
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
                stack: options.watch_stack.then(dev::StackWatch::default),
                cursor: None,
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
        // 방송 촬영용 창을 열어 둔 채 껐으면 다시 연다.
        app.postits.set_list_top(app.state.list_top);
        if app.state.studio_open {
            app.open_studio(false, started)?;
        }
        Ok(app)
    }

    /// 포스트잇에게 모니터마다의 범위와 화면 배율을 알려 준다. 포스트잇은 자기가 놓인 모니터의 배율로 그린다.
    fn tell_postits_monitors(&mut self) {
        let forced = self.dev.options.dpi;
        let monitors: Vec<_> =
            self.screens.monitors().iter().map(|monitor| (monitor.area, forced.unwrap_or(monitor.dpi))).collect();
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
        lines.push(if !self.update_enabled() {
            "새 버전 확인: 꺼짐".to_string()
        } else if self.update_online() {
            format!("새 버전 확인: 하루에 한 번 {}에 물음", update::HOST)
        } else {
            "새 버전 확인: 켜짐. 확인용 실행이라 인터넷은 쓰지 않음".to_string()
        });
        let pending = self.memos.list().iter().filter(|memo| memo.next.is_some()).count();
        lines.push(format!("메모 {}개, 알릴 것 {pending}개", self.memos.list().len()));
        lines.push(match self.tray.icon_rect() {
            Some(rect) => format!("트레이 아이콘 등록됨: 자리 ({}, {}) {}x{}", rect.x, rect.y, rect.w, rect.h),
            None => "트레이 아이콘이 등록되지 않음".to_string(),
        });
        self.dev.notes.splice(0..0, lines);
    }

    /// 꽃핀들의 버릇. 확인용으로 가만히 세워 두면 돌아다니지도 가끔 하는 행동을 하지도 않는다.
    fn habits(&self) -> Habits {
        if self.dev.options.still { Habits { roam: false, whim_rate: 0.0 } } else { self.settings.habits() }
    }

    /// 설정의 버릇을 모든 꽃핀에게 알린다. 저절로 파도를 탈 때도 빈도에 맞춰 다시 잡는다.
    fn apply_habits(&mut self) {
        let habits = self.habits();
        self.characters.iter_mut().for_each(|character| character.actor_mut().set_habits(habits));
        self.next_wave = together::next_wave(habits.whim_rate, &mut self.rng, Instant::now());
    }

    /// 같이 하기를 따져 볼 꽃핀들. 꽃핀 번호 차례다. `eager`면 메뉴나 단추로 시킨 것이라, 잡혀 있거나 돌아가는
    /// 꽃핀만 빼고 모두 할 수 있는 것으로 친다([`App::can_join`]). 아니면 쉬거나 걷는 꽃핀만이고, "다 같이" 할
    /// 차례를 기다리며 서 있는 꽃핀은 뺀다.
    fn members_for(&self, eager: bool) -> Vec<Member> {
        let dragged = match self.pointer {
            Pointer::Dragging { hwnd } => Some(hwnd),
            _ => None,
        };
        let member = |character: &Character| {
            let actor = character.actor();
            let hwnd = Some(character.window().hwnd());
            Member {
                who: character.id(),
                x: actor.position().0,
                monitor: character.monitor_key(&self.stage),
                free: if eager {
                    self.can_join(character)
                } else {
                    actor.mode() == Mode::Ground
                        && !actor.is_leaving()
                        && !actor.is_waiting()
                        && hwnd != self.held
                        && hwnd != dragged
                },
                scale: character.scale().factor(),
            }
        };
        self.characters.iter().map(member).collect()
    }

    /// 저절로 하는 같이 하기(따라 하기, 파도)를 따져 볼 꽃핀들.
    fn members(&self) -> Vec<Member> {
        self.members_for(false)
    }

    /// 메뉴나 단추로 시킨 "다 같이"를 할 수 있는가. 잡혀 있거나, 머리 위 단추를 띄워 멈춰 세웠거나, 돌아가는
    /// 중이면 못 한다. 다른 행동을 하던 꽃핀도 멈추고 한다.
    fn can_join(&self, character: &Character) -> bool {
        let actor = character.actor();
        let hwnd = Some(character.window().hwnd());
        let dragged = match self.pointer {
            Pointer::Dragging { hwnd } => Some(hwnd),
            _ => None,
        };
        !actor.is_leaving()
            && !matches!(actor.mode(), Mode::Drag | Mode::Vanish | Mode::Gone)
            && hwnd != self.held
            && hwnd != dragged
    }

    /// 때가 된 같이 하기를 시킨다. 그때 할 수 없는 꽃핀은 넘어간다. 저절로 파도를 탈 때가 됐으면 탄다.
    fn play_cues(&mut self, now: Instant) {
        if self.out_of_sight || self.quitting.is_some() {
            self.drop_cues();
            return;
        }
        self.tend_gathering(now);
        if self.next_wave.is_some_and(|at| now >= at) {
            self.next_wave = together::next_wave(self.habits().whim_rate, &mut self.rng, now);
            let members = self.members();
            if let Some(monitor) = together::crowded(&members) {
                let wave = together::wave(&members, Some(monitor), now);
                self.note(now, format!("저절로 파도 점프: {}명", wave.len()));
                self.cues.extend(wave);
            }
        }
        if !self.cues.iter().any(|cue| cue.at <= now) {
            return;
        }
        let members = self.members();
        let (due, later): (Vec<Cue>, Vec<Cue>) = self.cues.drain(..).partition(|cue| cue.at <= now);
        self.cues = later;
        for cue in due {
            let Some(index) = members.iter().position(|member| member.who == cue.who) else { continue };
            let member = members[index];
            if cue.eager {
                // 메뉴나 단추로 시킨 것이다. 하던 것을 멈추고 한다.
                if self.can_join(&self.characters[index]) {
                    self.characters[index].join(cue.show, &self.motions, now);
                } else {
                    // 차례가 왔을 때 할 수 없게 됐다(머리 위 단추로 멈춰 세움 등). 기다리던 것을 그만둔다.
                    self.characters[index].actor_mut().stop_waiting();
                }
            } else if member.free {
                self.characters[index].show(cue.show, &self.motions, &self.stage, now);
            }
        }
    }

    /// 혼자 가끔 하는 행동을 시작한 꽃핀이 있으면, 옆 꽃핀이 따라 하게 한다.
    fn follow_whims(&mut self, now: Instant) {
        let started: Vec<(usize, Show)> = (self.characters.iter_mut().enumerate())
            .filter_map(|(index, character)| {
                Some((index, together::followable(character.actor_mut().take_started()?)?))
            })
            .collect();
        if started.is_empty() || self.out_of_sight {
            return;
        }
        let members = self.members();
        let rate = self.habits().whim_rate;
        for (leader, show) in started {
            if let Some(cue) = together::follow(&members, leader, show, rate, &mut self.rng, now) {
                self.note(now, format!("따라 하기: {leader}번의 {}", show.label()));
                self.cues.push(cue);
            }
        }
    }

    /// 커서 따라오기를 할 때인가. 설정에서 켰고, 꽃핀이 보이고, 방해하면 안 되는 때가 아닐 때다.
    fn watches_cursor(&self) -> bool {
        self.settings.is_on(Key::FollowCursor) && !self.out_of_sight && !self.is_paused() && self.quitting.is_none()
    }

    /// 커서가 바닥 가까이에 머물면, 가로로 가장 가까운 꽃핀이 그 아래로 걸어온다.
    ///
    /// 꽃핀이 선 곳(바닥이나 창 윗변)에서 키의 두 배 높이 안에 커서가 1.5초 머물러야 한다. 한 번 부르면 커서가
    /// 다시 움직였다 머물 때까지 부르지 않는다. 쉬거나 걷는 꽃핀만 오고, 자거나 다른 행동을 하는 꽃핀은 오지 않는다.
    /// 올 꽃핀이 없으면 커서가 머무는 동안 계속 살핀다. 하던 행동을 마친 꽃핀이 그때 온다.
    fn watch_cursor(&mut self, now: Instant) {
        if !self.watches_cursor() {
            self.dwell = None;
            return;
        }
        if now < self.next_cursor_check {
            return;
        }
        self.next_cursor_check = now + CURSOR_EVERY;
        let cursor = self.dev.cursor.unwrap_or_else(win::cursor_position);
        // 방송 촬영용 창이 열려 있으면 커서가 그 창 안쪽에 있을 때만 본다. 진짜 커서면 그 자리에서 창이 다른 창에
        // 가려져 있지 않아야 한다.
        if let Some(studio) = &self.studio
            && !studio.covers(cursor, self.dev.cursor.is_none())
        {
            self.dwell = None;
            return;
        }
        let still =
            self.dwell.is_some_and(|(at, _, _)| (at.0 - cursor.0).abs().max((at.1 - cursor.1).abs()) <= CURSOR_SLOP);
        if !still {
            self.dwell = Some((cursor, now, false));
            return;
        }
        let Some((at, since, false)) = self.dwell else { return };
        if now.saturating_duration_since(since) < CURSOR_DWELL {
            return;
        }
        let monitor = self.stage.nearest(cursor.0, cursor.1);
        let monitor = (monitor.area.x, monitor.area.y);
        let members = self.members();
        let (x, y) = (cursor.0 as f64, cursor.1 as f64);
        let near = |index: &usize| {
            let character = &self.characters[*index];
            let foot = character.actor().position().1;
            let member = &members[*index];
            member.free && member.monitor == monitor && y <= foot && y >= foot - CURSOR_BAND * character.height()
        };
        let candidates = (0..self.characters.len()).filter(near);
        let nearest = candidates.min_by(|&a, &b| (members[a].x - x).abs().total_cmp(&(members[b].x - x).abs()));
        if let Some(index) = nearest
            && self.characters[index].actor_mut().come_to(x)
        {
            self.characters[index].wake(now);
            self.note(now, format!("커서 따라오기: {index}번이 x {} 쪽으로", cursor.0));
            // 한 번 불렀으면 커서가 움직였다 다시 머물 때까지 부르지 않는다.
            self.dwell = Some((at, since, true));
        }
    }

    /// 새로 만들거나 설정을 바꿀 꽃핀의 모습. 방송 촬영용 창 안의 꽃핀은 화면에 내보내지 않는 버퍼에 그린다.
    fn character_look(&self) -> Look {
        if self.studio.is_some() { Look { topmost: false, offscreen: true, ..self.look } } else { self.look }
    }

    /// 꽃핀의 머리 꼭대기와 몸의 반폭, 그 위에 띄우는 것(단추, 말풍선)이 벗어나면 안 되는 범위, 화면 배율.
    /// 방송 촬영용 창 안의 꽃핀이면 범위는 창 안쪽이 아니라 그 창이 놓인 모니터다.
    fn head_of(&self, character: &Character) -> ((i32, i32), i32, Rect, u32) {
        let (head, half_body, bounds, dpi) = character.head(&self.stage);
        let bounds = if self.studio.is_some() { self.screens.nearest(head.0, head.1).work } else { bounds };
        (head, half_body, bounds, dpi)
    }

    /// 꽃핀들이 움직이지 않는 때인가. 숨겨 두었거나, 방송 촬영용 창을 최소화해 두었으면 그렇다.
    fn frozen(&self) -> bool {
        self.out_of_sight || self.studio.as_ref().is_some_and(Studio::is_minimized)
    }

    /// 꽃핀들의 무대를 옮기기 전에, 지금 꽃핀들에게 걸려 있던 것(머리 위 단추, 누르기, 줄줄이 올 꽃핀, 함께 하기)을
    /// 거둔다.
    fn leave_stage(&mut self, now: Instant) {
        self.close_fan(now);
        if !matches!(self.pointer, Pointer::Idle) {
            self.pointer = Pointer::Idle;
            character_window::stop_pressing();
        }
        (self.held, self.menu_target, self.hover, self.dwell) = (None, None, None, None);
        self.calls.clear();
        self.drop_cues();
    }

    /// 방송 촬영용 창을 연다. 열려 있으면 앞으로 가져온다. `activate`가 아니면 쓰던 창의 포커스를 빼앗지 않는다.
    ///
    /// 창이 열려 있는 동안은 꽃핀들의 무대가 창 안으로 옮겨 간다. 바탕화면의 꽃핀들은 그 자리에 멈춰 숨고, 창
    /// 안에는 처음 꽃핀 한 명이 위쪽 공중에 나타나 떨어진다.
    fn open_studio(&mut self, activate: bool, now: Instant) -> Result<(), PackError> {
        if let Some(studio) = &self.studio {
            if activate {
                studio.bring_to_front();
            }
            return Ok(());
        }
        let cursor = win::cursor_position();
        let work = self.screens.nearest(cursor.0, cursor.1).work;
        let center = (work.x + work.w / 2, work.y + work.h / 2);
        let place = self.state.studio_place.map(|(x, y, w, h)| Rect::new(x, y, w, h));
        // 기억해 둔 자리가 지금의 모니터들 밖이면 쓰지 않는다. 제목 줄의 가운데가 어느 모니터 안에 있어야 창을 잡아
        // 옮길 수 있다(그 모니터를 떼어 냈으면 위, 아래, 옆 어느 쪽으로든 밖일 수 있다).
        let place = place.filter(|rect| self.screens.contains(rect.x + rect.w / 2, rect.y + 8));
        let color = self.state.studio_color.unwrap_or(studio::COLORS[0].1);
        let first = self.motions.clip(self.motions.idle2).first_frame;
        let small_size = tray::icon_size();
        let (Ok(face), Ok(small_face)) =
            (self.frames.portrait(first, ABOUT_ICON as usize), self.frames.portrait(first, small_size as usize))
        else {
            return Ok(());
        };
        let icons = (tray::make_icon(&face, ABOUT_ICON), tray::make_icon(&small_face, small_size));
        let studio = Studio::open((place, center), (color, activate), icons);
        self.note(now, studio.describe());
        // 바탕화면 꽃핀들은 그 자리에 멈춰 숨는다.
        self.leave_stage(now);
        let mut parked = std::mem::take(&mut self.characters);
        parked.iter_mut().for_each(|character| character.set_visible(false, now));
        self.note(now, format!("바탕화면 꽃핀 {}명을 세워 둠", parked.len()));
        self.parked = Some((parked, now));
        self.stage = studio.stage().clone();
        self.studio = Some(studio);
        // 창 안에는 처음 꽃핀 한 명이 위쪽 공중에 나타나 떨어진다.
        let work = self.stage.primary().work;
        let spot = ((work.x + work.w / 2) as f64, (work.y + work.h / 3) as f64);
        let (seed, look) = (self.next_seed(), self.character_look());
        let mut first =
            Character::appear(&mut self.frames, &self.motions, &self.stage, look, spot, (now, self.started), seed)?;
        first.actor_mut().set_away(self.away);
        self.characters.push(first);
        self.apply_habits();
        if !self.state.studio_open {
            self.state.studio_open = true;
            self.store.state_changed(now);
        }
        Ok(())
    }

    /// 방송 촬영용 창을 닫는다. 다시 켤 때는 열지 않는다.
    ///
    /// 창 안의 꽃핀들은 창과 함께 사라지고, 세워 두었던 바탕화면 꽃핀들이 그 자리에서 다시 움직인다. 세워 둔
    /// 동안은 더 부른 꽃핀의 머무를 시간에 세지 않는다.
    fn close_studio(&mut self, now: Instant) -> Result<(), PackError> {
        if self.studio.is_some() {
            self.leave_stage(now);
            self.characters.clear();
            self.studio = None;
            self.stage = self.screens.clone();
            if let Some((parked, since)) = self.parked.take() {
                let gone = now.saturating_duration_since(since);
                self.characters = parked;
                let (look, hidden, away) = (self.look, self.out_of_sight, self.away);
                for character in &mut self.characters {
                    if let Stay::Guest(at) = character.stay() {
                        character.set_guest(at + gone);
                    }
                    character.set_look(look, now);
                    character.set_visible(!hidden, now);
                    character.actor_mut().set_away(away);
                    character.wake(now);
                }
            }
            self.apply_habits();
            // 세워 둔 동안 모니터가 바뀌었으면 화면 밖에 남은 꽃핀을 데려온다.
            self.rescue_characters(now)?;
            self.scanned = None;
            self.restack();
            self.note(now, format!("방송 촬영용 창을 닫음: 바탕화면 꽃핀 {}명이 이어 감", self.characters.len()));
        }
        if self.state.studio_open {
            self.state.studio_open = false;
            self.store.state_changed(now);
        }
        Ok(())
    }

    /// 화면(무대) 밖에 남은 꽃핀을 무대 안 위쪽 공중에 다시 나타나게 한다. 더 부른 꽃핀이었으면 머무를 시간은
    /// 그대로 이어진다.
    fn rescue_characters(&mut self, now: Instant) -> Result<(), PackError> {
        for i in 0..self.characters.len() {
            let (x, y) = self.characters[i].actor().position();
            if !self.stage.contains(x.round() as i32, y.round() as i32 - 1) {
                let work = self.stage.primary().work;
                let spot = ((work.x + work.w / 2) as f64, (work.y + work.h * 3 / 10) as f64);
                let seed = self.next_seed();
                let stay = self.characters[i].stay();
                let look = self.character_look();
                let mut character = Character::appear(
                    &mut self.frames,
                    &self.motions,
                    &self.stage,
                    look,
                    spot,
                    (now, self.started),
                    seed,
                )?;
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
        Ok(())
    }

    /// 방송 촬영용 창에서 일어난 일을 처리한다.
    ///
    /// 누르기, 끌기, 문지르기, 꽃핀 위의 오른쪽 클릭은 그 자리의 꽃핀에게 일어난 일로 바꿔 돌려준다. 바탕화면
    /// 꽃핀과 똑같이 다룬다. 빈 곳을 오른쪽 클릭했으면 메뉴를 띄우라고 돌려준다.
    fn studio_events(&mut self, now: Instant) -> Result<(Vec<WindowEvent>, Option<Flow>), PackError> {
        let (mut events, mut flow) = (Vec::new(), None);
        for event in studio::take_events() {
            if self.studio.is_none() {
                continue;
            }
            // 지금 누르고 있거나 잡고 있는 꽃핀.
            let held = match self.pointer {
                Pointer::Pressing { hwnd, .. } | Pointer::Dragging { hwnd } => Some(hwnd),
                Pointer::Idle => None,
            };
            match event {
                StudioEvent::Press(x, y) => match self.studio_hit((x, y)) {
                    Some(hwnd) => events.push(WindowEvent::Press { hwnd, x, y }),
                    None => {
                        self.close_fan(now);
                        character_window::stop_pressing();
                    }
                },
                StudioEvent::Move(x, y) => events.extend(held.map(|hwnd| WindowEvent::Move { hwnd, x, y })),
                StudioEvent::Lift => events.extend(held.map(|hwnd| WindowEvent::Lift { hwnd })),
                StudioEvent::Hover(x, y) => match self.studio_hit((x, y)) {
                    Some(hwnd) => events.push(WindowEvent::Hover { hwnd, x, y }),
                    None => self.hover = None,
                },
                StudioEvent::Menu(x, y) if let Some(hwnd) = self.studio_hit((x, y)) => {
                    events.push(WindowEvent::Menu { hwnd, x, y });
                }
                other => {
                    let Some(studio) = &mut self.studio else { continue };
                    match studio.handle(other) {
                        Some(StudioAsk::Menu(x, y)) => {
                            self.close_fan(now);
                            flow = Some(Flow::StudioMenu(x, y));
                        }
                        Some(StudioAsk::Close) => self.close_studio(now)?,
                        Some(StudioAsk::Moved) => self.studio_moved(now),
                        None => {}
                    }
                }
            }
        }
        Ok((events, flow))
    }

    /// 방송 촬영용 창 안의 그 자리(화면 좌표)에 몸이 있는 꽃핀. 앞에 선 꽃핀부터 본다. 숨겨 둔 동안에는 없다.
    fn studio_hit(&self, at: (i32, i32)) -> Option<HWND> {
        if self.frozen() {
            return None;
        }
        let order = self.stack_order();
        let front_first = order.iter().rev().map(|&index| self.characters[index].window());
        front_first.into_iter().find(|window| studio::touches(window, at)).map(CharacterWindow::hwnd)
    }

    /// 방송 촬영용 창의 자리나 크기가 바뀌었다. 꽃핀들의 무대를 다시 읽고, 창의 자리를 기억해 둔다.
    ///
    /// 크기가 바뀌었으면 꽃핀들이 하던 일을 멈추고, 창 너비에 대한 비율대로 가로 자리를 옮겨 새 바닥에 바로 선다.
    /// 옮기기만 했으면 꽃핀들도 같은 만큼 옮겨 가서 창 안의 자리가 그대로이고, 하던 일을 이어 간다.
    fn studio_moved(&mut self, now: Instant) {
        let Some(studio) = &self.studio else { return };
        let old = self.stage.primary().work;
        self.stage = studio.stage().clone();
        let new = self.stage.primary().work;
        if (old.w, old.h) != (new.w, new.h) {
            self.close_fan(now);
            for character in &mut self.characters {
                let ratio = ((character.actor().position().0 - old.x as f64) / old.w.max(1) as f64).clamp(0.0, 1.0);
                character.settle_at(&self.motions, &self.stage, new.x as f64 + ratio * new.w as f64, now);
            }
            self.note(
                now,
                format!(
                    "방송 촬영용 창의 크기가 바뀜: {}x{} → {}x{}, 꽃핀들을 새 바닥에 세움",
                    old.w, old.h, new.w, new.h
                ),
            );
        } else if (old.x, old.y) != (new.x, new.y) {
            let moved = ((new.x - old.x) as f64, (new.y - old.y) as f64);
            for character in &mut self.characters {
                character.actor_mut().shift(moved);
                character.wake(now);
            }
        }
        self.remember_studio(now);
    }

    /// 방송 촬영용 창에 꽃핀들을 앞뒤 차례대로 그린다. `changed`가 아니면 통째로 다시 그려야 할 때만 그린다.
    /// 숨겨 둔 동안에는 바탕색만 칠한다.
    fn paint_studio(&mut self, changed: bool) {
        if self.studio.is_none() {
            return;
        }
        let order = if self.out_of_sight { Vec::new() } else { self.stack_order() };
        let Some(studio) = &mut self.studio else { return };
        let windows: Vec<&CharacterWindow> = order.iter().map(|&index| self.characters[index].window()).collect();
        studio.compose(&windows, changed);
    }

    /// 방송 촬영용 창의 자리와 크기, 바탕색을 기억해 둔다.
    fn remember_studio(&mut self, now: Instant) {
        let Some(studio) = &self.studio else { return };
        let place = studio.place().map(|rect| (rect.x, rect.y, rect.w, rect.h)).or(self.state.studio_place);
        let color = Some(studio.color());
        if (place, color) != (self.state.studio_place, self.state.studio_color) {
            (self.state.studio_place, self.state.studio_color) = (place, color);
            self.store.state_changed(now);
        }
    }

    /// 방송 촬영용 창의 메뉴를 띄울 창과, 메뉴가 보여 줄 것.
    pub fn studio_menu(&self) -> Option<(HWND, studio::MenuState)> {
        let studio = self.studio.as_ref()?;
        let state = studio::MenuState {
            color: studio.color(),
            can_call: self.can_call(),
            room: self.room(),
            can_send: self.can_send(),
            hidden: self.out_of_sight,
        };
        Some((studio.hwnd(), state))
    }

    /// 방송 촬영용 창의 메뉴에서 고른 것을 한다. `custom`은 색 고르기 창에서 고른 색이다.
    pub fn studio_picked(
        &mut self,
        picked: Option<StudioPick>,
        custom: Option<u32>,
        now: Instant,
    ) -> Result<(), PackError> {
        let color = match picked {
            Some(StudioPick::Color(index)) => Some(studio::COLORS[index].1),
            Some(StudioPick::Custom) => custom,
            Some(StudioPick::Close) => return self.close_studio(now),
            Some(StudioPick::Call) => return self.call_guest(now),
            Some(StudioPick::CallMany(index)) => {
                self.call_many(tray::CALLS[index.min(tray::CALLS.len() - 1)], None, now);
                None
            }
            Some(StudioPick::CallFill) => {
                self.call_many(self.room(), None, now);
                None
            }
            Some(StudioPick::SendAll) => {
                self.send_all(now);
                None
            }
            Some(StudioPick::Together(index)) => {
                self.play_together(index, now);
                None
            }
            None => None,
        };
        if let (Some(color), Some(studio)) = (color, &mut self.studio) {
            studio.set_color(color);
            self.note(now, format!("방송 촬영용 창의 바탕색 #{color:06X}"));
            self.remember_studio(now);
            self.paint_studio(false);
        }
        Ok(())
    }

    /// 확인용 스크립트의 `studio` 명령. 누르기, 끌기, 문지르기는 창에서 그 일이 일어난 것으로 친다.
    fn studio_script(&mut self, word: &str, numbers: &[f64], now: Instant) -> Result<(), PackError> {
        let number = |i: usize| numbers.get(i).copied().unwrap_or(0.0);
        if word == "open" {
            return self.open_studio(false, now);
        }
        if word == "close" {
            return self.close_studio(now);
        }
        if self.studio.is_none() {
            return Ok(());
        }
        // 누를 꽃핀: `studio click 번호`처럼 번호를 주면 그 꽃핀, 없으면 첫 꽃핀. 발바닥에서 몸 높이의 절반쯤 위를
        // 누른다.
        let who = match word {
            "click" | "menu" => number(0) as usize,
            "rub" => number(1) as usize,
            "drag" => number(2) as usize,
            _ => 0,
        }
        .min(self.characters.len().saturating_sub(1));
        let (body, head) = match self.characters.get(who) {
            Some(character) => {
                let (x, y) = character.actor().position();
                let tall = character.height();
                let x = x.round() as i32;
                ((x, (y - tall * 0.45).round() as i32), (x, (y - tall * 0.85).round() as i32))
            }
            None => ((0, 0), (0, 0)),
        };
        match word {
            "click" => {
                studio::inject(StudioEvent::Press(body.0, body.1));
                studio::inject(StudioEvent::Lift);
            }
            "drag" => {
                studio::inject(StudioEvent::Press(body.0, body.1));
                studio::inject(StudioEvent::Move(body.0 + number(0) as i32, body.1 + number(1) as i32));
            }
            "lift" => studio::inject(StudioEvent::Lift),
            "rub" => {
                let steps = (number(0) / 20.0).ceil().max(1.0) as i32;
                for i in 0..=steps {
                    studio::inject(StudioEvent::Hover(head.0 + if i % 2 == 0 { -10 } else { 10 }, head.1));
                }
            }
            // `studio menu 번호`: 그 꽃핀을 오른쪽 클릭한다(머리 위 단추가 뜬다).
            "menu" => studio::inject(StudioEvent::Menu(body.0, body.1)),
            // `studio call|many 번호|fill|send|together 번호`: 빈 곳의 메뉴에서 그것을 고른 것으로 친다. 진짜 메뉴는
            // 띄우지 않는다(띄우면 고를 때까지 기다린다).
            "call" | "many" | "fill" | "send" | "together" => {
                let index = number(0) as usize;
                let pick = match word {
                    "call" => StudioPick::Call,
                    "many" => StudioPick::CallMany(index),
                    "fill" => StudioPick::CallFill,
                    "send" => StudioPick::SendAll,
                    _ => StudioPick::Together(index.min(tray::TOGETHER.len() - 1)),
                };
                let state = self.studio_menu().map(|(_, state)| state);
                self.note(now, format!("방송 촬영용 창의 메뉴: {state:?}에서 {pick:?}"));
                self.studio_picked(Some(pick), None, now)?;
            }
            "color" => {
                let index = (number(0) as usize).min(studio::COLORS.len() - 1);
                if let Some(studio) = &mut self.studio {
                    studio.set_color(studio::COLORS[index].1);
                }
                self.paint_studio(false);
            }
            "snap" => {
                let order = if self.out_of_sight { Vec::new() } else { self.stack_order() };
                let windows: Vec<&CharacterWindow> =
                    order.iter().map(|&index| self.characters[index].window()).collect();
                let Some(studio) = &mut self.studio else { return Ok(()) };
                let (width, pixels) = studio.snapshot(&windows);
                let modes: Vec<String> =
                    self.characters.iter().map(|character| format!("{:?}", character.actor().mode())).collect();
                let line = format!("{}, 꽃핀 {}명 [{}]", studio.describe(), self.characters.len(), modes.join(", "));
                self.dev.snaps += 1;
                if let Some(path) = &self.dev.options.settings_shot {
                    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("shot");
                    let file = path.with_file_name(format!("{stem}_studio_{}.bmp", self.dev.snaps));
                    dev::write_bmp(&file, width, &pixels);
                }
                self.note(now, line);
            }
            _ => {}
        }
        Ok(())
    }

    /// 쉬는 시간 알림을 시작한다. 떠 있던 쉬는 시간 말풍선은 치운다.
    fn start_focus(&mut self, focus: Focus, now: Instant) {
        if self.notices.cancel(FOCUS_TAG) {
            self.close_bubble(now);
        }
        self.note(now, format!("쉬는 시간 알림 시작: {}", focus.label()));
        self.focus = Some(focus);
    }

    /// 쉬는 시간 알림을 그만둔다.
    fn stop_focus(&mut self, now: Instant) {
        self.focus = None;
        if self.notices.cancel(FOCUS_TAG) {
            self.close_bubble(now);
        }
        self.note(now, "쉬는 시간 알림 그만둠".to_string());
    }

    /// 트레이 메뉴에 보여 줄 쉬는 시간 알림의 상태. 돌고 있지 않으면 없다.
    pub fn focus_menu(&self) -> Option<String> {
        self.focus.as_ref().map(Focus::label)
    }

    /// 쉬는 시간 알림 판에서 고른 시간. 고른 적이 없으면 25분 집중, 5분 쉬기다.
    fn focus_plan(&self) -> Plan {
        self.state.focus_plan.unwrap_or_default()
    }

    /// 쉬는 시간 알림 판이 보여 줄 것.
    fn focus_model(&self) -> FocusModel {
        let plan = self.focus_plan();
        let running = self.focus.as_ref();
        FocusModel {
            phase: running.map(|focus| focus.phase() == Phase::Focus),
            paused: running.is_some_and(Focus::is_paused),
            away: self.away,
            left: running.map_or(0, Focus::left_secs),
            total: running.map_or(0, |focus| focus.total().as_secs()),
            focus: plan.focus,
            rest: plan.rest,
            auto: plan.auto,
            top: plan.top,
        }
    }

    /// 쉬는 시간 알림 판을 그 꽃핀 옆에 띄운다.
    fn open_focus_panel(&mut self, index: usize) {
        let Some(character) = self.characters.get(index) else { return };
        let (head, half_body, _, dpi) = character.head(&self.stage);
        let model = self.focus_model();
        self.postits.open_focus((head, half_body), dpi, model);
        self.note(Instant::now(), format!("쉬는 시간 알림 판을 띄움: {index}번 옆"));
    }

    /// 쉬는 시간 알림 판에서 누른 것을 한다.
    fn focus_acted(&mut self, act: FocusAct, now: Instant) {
        let mut plan = self.focus_plan();
        let away = self.away;
        match act {
            FocusAct::Pick { rest: false, minutes } => plan.focus = minutes,
            FocusAct::Pick { rest: true, minutes } => plan.rest = minutes,
            FocusAct::Step { rest, steps } => plan.step(rest, steps),
            FocusAct::Auto => {
                plan.auto = !plan.auto;
                // 도는 타이머에도 바로 쓴다. 이번 쉬는 시간이 끝날 때부터 달라진다.
                if let Some(focus) = &mut self.focus {
                    focus.set_auto(plan.auto);
                }
            }
            FocusAct::Start => self.start_focus(Focus::start(plan, now), now),
            FocusAct::Pause(pause) => {
                if let Some(focus) = &mut self.focus {
                    focus.pause(pause, now, away);
                    self.note(now, format!("쉬는 시간 알림: {}", if pause { "잠깐 멈춤" } else { "다시" }));
                }
            }
            FocusAct::Stop => self.stop_focus(now),
            FocusAct::Pin => plan.top = !plan.top,
            FocusAct::Skip => {
                if let Some(focus) = &mut self.focus {
                    focus.skip(now);
                    let label = focus.label();
                    if self.notices.cancel(FOCUS_TAG) {
                        self.close_bubble(now);
                    }
                    self.note(now, format!("쉬는 시간 알림 넘김: {label}"));
                }
            }
        }
        let plan = plan.fit();
        if plan != self.focus_plan() {
            self.state.focus_plan = Some(plan);
            self.store.state_changed(now);
            self.note(
                now,
                format!(
                    "쉬는 시간 알림 판: {}분 집중, {}분 쉬기, 저절로 {}, 항상 위 {}",
                    plan.focus, plan.rest, plan.auto, plan.top
                ),
            );
        }
        let model = self.focus_model();
        self.postits.set_focus(model);
    }

    /// 쉬는 시간 알림의 시간을 흘리고, 때가 되면 알린다.
    ///
    /// 집중 시간이 끝나면 말풍선으로 쉬자고 하고, 나와 있는 꽃핀들이 다 같이 기지개를 켠다. 쉬는 시간이 끝나면
    /// 말풍선으로 알리고 타이머를 끝낸다. 그 말풍선을 누르면 같은 길이로 다시 시작한다. 전체 화면 프로그램을 쓰거나
    /// 꽃핀을 숨겨 둔 동안에는 말풍선이 그 뒤에 뜨므로, 메모 알림처럼 Windows 알림으로도 보낸다(설정을 따른다).
    fn tend_focus(&mut self, now: Instant) {
        let away = self.away;
        let Some(focus) = &mut self.focus else { return };
        let turn = focus.advance(now, away);
        let (rest, minutes) = (focus.rest().as_secs().div_ceil(60), focus.focus().as_secs().div_ceil(60));
        let (title, body, click, lasts) = match turn {
            None => return,
            Some(Turn::BreakTime) => {
                let members = self.members();
                let stretch = together::together(&members, Show::Stretch, &mut self.rng, now);
                self.cues.extend(stretch);
                ("쉬는 시간이에요!".to_string(), format!("{rest}분 쉬어요. 기지개 한번 켜요."), Click::Close, None)
            }
            Some(Turn::BackToWork) => {
                self.focus = None;
                let minutes = self.focus_plan().focus;
                ("쉬는 시간 끝!".to_string(), format!("누르면 다시 {minutes}분 집중을 시작해요."), Click::Focus, None)
            }
            // 저절로 이어 가면 누르지 않아도 말풍선이 닫힌다.
            Some(Turn::Again) => {
                ("쉬는 시간 끝!".to_string(), format!("다시 {minutes}분 집중해요."), Click::Close, Some(HELD_LASTS))
            }
        };
        self.note(now, format!("쉬는 시간 알림: {title} {body}"));
        if self.notices.cancel(FOCUS_TAG) {
            self.close_bubble(now);
        }
        let speech = Speech { title: title.clone(), body: body.clone() };
        self.notices.push(Notice { kind: Kind::Reminder, speech, lasts, click, tag: Some(FOCUS_TAG) });
        if (self.is_quiet() || self.out_of_sight) && self.settings.is_on(Key::ReminderToast) {
            self.toast(&title, &body, false, now);
        }
    }

    /// 메뉴의 "다 같이"에서 고른 것을 나와 있는 꽃핀들이 함께 한다.
    ///
    /// 모두 하던 행동을 멈추고 그 자리에 선다. 자던 꽃핀은 깨어 일어나고, 걸터앉은 꽃핀은 일어서고, 떨어지는 꽃핀은
    /// 내려선다. 모두 선 순간에 함께 시작한다([`App::tend_gathering`]). 잡혀 있거나 돌아가는 꽃핀은 빠진다.
    fn play_together(&mut self, index: usize, now: Instant) {
        // 기다리던 것이 있으면 그만두고 새로 시킨 것을 기다린다.
        self.end_gathering();
        let mut who = Vec::new();
        for index in 0..self.characters.len() {
            if self.can_join(&self.characters[index]) && self.characters[index].halt(&self.motions, now) {
                who.push(self.characters[index].id());
            }
        }
        self.note(now, format!("다 같이 {}: {}명이 멈추고 서기를 기다림", tray::TOGETHER[index], who.len()));
        self.gathering = Some(Gathering { index, who, until: now + GATHER_WAIT, started: now });
        self.tend_gathering(now);
    }

    /// "다 같이"를 기다리던 꽃핀들이 모두 섰거나 오래 기다렸으면 함께 시작한다. 그때까지 서지 못한 꽃핀은 빼고,
    /// 하던 대로 지내게 한다.
    fn tend_gathering(&mut self, now: Instant) {
        let Some(gathering) = &self.gathering else { return };
        let ready = |character: &Character| character.actor().is_waiting();
        let still: Vec<&Character> = gathering
            .who
            .iter()
            .filter_map(|id| self.characters.iter().find(|character| character.id() == *id))
            .filter(|character| self.can_join(character))
            .collect();
        if !still.iter().all(|character| ready(character)) && now < gathering.until {
            return;
        }
        let ready_ids: Vec<isize> =
            still.iter().filter(|character| ready(character)).map(|character| character.id()).collect();
        let (index, waited) = (gathering.index, now.saturating_duration_since(gathering.started));
        let who = self.gathering.take().map(|gathering| gathering.who).unwrap_or_default();
        // 서지 못한 꽃핀은 빼고, 하던 대로 지내게 한다. 선 꽃핀은 차례가 올 때까지 계속 서 있는다.
        for character in &mut self.characters {
            if who.contains(&character.id()) && !ready_ids.contains(&character.id()) {
                character.actor_mut().stop_waiting();
            }
        }
        let mut members = self.members_for(true);
        for member in &mut members {
            member.free &= ready_ids.contains(&member.who);
        }
        let mut cues = match index {
            0 => together::wave(&members, None, now),
            1 => together::together(&members, Show::Stretch, &mut self.rng, now),
            _ => together::together(&members, Show::Yawn, &mut self.rng, now),
        };
        cues.iter_mut().for_each(|cue| cue.eager = true);
        let late = format!("{:.1}초 기다림", waited.as_secs_f64());
        self.note(now, format!("다 같이 {}: {}명이 함께 시작({late})", tray::TOGETHER[index], cues.len()));
        self.cues.extend(cues);
        self.play_cues(now);
    }

    /// 같이 하기 신호를 모두 버린다. 다 같이 할 차례를 기다리던 꽃핀들은 하던 대로 지낸다.
    fn drop_cues(&mut self) {
        self.cues.clear();
        self.gathering = None;
        self.characters.iter_mut().for_each(|character| character.actor_mut().stop_waiting());
    }

    /// "다 같이"를 기다리던 것을 그만둔다. 기다리던 꽃핀들은 하던 대로 지낸다.
    fn end_gathering(&mut self) {
        let Some(gathering) = self.gathering.take() else { return };
        for character in &mut self.characters {
            if gathering.who.contains(&character.id()) {
                character.actor_mut().stop_waiting();
            }
        }
    }

    /// 바뀐 설정을 화면의 꽃핀에게 바로 반영하고, 저장을 예약한다.
    fn apply_settings(&mut self, now: Instant) {
        self.look.size_percent = self.settings.size_percent();
        self.look.topmost = self.settings.is_on(Key::AlwaysOnTop);
        let look = self.character_look();
        self.characters.iter_mut().for_each(|character| character.set_look(look, now));
        // 말풍선은 방송 촬영용 창이 열려 있어도 화면에 뜨는 창이다. 설정의 "항상 위"를 그대로 따른다.
        if let Some(bubble) = &self.bubble {
            bubble.window().set_topmost(self.look.topmost);
        }
        self.apply_habits();
        if !self.live_enabled() {
            // 방송 상태 확인을 껐다. 알던 것을 잊고, 방송 안내와 아직 띄우지 않은 인터넷 안내를 치운다.
            if *self.live.tracker.view() != View::Unknown {
                self.live.tracker = Tracker::new(self.live.tracker.announced().map(str::to_string));
                self.live.checked = None;
            }
            // 인터넷을 쓴다는 안내는 인터넷을 쓰는 일이 하나도 남지 않았을 때 치운다.
            let nothing_online = !self.update_enabled();
            if self.notices.cancel(LIVE_TAG) | (nothing_online && self.notices.cancel(GREET_LIVE_TAG)) {
                self.close_bubble(now);
            }
        }
        self.show_live();
        self.show_update();
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
        if let Some(studio) = &self.studio {
            studio.bring_to_front();
        } else if !self.look.topmost {
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
        // 인터넷을 쓰는 일을 알린다. 방송 상태 확인과 새 버전 확인이다.
        let mut uses = Vec::new();
        if self.live_enabled() {
            uses.push(format!(
                "방송이 켜지면 알려 주려고 {}분마다 치지직에 물어봐요. 치지직의 공식 API는 아니에요.",
                self.settings.number(Key::LiveCheckInterval)
            ));
        }
        if self.update_enabled() {
            uses.push("새 버전이 나왔는지는 하루에 한 번 GitHub에 물어봐요.".to_string());
        }
        if !uses.is_empty() {
            uses.push("설정에서 끌 수 있어요.".to_string());
            self.notices.push(note("인터넷을 이렇게 써요", uses.join(" "), None, GREET_LIVE_TAG));
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

    /// 새 버전 확인이 켜져 있는가.
    fn update_enabled(&self) -> bool {
        self.settings.is_on(Key::UpdateCheck)
    }

    /// GitHub에 진짜로 물어도 되는가. 정한 시간만 도는 확인용 실행은 시켰을 때만 묻는다.
    fn update_online(&self) -> bool {
        self.quitting.is_none() && (self.dev.options.run_for.is_none() || self.dev.options.update_check)
    }

    /// 새 버전이 나왔는지 저절로 물을 시각. 묻지 않을 때는 `None`이다.
    fn update_due(&self) -> Option<Instant> {
        let asks = self.update_enabled() && self.update_online() && self.update.waiting.is_none();
        // 처음 켰을 때는 인터넷을 쓴다는 안내를 띄운 뒤부터 묻는다.
        (asks && !self.notices.is_waiting(GREET_LIVE_TAG)).then_some(self.update.due)
    }

    /// GitHub에서 온 답을 받고, 때가 됐으면 새 버전이 나왔는지 묻는다.
    fn check_update(&mut self, now: Instant) {
        if let Some(waiting) = &self.update.waiting {
            match waiting.try_recv() {
                Ok(answer) => {
                    self.update.waiting = None;
                    let reply = answer.map_or(update::Reply::Failed, |(status, body)| update::judge(status, &body));
                    self.update_reply(reply, now);
                }
                Err(TryRecvError::Empty) => {}
                // 묻는 스레드가 답 없이 끝났다.
                Err(TryRecvError::Disconnected) => {
                    self.update.waiting = None;
                    self.update_reply(update::Reply::Failed, now);
                }
            }
        }
        if self.update_due().is_some_and(|due| now >= due) {
            self.ask_update(now);
        }
    }

    /// 새 버전이 나왔는지 GitHub에 묻는다. 답은 나중에 온다. 설정 창의 단추를 눌렀을 때도 부른다.
    fn ask_update(&mut self, now: Instant) {
        if self.update.waiting.is_some() {
            return;
        }
        // 답이 오지 않아도 한동안은 다시 묻지 않는다. 답이 오면 그때 다음 때를 다시 잡는다.
        self.update.due = now + Duration::from_secs(update::RETRY);
        if !self.update_online() {
            self.note(now, "새 버전을 묻지 않음: 확인용 실행이라 인터넷을 쓰지 않음".to_string());
            return;
        }
        self.update.waiting = checker::ask_once(update::HOST, update::PATH, self.tray.hwnd());
        if self.update.waiting.is_some() {
            self.note(now, format!("새 버전이 나왔는지 물음: {}", update::HOST));
        }
        self.show_update();
    }

    /// 새 버전에 대한 답 하나를 받아, 알릴 일이 있으면 알린다.
    fn update_reply(&mut self, reply: update::Reply, now: Instant) {
        self.update.status = self.update.status.after(reply, update::Version::current());
        self.note(now, format!("새 버전에 대한 답: {reply:?}"));
        if reply != update::Reply::Failed {
            // 답을 받았다. 하루 뒤에 다시 묻는다. 앱을 다시 켜도 하루에 한 번만 묻게 받은 때를 적어 둔다.
            self.update.due = now + Duration::from_secs(update::EVERY as u64);
            self.state.update_checked = Some(unix_now());
            self.state.update_latest = match reply {
                update::Reply::Latest(version) => Some(version.to_string()),
                _ => None,
            };
            self.store.state_changed(now);
        }
        // 새 버전은 버전마다 한 번만 말풍선으로 알린다. 그 뒤에는 메뉴와 설정 창에서 볼 수 있다.
        if let Some(version) = self.update.status.newer().map(|version| version.to_string())
            && self.state.update_announced.as_deref() != Some(version.as_str())
        {
            self.notices.push(Notice {
                kind: Kind::Note,
                speech: Speech {
                    title: "새 버전이 나왔어요".to_string(),
                    body: format!("{version} 버전을 받을 수 있어요. 누르면 받는 곳이 열려요."),
                },
                lasts: None,
                click: Click::Open(update::RELEASES_URL.to_string()),
                tag: Some(UPDATE_TAG),
            });
            self.state.update_announced = Some(version);
            self.store.state_changed(now);
        }
        self.show_update();
    }

    /// 설정 창의 새 버전 줄과 단추를 지금 아는 것에 맞춘다.
    fn show_update(&self) {
        if let Some(window) = &self.window {
            let status = if self.update.waiting.is_some() { update::Status::Asking } else { self.update.status };
            let line = update::status_line(status, self.update_enabled());
            window.set_update_status(&line, self.update.status.newer().is_some());
        }
    }

    /// 더 새 버전이 나와 있으면 그 버전. 메뉴 맨 위에 받는 길을 넣는다.
    pub fn update_menu(&self) -> Option<String> {
        self.update.status.newer().map(|version| version.to_string())
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
        self.settings.number(Key::MaxCharacters).clamp(1, CROWD_MAX) as usize
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

    /// 화면을 쓰지 않는 때인가. 잠겨 있거나, 절전 중이거나, 화면이 꺼졌거나, 화면 보호기가 떠 있으면 그렇다.
    /// 이때는 방송 상태도 묻지 않고 알림 시각도 보지 않는다. 돌아오면 그때 한꺼번에 본다.
    fn is_paused(&self) -> bool {
        self.locked || self.asleep || self.dark || self.screen == Screen::Absent
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
        // 방송 촬영용 창 안의 꽃핀은 전체 화면 프로그램이 떠 있어도 숨지 않는다. 게임을 전체 화면으로 하며
        // 방송할 수 있다.
        let busy = if self.studio.is_some() { self.is_paused() } else { self.is_quiet() };
        let hidden = self.hide != Hide::Off || busy;
        if hidden != self.out_of_sight {
            self.out_of_sight = hidden;
            if hidden {
                let why = if self.is_paused() {
                    "화면을 쓰지 않음"
                } else if self.hide != Hide::Off {
                    "잠깐 숨기기"
                } else {
                    "전체 화면"
                };
                self.note(now, format!("꽃핀을 숨김: {why}"));
                self.close_fan(now);
                // 한동안 그릴 일이 없다. 풀어서 들고 있던 장들을 버려 메모리를 돌려준다.
                self.frames.forget();
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
            if let Some(studio) = &mut self.studio {
                studio.refresh();
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

    /// 먼저 온 꽃핀을 돌려보내지 않고 더 부를 수 있는 꽃핀의 수. 오기로 하고 아직 오지 않은 꽃핀의 자리는 뺀다.
    pub fn room(&self) -> usize {
        if !self.can_call() {
            return 0;
        }
        self.crowd_limit().saturating_sub(self.staying() + self.calls.waiting)
    }

    /// 꽃핀을 여러 명 초대한다. 한꺼번에 나타나지 않고 사이를 두고 줄줄이 온다. `near` 꽃핀(없으면 기본 꽃핀)이
    /// 있는 모니터에 온다.
    ///
    /// 자리보다 많이 부르면 한 명씩 올 때마다 가장 먼저 부른 꽃핀이 자리를 내준다. 더 부른 꽃핀이 앉을 수 있는
    /// 자리보다 많이 불러 봐야 방금 온 꽃핀이 바로 돌아가게 되므로, 그 수까지만 부른다.
    fn call_many(&mut self, count: usize, near: Option<HWND>, now: Instant) {
        if count == 0 || !self.can_call() {
            return;
        }
        let seats = self.crowd_limit().saturating_sub(1);
        self.calls.add(count.min(seats), seats, now);
        self.calls.near = near;
        self.note(now, format!("꽃핀 {count}명을 초대함: 줄줄이 올 꽃핀 {}명", self.calls.waiting));
    }

    /// 줄줄이 오기로 한 꽃핀 가운데 올 때가 된 한 명을 나타나게 한다.
    fn tend_calls(&mut self, now: Instant) -> Result<(), PackError> {
        if self.calls.next.is_none_or(|at| now < at) {
            return Ok(());
        }
        if self.can_call() {
            self.calls.came(now);
            let near = self.calls.near.and_then(|hwnd| self.find(hwnd)).unwrap_or(0);
            self.call_guest_near(near, now)?;
        } else if !self.out_of_sight && self.crowd_limit() > 1 {
            // 돌아가는 중인 꽃핀이 많아 창을 더 만들 수 없다. 그 꽃핀들이 사라질 때까지 조금 기다린다.
            self.calls.put_off(now + CALL_GAP);
        } else {
            // 꽃핀을 숨겼거나 한 명만 두기로 했다. 남은 꽃핀은 오지 않는다.
            self.calls.clear();
        }
        Ok(())
    }

    /// 꽃핀을 하나 더 부른다. 단추를 띄운 꽃핀이 있는 모니터의 위쪽 공중에 나타나 떨어진다.
    /// 트레이에서 불렀으면 기본 꽃핀이 있는 모니터다.
    fn call_guest(&mut self, now: Instant) -> Result<(), PackError> {
        self.call_guest_near(self.menu_character(), now)
    }

    /// 꽃핀을 하나 더 나타나게 한다. `near`번째 꽃핀이 있는 모니터의 위쪽 공중에 나타나 떨어진다.
    fn call_guest_near(&mut self, near: usize, now: Instant) -> Result<(), PackError> {
        if !self.can_call() {
            return Ok(());
        }
        // 자리가 없으면 가장 먼저 부른 꽃핀부터 손을 흔들며 돌아간다.
        while self.staying() >= self.crowd_limit() {
            let Some(oldest) = self.guests().next() else { break };
            self.send(oldest, now);
        }
        let work = match self.characters.get(near).map(|character| character.actor().position()) {
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
        let look = self.character_look();
        let mut guest = Character::appear(&mut self.frames, &self.motions, &self.stage, look, spot, beats, seed)?;
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
        // 머리 위에 단추들을 띄워 둔 꽃핀이면 단추들을 거둔다. 멈춰 세워 둔 채로는 돌아가지 못한다.
        if self.held == Some(self.characters[index].window().hwnd()) {
            self.close_fan(now);
        }
        self.characters[index].actor_mut().leave(&self.motions);
        self.characters[index].wake(now);
        self.note(now, format!("꽃핀을 돌려보냄: {index}번"));
    }

    /// 꽃핀 하나를 골라 돌려보낸다. 기본 꽃핀도 보낼 수 있다. 그러면 가장 먼저 초대한 꽃핀이 기본 꽃핀이 된다.
    /// 꽃핀이 혼자면 보내지 않는다.
    fn send_one(&mut self, index: usize, now: Instant) {
        let Some(character) = self.characters.get(index) else { return };
        if self.staying() <= 1 || character.actor().is_leaving() {
            return;
        }
        if character.stay() != Stay::Home {
            return self.send(index, now);
        }
        let Some(heir) = self.guests().next() else { return };
        // 자리를 맞바꾼다. 기본 꽃핀은 늘 맨 앞에 있다.
        self.characters[heir].set_home();
        self.characters[index].set_guest(now);
        self.characters.swap(index, heir);
        self.send(heir, now);
        self.note(now, "기본 꽃핀이 바뀜".to_string());
        self.restack();
    }

    /// 더 부른 꽃핀을 모두 돌려보낸다. 오기로 하고 아직 오지 않은 꽃핀도 오지 않는다.
    fn send_all(&mut self, now: Instant) {
        self.calls.clear();
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

    /// 맨 앞에서 입력을 받는 창이 바뀌었다.
    fn foreground_changed(&mut self, now: Instant) {
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

    /// 꽃핀들을 뒤에 설 것부터 앞에 설 것까지 늘어놓는다([`stack_order`]).
    fn stack_order(&self) -> Vec<usize> {
        let dragged = match self.pointer {
            Pointer::Dragging { hwnd } => self.find(hwnd),
            _ => None,
        };
        let guests: Vec<bool> =
            self.characters.iter().map(|character| matches!(character.stay(), Stay::Guest(_))).collect();
        stack_order(&guests, dragged)
    }

    /// `--watch-stack`이면 꽃핀끼리의 실제 앞뒤를 읽는다. `moved`면 방금 앞뒤를 옮긴 것이고, `settled`면 앞뒤를
    /// 다 맞춘 뒤라서 정한 차례와도 견준다.
    fn watch_stack(&mut self, moved: bool, settled: bool) {
        if self.dev.stack.is_none() {
            return;
        }
        let order = settled.then(|| self.stack_order());
        let windows: Vec<&CharacterWindow> = self.characters.iter().map(Character::window).collect();
        if let Some(watch) = &mut self.dev.stack {
            if moved {
                watch.moved();
            }
            watch.check(&windows, order.as_deref(), Instant::now());
        }
    }

    /// 꽃핀끼리의 앞뒤 차례를 맞춘다. 차례는 [`stack_order`]가 정한다.
    ///
    /// 맨 앞에 설 꽃핀부터 자리를 잡고, 나머지는 바로 앞에 설 꽃핀의 뒤에 차례로 세운다. 이미 그 자리에 있는
    /// 꽃핀은 옮기지 않는다. 화면은 창을 옮기는 도중에도 그려진다. 뒤에 설 꽃핀부터 하나씩 맨 앞으로 올리면
    /// 다 올릴 때까지 뒤에 있어야 할 꽃핀이 잠깐 앞에 나와, 겹친 꽃핀들이 깜빡여 보였다. 앞에서부터 세우면
    /// 자리를 잡은 꽃핀끼리는 옮기는 도중에도 앞뒤가 뒤집히지 않는다.
    fn restack(&mut self) {
        // 방송 촬영용 창 안의 꽃핀은 창을 띄우지 않는다. 앞뒤 차례는 창이 그릴 때 따른다.
        if self.studio.is_some() {
            self.characters.iter_mut().for_each(|character| {
                character.take_raised();
            });
            return;
        }
        let order = self.stack_order();
        let Some((&front, behind)) = order.split_last() else { return };
        if let Some(watch) = &mut self.dev.stack {
            watch.restacked();
        }
        if self.look.topmost {
            // 작업표시줄 같은 다른 "항상 위" 창보다도 앞으로 올린다.
            self.characters[front].keep_on_top();
            self.watch_stack(true, false);
        }
        // "항상 위"가 꺼져 있으면 다른 창들 사이에서의 높이는 건드리지 않는다. 맨 앞에 설 꽃핀이 있는 높이를 따른다.
        // 다른 프로그램의 보이는 창(작업표시줄 등)이 꽃핀들 사이에 끼어 있으면, 그 창보다 앞으로 옮긴다.
        let mut ahead = self.characters[front].window().hwnd();
        for &index in behind.iter().rev() {
            let window = self.characters[index].window();
            let (in_place, between) = window.is_just_behind(ahead);
            if !in_place {
                window.place_behind(ahead);
                let stranger = between.is_some_and(|hwnd| self.find(hwnd).is_none());
                if let Some(watch) = &mut self.dev.stack
                    && stranger
                {
                    watch.stranger();
                }
                self.watch_stack(true, false);
            }
            ahead = self.characters[index].window().hwnd();
        }
        if self.look.topmost
            && let Some(bubble) = &self.bubble
        {
            bubble.window().set_topmost(true);
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
        // 방송 촬영용 창 안에서 누르고 끈 것은 그 자리의 꽃핀 창에서 일어난 것으로 바꿔 함께 다룬다.
        let (from_studio, studio_flow) = self.studio_events(now)?;
        if let Some(next) = studio_flow {
            flow = next;
        }
        let mut events = character_window::take_events();
        events.extend(from_studio);
        for event in events {
            match event {
                WindowEvent::Press { hwnd, .. } if self.is_bubble(hwnd) => {
                    // 말풍선은 누르면 바로 클릭으로 친다. 끌 것이 없으므로 누르고 있기를 그만둔다.
                    character_window::stop_pressing();
                    self.bubble_clicked();
                }
                WindowEvent::Press { hwnd, x, y } => {
                    // 머리 위에 단추들이 떠 있었으면 거둔다.
                    self.close_fan(now);
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
                // 꽃핀을 오른쪽 클릭하면 머리 위에 단추들이 부채꼴로 뜬다. 떠 있는 꽃핀을 다시 누르면 거둔다.
                WindowEvent::Menu { hwnd, .. } => {
                    let again = self.postits.has_fan() && self.menu_target == Some(hwnd);
                    self.close_fan(now);
                    if let (false, Some(index)) = (again, self.find(hwnd)) {
                        self.open_fan(index);
                    }
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
                    self.close_fan(now);
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
                TrayEvent::Dark(dark) => {
                    self.dark = dark;
                    self.note(now, format!("화면 꺼짐: {dark}"));
                }
            }
        }
        for event in watch::take_events() {
            match event {
                WatchEvent::Foreground => self.foreground_changed(now),
                // 창의 자리가 달라졌다. 다음 걸음에서 바로 다시 읽는다.
                WatchEvent::Moved => self.scanned = None,
            }
        }
        for event in settings_window::take_events() {
            if let Some(tab) = self.dialog_event(event, now) {
                flow = Flow::ConfirmReset(tab);
            }
        }

        // 3. 모니터가 바뀌었으면 무대를 다시 읽는다. 화면 밖에 남은 꽃핀은 주 모니터에 다시 나타난다.
        if stage_changed {
            self.screens = Stage::read().pretend_dpi(&self.dev.options.monitor_dpi);
            // 방송 촬영용 창이 열려 있으면 꽃핀들의 무대는 그대로 창 안이다.
            if self.studio.is_none() {
                self.stage = self.screens.clone();
            }
            self.tell_postits_monitors();
            // 화면 밖에 남은 포스트잇도 가까운 화면 안으로 데려온다.
            self.postits.rescue();
            self.rescue_characters(now)?;
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
            self.tend_calls(now)?;
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
            self.check_update(now);
        }

        // 7. 알릴 때가 된 메모를 알린다. 그리고 포스트잇에서 온 일을 처리하고 움직이는 포스트잇을 그린다.
        if !self.is_paused() {
            self.check_memos(now);
        }
        let wall = self.wall(now);
        for outcome in self.postits.frame(&mut self.memos, wall, now) {
            if let Some(next) = self.memo_outcome(outcome, now)? {
                flow = next;
            }
        }

        // 8. 끝내기: 꽃핀들이 손을 흔들고 사라진 뒤에 끝낸다. 한 번 더 고르면 바로 끝낸다.
        if quit_asked && self.ask_quit(now) {
            return Ok(Flow::Quit);
        }

        // 9. 같이 하기에서 때가 된 행동을 시키고, 때가 된 꽃핀의 시간을 흘리고 화면에 넘긴다. 한 명도 남지 않으면 끝낸다.
        self.tend_focus(now);
        if self.postits.has_focus() {
            let model = self.focus_model();
            self.postits.set_focus(model);
        }
        self.play_cues(now);
        self.watch_cursor(now);
        if !self.tick_characters(now)? {
            return Ok(Flow::Quit);
        }

        // 10. 기본 꽃핀이 내려앉은 자리를 기억해 둔다. 다시 켜면 그 위에 나타난다.
        let landed = self
            .home()
            .filter(|_| self.studio.is_none())
            .map(Character::actor)
            .filter(|home| home.mode() == Mode::Ground);
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
        // 숨겨 둔 동안과 방송 촬영용 창을 최소화해 둔 동안에는 꽃핀의 시간이 흐르지 않는다.
        if self.frozen() {
            self.paint_studio(false);
            return Ok(true);
        }
        // 깨어난 김에 곧 올 박자까지 함께 밟는다.
        let horizon = now + character::LOOKAHEAD;
        // 다음 장을 그리기 바로 전에, 앞 장이 화면에 제대로 나와 있는지 본다. 첫 꽃핀이 멈춰 있을 때만 본다.
        if let (Some(check), Some(first), None) = (&mut self.dev.screen_check, self.characters.first(), &self.studio)
            && horizon >= first.due()
            && !first.moved()
        {
            check.sample(first.window(), now);
        }
        self.read_desktop(now);
        // 새로 만든 꽃핀 창은 다른 꽃핀들보다 앞에 놓인다. 처음 보이기 전에 제 차례로 옮겨 둔다.
        if self.characters.iter_mut().fold(false, |any, character| character.take_raised() | any) {
            self.restack();
        }
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
        if let (Some(first), None) = (self.characters.first(), &self.studio)
            && !self.dev.click_checked
            && first.actor().mode() == Mode::Ground
        {
            // 바닥에 선 뒤에 한 번, 몸은 클릭을 받고 투명한 곳은 뒤 창으로 넘기는지 본다.
            self.dev.click_checked = true;
            self.dev.notes.push(dev::click_check(first.window()));
        }
        self.paint_studio(shown);
        let before = self.characters.len();
        self.characters.retain(|character| !character.actor().is_gone());
        if self.postits.has_fan() && self.menu_target.is_none_or(|hwnd| self.find(hwnd).is_none()) {
            self.close_fan(now);
        }
        if self.characters.len() != before {
            self.note(
                now,
                format!("꽃핀 {}명이 사라져 {}명 남음", before - self.characters.len(), self.characters.len()),
            );
        }
        // 새로 나타났거나 걸터앉으려고 앞으로 나온 꽃핀이 있으면 꽃핀끼리의 앞뒤 차례를 다시 맞춘다.
        let raised = self.characters.iter_mut().fold(false, |any, character| character.take_raised() | any);
        self.follow_whims(now);
        if raised {
            self.restack();
        }
        if shown {
            self.watch_stack(false, true);
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
        let (head, half_body, bounds, dpi) = self.head_of(first);
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
            Some(Click::Focus) => self.start_focus(Focus::start(self.focus_plan(), now), now),
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
    fn memo_outcome(&mut self, outcome: Outcome, now: Instant) -> Result<Option<Flow>, PackError> {
        match outcome {
            Outcome::Changed => self.store.memos_changed(now),
            Outcome::Menu { id, x, y } => return Ok(Some(Flow::MemoMenu(id, x, y))),
            Outcome::AskDelete(id) => return Ok(Some(Flow::ConfirmDelete(id))),
            Outcome::Answered { id, snooze: true } => {
                if self.memos.snooze(id, self.wall(now)) {
                    self.store.memos_changed(now);
                    self.note(now, format!("알림을 {SNOOZE_MINUTES}분 뒤로 미룸: 메모 {id}"));
                    self.postits.refresh(&self.memos);
                }
            }
            Outcome::Answered { id, snooze: false } => self.note(now, format!("알림을 확인함: 메모 {id}")),
            Outcome::Create { x, y } => self.new_memo((x, y)),
            Outcome::Fan(pick) => self.fan_picked(pick, now)?,
            Outcome::Focus(act) => self.focus_acted(act, now),
            Outcome::ListTop(top) => {
                if self.state.list_top != top {
                    self.state.list_top = top;
                    self.store.state_changed(now);
                }
                self.note(now, format!("메모 목록 창의 항상 위: {top}"));
            }
            Outcome::FanClosed => self.release_held(now),
        }
        Ok(None)
    }

    /// 꽃핀의 머리 위에 단추들을 부채꼴로 띄운다. 그동안 그 꽃핀은 멈춰서 기다린다.
    ///
    /// 무엇인가를 딛고 서 있을 때만 띄운다. 떠 있거나 잡혀 있거나 돌아가는 중이면 띄우지 않는다.
    /// 꽃핀이 혼자면 "보내기"와 "다 같이"는 넣지 않는다. 다 같이와 바로 가기는 묶음 안에 있다.
    fn open_fan(&mut self, index: usize) {
        let character = &self.characters[index];
        let standing = character.actor().can_speak() && !character.actor().is_leaving();
        if !standing || self.quitting.is_some() || self.out_of_sight {
            return;
        }
        let (head, _, bounds, dpi) = self.head_of(character);
        let hwnd = character.window().hwnd();
        let (can_call, room) = (self.can_call(), self.room());
        let mut buttons = vec![(FanPick::Invite, can_call)];
        let many = self.staying() > 1;
        if many {
            buttons.extend([(FanPick::Send, true), (FanPick::Together, true)]);
        }
        buttons.extend([FanPick::Focus, FanPick::Memos, FanPick::Links, FanPick::Settings].map(|pick| (pick, true)));
        // 초대하기와 보내기도 묶음이다. 트레이 메뉴와 같이, 남은 자리가 없으면 "가득"만 흐리다.
        let calls = [FanPick::CallOne, FanPick::CallThree, FanPick::CallFive, FanPick::CallTen];
        let mut invite: Vec<_> = calls.map(|pick| (pick, can_call)).to_vec();
        invite.push((FanPick::CallFill, room > 0));
        let send = vec![(FanPick::SendThis, true), (FanPick::SendAll, self.can_send())];
        let links = [FanPick::Chzzk, FanPick::Youtube, FanPick::Cafe].map(|pick| (pick, true)).to_vec();
        let together = TOGETHER_PICKS.map(|pick| (pick, true)).to_vec();
        let groups =
            [(FanPick::Invite, invite), (FanPick::Send, send), (FanPick::Links, links), (FanPick::Together, together)];
        self.postits.open_fan(head, bounds, dpi, &buttons, &groups);
        if room > 0 {
            self.postits.name_fan(FanPick::CallFill, format!("가득 채우기 ({room}명)"));
        }
        if self.focus.is_some() {
            self.postits.mark_fan(FanPick::Focus);
        }
        (self.menu_target, self.held) = (Some(hwnd), Some(hwnd));
    }

    /// 머리 위의 단추들을 거두고, 멈춰 세워 둔 꽃핀을 다시 움직이게 한다.
    fn close_fan(&mut self, now: Instant) {
        if self.postits.has_fan() {
            self.postits.close_fan();
            self.release_held(now);
        }
    }

    /// 머리 위의 단추 가운데 하나를 눌렀다. 단추들은 이미 닫혔다.
    fn fan_picked(&mut self, pick: FanPick, now: Instant) -> Result<(), PackError> {
        let index = self.menu_character();
        self.release_held(now);
        let near = self.characters.get(index).map(|character| character.window().hwnd());
        match pick {
            FanPick::CallOne => self.call_guest(now)?,
            FanPick::CallThree => self.call_many(3, near, now),
            FanPick::CallFive => self.call_many(5, near, now),
            FanPick::CallTen => self.call_many(10, near, now),
            FanPick::CallFill => self.call_many(self.room(), near, now),
            FanPick::SendThis => self.send_one(index, now),
            FanPick::SendAll => self.send_all(now),
            FanPick::Memos => {
                // 메모 목록은 그 꽃핀의 오른쪽 옆에 뜬다. 전에 띄웠던 자리가 있으면 그 자리에 뜬다.
                if let Some(character) = self.characters.get(index) {
                    let (head, half_body, _, dpi) = character.head(&self.stage);
                    self.postits.open_list((head.0 + half_body + 220 * dpi.max(96) as i32 / 96, head.1));
                }
            }
            FanPick::Chzzk => self.open(LINKS[links::LIVE].url),
            FanPick::Youtube => self.open(LINKS[links::YOUTUBE].url),
            FanPick::Cafe => self.open(LINKS[links::CAFE].url),
            FanPick::Settings => self.open_settings(),
            FanPick::Focus => self.open_focus_panel(index),
            FanPick::Wave | FanPick::Stretch | FanPick::Yawn => {
                if let Some(index) = TOGETHER_PICKS.iter().position(|&together| together == pick) {
                    self.play_together(index, now);
                }
            }
            // 묶음을 열고 되돌아가는 것은 단추들이 알아서 한다.
            FanPick::Invite | FanPick::Send | FanPick::Links | FanPick::Together | FanPick::Back => {}
        }
        Ok(())
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
        // 방송 촬영용 창 안에는 올라설 창이 없다. "항상 위"가 꺼져 있으면 창 위에 올라서지 않으므로 읽을 까닭이 없다.
        if !self.look.topmost || self.studio.is_some() {
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
        // 숨겨 둔 동안에는 꽃핀 때문에 깨어날 일이 없다. 머리 위에 단추들을 띄우고 멈춰 선 꽃핀도 그렇다.
        // 그 꽃핀은 시간이 흐르지 않아 깰 때가 지난 채로 있으므로, 세어 넣으면 쉬지 못하고 계속 돈다.
        let moving = self.characters.iter().filter(|character| Some(character.window().hwnd()) != self.held);
        let mut wake = if self.frozen() { None } else { moving.map(Character::due).min() };
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
        sooner(self.update_due().filter(|_| !paused));
        sooner((self.memos.has_pending() && self.quitting.is_none() && !paused).then_some(self.next_memo_check));
        sooner(self.postits.due());
        sooner(self.guests_due());
        sooner(self.calls.next);
        sooner(self.cues.iter().map(|cue| cue.at).min());
        sooner(self.gathering.as_ref().map(|gathering| gathering.until));
        sooner(self.watches_cursor().then_some(self.next_cursor_check));
        sooner(self.focus.and_then(|focus| focus.due(self.away)));
        // 쉬는 시간 알림 판이 떠 있으면 남은 시간이 바뀔 때마다 다시 그린다.
        sooner(self.focus.filter(|_| self.postits.has_focus()).and_then(|focus| focus.next_tick(self.away)));
        sooner(self.next_wave.filter(|_| !self.out_of_sight));
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
            Some(MenuItem::CallMany(index)) => self.call_many(tray::CALLS[index], None, now),
            Some(MenuItem::CallFill) => self.call_many(self.room(), None, now),
            Some(MenuItem::SendAll) => self.send_all(now),
            // 고르는 사이에 그 꽃핀이 돌아갔으면 아무 일도 하지 않는다.
            Some(MenuItem::SendThis) => self.send_one(self.menu_character(), now),
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
            Some(MenuItem::Update) => self.open(update::RELEASES_URL),
            Some(MenuItem::MemoNew) => self.new_memo(win::cursor_position()),
            Some(MenuItem::MemoShow) => self.postits.show_all(),
            Some(MenuItem::MemoTidy) => self.postits.tidy(),
            Some(MenuItem::MemoList) => self.postits.open_list(win::cursor_position()),
            Some(MenuItem::Link(index)) => self.open(LINKS[index].url),
            Some(MenuItem::Together(index)) => self.play_together(index, now),
            // 판은 첫 꽃핀 옆에 뜬다.
            Some(MenuItem::FocusPanel) => self.open_focus_panel(self.menu_character()),
            Some(MenuItem::Studio) => self.open_studio(true, now)?,
            Some(MenuItem::Show(show)) => {
                let target = self.menu_character();
                if let Some(character) = self.characters.get_mut(target) {
                    character.show(show, &self.motions, &self.stage, now);
                    self.note(now, format!("행동을 시킴: {target}번에게 {}", show.label()));
                }
            }
            None => {}
        }
        Ok(false)
    }

    /// 단추를 누르거나 행동을 시킬 꽃핀. 머리 위에 단추들을 띄운 꽃핀이고, 트레이에서 골랐거나 그 꽃핀이
    /// 없어졌으면 첫 꽃핀이다.
    fn menu_character(&self) -> usize {
        self.menu_target.and_then(|hwnd| self.find(hwnd)).unwrap_or(0)
    }

    /// 멈춰 세워 둔 꽃핀을 다시 움직이게 한다.
    fn release_held(&mut self, now: Instant) {
        if let Some(hwnd) = self.held.take()
            && let Some(index) = self.find(hwnd)
        {
            self.characters[index].resume(now);
        }
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
        // 숨겨 둔 동안에는 인사할 꽃핀이 보이지 않는다. 방송 촬영용 창을 최소화해 둔 동안에는 꽃핀의 시간이 흐르지
        // 않아 인사를 마치지 못한다. 둘 다 바로 끝낸다.
        if self.quitting.is_some() || self.frozen() {
            return true;
        }
        self.close_fan(now);
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
        self.close_fan(now);
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
        let work = self.screens.nearest(cursor.0, cursor.1).work;
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
        self.show_update();
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
            DialogEvent::OpenProject => self.open(links::PROJECT_URL),
            DialogEvent::CheckUpdate => self.ask_update(now),
            DialogEvent::GetUpdate => self.open(update::RELEASES_URL),
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
                let look = self.character_look();
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
            dev::Command::Dark(dark) => self.dark = dark,
            dev::Command::Hello => self.blink_all(now),
            dev::Command::Foreground => self.foreground_changed(now),
            dev::Command::Cursor(at) => self.dev.cursor = at,
            dev::Command::Studio(word, numbers) => self.studio_script(&word, &numbers, now)?,
            dev::Command::Focus(work, rest) => {
                let lengths = (Duration::from_secs_f64(work), Duration::from_secs_f64(rest));
                self.start_focus(Focus::with(lengths, self.focus_plan().auto, now), now);
            }
            dev::Command::FocusPanel(index) => self.open_focus_panel(index),
            dev::Command::FocusPress(word, minutes) => self.postits.focus_script(&word, minutes),
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
            dev::Command::Update(step) => match step {
                dev::UpdateStep::Latest(version) => {
                    let latest = update::Version::parse(&version);
                    self.update_reply(latest.map_or(update::Reply::Failed, update::Reply::Latest), now);
                }
                dev::UpdateStep::Nothing => self.update_reply(update::Reply::Nothing, now),
                dev::UpdateStep::Fail => self.update_reply(update::Reply::Failed, now),
                dev::UpdateStep::Ask => self.ask_update(now),
            },
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
            dev::Command::Fan(dev::FanStep::Open(index)) => {
                if let Some(hwnd) = self.characters.get(index).map(|character| character.window().hwnd()) {
                    let again = self.postits.has_fan() && self.menu_target == Some(hwnd);
                    self.close_fan(now);
                    if !again {
                        self.open_fan(index);
                    }
                }
            }
            dev::Command::Fan(step) => self.postits.fan_script(&step),
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
                if let Some(line) = self.postits.describe_fan() {
                    self.note(now, line);
                }
                if let Some(line) = self.postits.describe_focus() {
                    self.note(now, line);
                }
                self.dev.snaps += 1;
                if let Some(path) = &self.dev.options.settings_shot {
                    let back_first: Vec<_> =
                        front_first.iter().rev().map(|&index| self.characters[index].window()).collect();
                    let over = self.postits.fan_shot().or_else(|| self.postits.focus_shot());
                    dev::crowd_shot(&back_first, over.as_ref(), &dev::numbered(path, self.dev.snaps));
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
        // 방송 촬영용 창이 열려 있으면 세워 둔 바탕화면 꽃핀의 자리를 기억한다.
        let desktop_home = match &self.parked {
            Some((parked, _)) => parked.first().filter(|character| character.stay() == Stay::Home),
            None => self.home(),
        };
        if let Some(first) = desktop_home.map(Character::actor)
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
        if let Some(watch) = &self.dev.stack {
            lines.push(watch.summary());
        }
        lines.push(format!("풀어서 들고 있는 장 {:.2}MiB", self.frames.kept_bytes() as f64 / 1_048_576.0));
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
    fn several_guests_come_one_after_another() {
        let start = Instant::now();
        let mut calls = Calls::default();
        // 세 명을 부르면 첫 꽃핀은 바로 오고, 나머지는 사이를 두고 온다.
        calls.add(3, 39, start);
        assert_eq!((calls.waiting, calls.next), (3, Some(start)));
        calls.came(start);
        assert_eq!((calls.waiting, calls.next), (2, Some(start + CALL_GAP)));
        // 오는 중에 더 부르면 줄 뒤에 선다. 다음 꽃핀이 올 때는 그대로다.
        calls.add(2, 39, start + CALL_GAP / 2);
        assert_eq!((calls.waiting, calls.next), (4, Some(start + CALL_GAP)));
        // 지금 올 수 없으면 조금 뒤에 다시 본다. 수는 줄지 않는다.
        calls.put_off(start + CALL_GAP * 2);
        assert_eq!((calls.waiting, calls.next), (4, Some(start + CALL_GAP * 2)));
        // 마지막 꽃핀이 오면 더 깨어날 일이 없다.
        for _ in 0..4 {
            calls.came(start + CALL_GAP * 2);
        }
        assert_eq!(calls, Calls::default());
        // 앉을 자리보다 많이는 기다리지 않는다. 아무도 부르지 않았으면 깨어나지 않는다.
        calls.add(10, 4, start);
        calls.add(10, 4, start);
        assert_eq!(calls.waiting, 4);
        calls.clear();
        calls.add(0, 4, start);
        calls.put_off(start);
        assert_eq!(calls, Calls::default());
    }

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
