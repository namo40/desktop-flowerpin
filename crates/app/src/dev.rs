//! 만드는 동안 확인하고 재는 데 쓰는 실행 인자. 배포판의 기능이 아니다.
//!
//! ```text
//! DesktopFlowerpin.exe [--for 초] [--report 파일] [--check-screen] [--size 퍼센트] [--dpi 값] [--characters 수]
//!                      [--play 동작,동작] [--script "초 명령; 초 명령"] [--seed 수] [--still]
//!                      [--away-after 초] [--data-dir 폴더] [--no-open] [--settings-dump 파일] [--settings-shot 파일]
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
//! - `--play 동작,동작`: 바닥에 선 채로 시작해 동작을 차례로 보여 준다. `all`이면 팩의 모든 동작이다.
//! - `--script "초 명령; 초 명령"`: 정한 시각에 첫 꽃핀에게 일을 시킨다. 마우스를 움직이지 않고도
//!   잡기와 놓기를 되풀이해 확인할 수 있다. 명령은 [`Command`]에 있다.
//! - `--seed 수`: 쉬는 시간과 걷는 방향을 고르는 난수의 씨앗. 같은 수면 같은 순서로 움직인다.
//! - `--still`: 모든 꽃핀이 돌아다니지 않고 제자리에 서 있는다. 가만히 있을 때의 자원을 재는 데 쓴다.
//! - `--away-after 초`: 마우스와 키보드를 이만큼 쓰지 않으면 자리를 비운 것으로 친다. 0이면 끈다.
//!   주면 설정 파일의 값 대신 이 값을 쓴다.
//! - `--data-dir 폴더`: 설정과 상태 파일을 둘 폴더. `--for`와 함께 쓸 때 이것을 주지 않으면 파일을 읽지도
//!   쓰지도 않는다. 확인하려고 돌린 것이 진짜 설정을 건드리지 않게 하려는 것이다.
//! - `--no-open`: 주소나 폴더를 열라고 해도 열지 않고 보고서에 적기만 한다.
//! - `--screen-check`: `--for`와 함께 쓸 때도 전체 화면 프로그램이 떠 있는지를 진짜로 본다.
//! - `--settings-dump 파일`, `--settings-shot 파일`: 스크립트의 `settings snap`이 설정 창의 컨트롤 좌표와
//!   그림(BMP)을 여기에 쓴다.
//!
//! 보고서에는 첫 꽃핀이 거친 상태와 동작이 시각과 함께 적힌다.
//!
//! `--for`를 [`WARM_UP`]보다 길게 주면, 그 뒤부터 끝날 때까지 쓴 메모리와 CPU를 보고서에 적는다.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, GetDC, ReleaseDC, SRCCOPY, SelectObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GW_HWNDNEXT, GetTopWindow, GetWindow, WindowFromPoint};

use crate::actor::{Actor, Mode, Whim};
use crate::character_window::CharacterWindow;
use crate::geom::Rect;
use crate::memos::Remind;
use crate::motions::{MotionId, Motions};
use crate::usage::{self, Meter};
use crate::when::When;

pub struct Options {
    pub run_for: Option<Duration>,
    pub report: Option<PathBuf>,
    pub check_screen: bool,
    pub size: u32,
    pub dpi: Option<u32>,
    pub characters: usize,
    /// 차례로 보여 줄 동작의 이름.
    pub play: Vec<String>,
    /// 정한 시각에 시킬 일. 시각 순서로 놓여 있다.
    pub script: Vec<Step>,
    pub seed: Option<u64>,
    pub still: bool,
    /// 자리를 비운 것으로 치는 시간. 주면 설정의 값 대신 쓴다.
    pub away_after: Option<Duration>,
    pub data_dir: Option<PathBuf>,
    pub no_open: bool,
    pub settings_dump: Option<PathBuf>,
    pub settings_shot: Option<PathBuf>,
    /// 정한 시간만 도는 확인용 실행에서도 치지직에 진짜로 묻는다. 주지 않으면 확인용 실행은 인터넷을 쓰지 않는다.
    pub live_check: bool,
    /// 확인용 실행에서도 처음 켰을 때의 안내를 한다.
    pub greet: bool,
    /// 확인용 실행에서도 Windows 알림을 진짜로 띄운다. 주지 않으면 보고서에 적기만 한다.
    pub toast: bool,
    /// 확인용 실행에서도 전체 화면 프로그램이 떠 있는지를 진짜로 본다. 주지 않으면 보지 않는다.
    /// 재는 동안 다른 창 때문에 꽃핀이 숨지 않게 하려는 것이다.
    pub screen_check: bool,
    /// `--clock "2026-11-12 18:59:50"`: 이 PC의 시계가 시작할 때 이 시각이었던 것으로 친다. 알림을 확인하는 데 쓴다.
    /// 벽시계의 1970-01-01 00:00부터 흐른 초다.
    pub clock: Option<i64>,
    /// `--run-key 자리`: "Windows 시작할 때 실행"을 진짜 자리 대신 레지스트리(`HKEY_CURRENT_USER` 아래)의
    /// 이 자리에 적는다. `--for`와 함께 쓸 때 이것을 주지 않으면 레지스트리를 건드리지 않는다.
    pub run_key: Option<String>,
    /// `--monitor-dpi 번호=값,번호=값`: 그 번호의 모니터(보고서에 적히는 차례, 0부터)의 DPI를 이 값으로 친다.
    /// 화면 배율이 서로 다른 모니터가 없어도, 꽃핀이나 포스트잇을 배율이 다른 모니터로 옮기는 것을 확인할 수 있다.
    pub monitor_dpi: Vec<(usize, u32)>,
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
            play: value("--play")
                .map(|s| s.split(',').map(|name| name.trim().to_string()).collect())
                .unwrap_or_default(),
            script: value("--script")
                .map(|s| parse_script(&s).unwrap_or_else(|e| crate::win::fatal(&e)))
                .unwrap_or_default(),
            seed: value("--seed").and_then(|s| s.parse().ok()),
            still: args.iter().any(|a| a == "--still"),
            away_after: value("--away-after").and_then(|s| s.parse::<f64>().ok()).map(Duration::from_secs_f64),
            data_dir: value("--data-dir").map(PathBuf::from),
            no_open: args.iter().any(|a| a == "--no-open"),
            settings_dump: value("--settings-dump").map(PathBuf::from),
            settings_shot: value("--settings-shot").map(PathBuf::from),
            live_check: args.iter().any(|a| a == "--live-check"),
            greet: args.iter().any(|a| a == "--greet"),
            toast: args.iter().any(|a| a == "--toast"),
            screen_check: args.iter().any(|a| a == "--screen-check"),
            clock: value("--clock")
                .map(|s| parse_clock(&s).unwrap_or_else(|| crate::win::fatal("--clock을 읽을 수 없습니다."))),
            run_key: value("--run-key"),
            monitor_dpi: value("--monitor-dpi")
                .map(|list| {
                    let pair = |text: &str| {
                        let (index, dpi) = text.split_once('=')?;
                        Some((index.trim().parse().ok()?, dpi.trim().parse().ok()?))
                    };
                    list.split(',').filter_map(pair).collect()
                })
                .unwrap_or_default(),
        }
    }
}

/// `--script`로 시킬 수 있는 일.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// `grab`: 첫 꽃핀을 그 자리에서 잡는다.
    Grab,
    /// `drag 가로 세로`: 잡은 자리에서 그만큼 떨어진 곳으로 끈다(화면 px, 위쪽이 음수).
    Drag(f64, f64),
    /// `release`: 놓는다.
    Release,
    /// `leave`: 메뉴에서 끝내기를 고른 것과 같다. 모든 꽃핀이 사라진 뒤 앱이 끝난다.
    Leave,
    /// `roam on|off`: 돌아다니지 않고 제자리에 서 있게 하거나, 다시 돌아다니게 한다.
    Roam(bool),
    /// `dpi 값`: 모니터 DPI를 이 값으로 친다. 실행 중에 배율이 바뀌는 것을 확인한다.
    Dpi(u32),
    /// `click`: 짧게 클릭한다.
    Click,
    /// `rub 거리`: 머리를 그만큼(px) 문지른다.
    Rub(f64),
    /// `do stretch|yawn|jump|sleep|sit|climb|hop`: 가끔 하는 행동을 바로 시킨다.
    /// climb은 가까운 창 윗변으로 뛰어오르고, hop은 올라서 있던 창에서 뛰어내린다.
    Do(Whim),
    /// `away on|off`: 사용자가 자리를 비운 것으로, 또는 돌아온 것으로 친다.
    /// 한 번 쓰면 그 뒤로는 진짜 입력 시각을 보지 않는다.
    Away(bool),
    /// `fullscreen on|off`: 전체 화면 프로그램이 떠 있는 것으로, 또는 없는 것으로 친다.
    /// 한 번 쓰면 그 뒤로는 진짜 화면을 보지 않는다.
    Fullscreen(bool),
    /// `lock on|off`: 화면을 잠근 것으로, 또는 푼 것으로 친다.
    Lock(bool),
    /// `hello`: 앱을 한 번 더 켰거나 트레이 아이콘을 클릭한 것으로 친다. 꽃핀이 반짝인다.
    Hello,
    /// `hide 초`: 메뉴에서 "잠깐 숨기기"를 고른 것과 같다. 그 시간이 지나면 다시 보인다.
    Hide(f64),
    /// `menu 번호`: 메뉴에서 그 항목을 고른 것으로 친다. 번호는 [`crate::tray::MenuItem`]의 값이다.
    Menu(u32),
    /// `target 번호`: 그 번호의 꽃핀(0이 기본 꽃핀)을 오른쪽 클릭해 메뉴를 띄운 것으로 친다.
    /// 뒤따르는 `menu`가 그 꽃핀에게 간다.
    Target(usize),
    /// `crowd`: 꽃핀이 몇 명인지와, 화면에서 앞에 있는 차례를 보고서에 적는다. `--settings-shot`을 줬으면
    /// 꽃핀들을 그 차례대로 겹쳐 그린 그림도 쓴다.
    Crowd,
    /// `settings ...`: 설정 창을 다룬다.
    Settings(Dialog),
    /// `say 초 제목 | 내용`: 말풍선으로 알릴 것을 줄에 세운다. 초가 0이면 클릭할 때까지 떠 있다.
    /// `|`와 내용은 빼도 된다.
    Say(f64, String, String),
    /// `bubble click`: 떠 있는 말풍선을 클릭한 것으로 친다.
    BubbleClick,
    /// `live open 2026-11-10 13:00:00 제목`, `live close`, `live fail`, `live moved`, `live refused`: 치지직에서
    /// 그런 답을 받은 것으로 친다. 인터넷을 쓰지 않고 방송 안내를 확인하는 데 쓴다. 시작 시각이 같으면 같은 방송이다.
    Live(LiveReply),
    /// `toast click`: Windows 알림을 클릭한 것으로 친다.
    ToastClick,
    /// `bubble dismiss`: 떠 있는 말풍선을 오른쪽 클릭한 것으로 친다.
    BubbleDismiss,
    /// `memo ...`: 포스트잇으로 붙은 메모를 다룬다.
    Memo(MemoStep),
}

/// `--script`의 `memo` 명령. 사람이 포스트잇에서 하는 일을 흉내 낸다. 메모는 번호로 가리킨다.
#[derive(Clone, Debug, PartialEq)]
pub enum MemoStep {
    /// `memo new [글]`: 새 메모를 붙인다. `\n`은 줄바꿈이다.
    New(String),
    /// `memo text 번호 글`: 글을 적는다.
    Text(u64, String),
    /// `memo remind 번호 none`, `memo remind 번호 once 2026-11-12 19:00`, `memo remind 번호 daily 23:00`
    Remind(u64, Remind),
    /// `memo hold 번호`: "꽃핀에게 맡기기"를 누른다.
    Hold(u64),
    /// `memo tool 번호 단추`: 단추를 누른다(0 색, 1 알림, 2 항상 위, 3 지우기).
    /// 지울지 묻는 창에는 "예"라고 답한 것으로 친다.
    Tool(u64, usize),
    /// `memo hover 번호 [단추]`, `memo leave 번호`: 마우스를 올린 것과 벗어난 것.
    Hover(u64, Option<usize>),
    Leave(u64),
    /// `memo edit 번호`, `memo done`: 글을 적기 시작하고 마친다.
    Edit(u64),
    Done,
    /// `memo move 번호 x y`: 화면의 그 자리로 옮긴다.
    Move(u64, i32, i32),
    /// `memo size 번호 너비 높이`
    Size(u64, u32, u32),
    /// `memo panel 번호 color|alarm|close`: 판을 띄우거나 닫는다.
    Panel(u64, String),
    /// `memo press 무엇 값`: 떠 있는 판에서 누른다(swatch, mode, quick, half, hour, minute, day, week).
    Press(String, i32),
    /// `memo custom 색상 진하기 밝기`: 떠 있는 판에서 색을 직접 고른다(0~1).
    Custom(f32, f32, f32),
    /// `memo ok 번호`, `memo snooze 번호`: 울리는 알림에 답한다.
    Answer(u64, bool),
    /// `memo tidy`: 다시 줄 세운다.
    Tidy,
    /// `memo show`: 모두 앞으로 가져온다.
    Show,
    /// `memo snap`: 지금 모습을 `--settings-shot`에 그림으로 쓰고, 보고서에 포스트잇마다 한 줄씩 적는다.
    Snap,
}

/// 찍은 차례를 파일 이름에 붙인다. `창.txt`의 세 번째는 `창_3.txt`다.
pub fn numbered(path: &Path, count: u32) -> PathBuf {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let extension = path.extension().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!("{stem}_{count}.{extension}"))
}

/// `"2026-11-12 18:59:50"`처럼 적은 시각을 벽시계의 초로 읽는다. 초는 빼도 된다.
fn parse_clock(text: &str) -> Option<i64> {
    let (date, time) = text.trim().split_once(' ')?;
    let mut parts = time.split(':');
    let minutes = format!("{date} {}:{}", parts.next()?, parts.next()?);
    let seconds = parts.next().map_or(Some(0), |second| second.parse::<i64>().ok().filter(|s| (0..60).contains(s)))?;
    let (year, month, day, hour, minute) = When::parse(&minutes)?.parts();
    let days = crate::when::days_from_civil(year as i64, month as i64, day as i64);
    Some(days * 86_400 + hour as i64 * 3_600 + minute as i64 * 60 + seconds)
}

/// `--script`의 `live` 명령이 흉내 내는 답.
#[derive(Clone, Debug, PartialEq)]
pub enum LiveReply {
    Open { opened: String, title: String },
    Close,
    Fail,
    Moved,
    Refused,
}

/// `--script`의 `settings` 명령. 사람이 창에서 하는 일을 흉내 낸다.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dialog {
    /// `settings open`, `settings close`
    Open,
    Close,
    /// `settings tab 번호`: 탭을 바꾼다(0부터).
    Tab(usize),
    /// `settings pick 컨트롤 순번`: 콤보 상자에서 그 순번을 고른다.
    Pick(i32, usize),
    /// `settings toggle 컨트롤`: 체크 상자를 누른다.
    Toggle(i32),
    /// `settings type 컨트롤 수`: 수를 적는 칸에 그 수를 적고 다른 곳으로 간다.
    Type(i32, i64),
    /// `settings press 컨트롤`: 버튼을 누른다.
    Press(i32),
    /// `settings reset`: "기본값으로"를 누르고, 묻는 창에는 "예"라고 답한 것으로 친다.
    Reset,
    /// `settings snap`: 지금 모습을 `--settings-dump`와 `--settings-shot`에 쓴다.
    Snap,
}

/// `--script`의 한 줄. 시작하고 `at`이 지났을 때 `command`를 한다.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub at: Duration,
    pub command: Command,
}

/// `"1.5 grab; 2 drag 300 -400; 4 release"` 같은 글을 읽는다. 시각 순서로 돌려준다.
pub fn parse_script(text: &str) -> Result<Vec<Step>, String> {
    let mut steps = Vec::new();
    for line in text.split(';').map(str::trim).filter(|line| !line.is_empty()) {
        let bad = || format!("--script를 읽을 수 없습니다: {line}");
        let words: Vec<&str> = line.split_whitespace().collect();
        let number = |i: usize| words.get(i).and_then(|w| w.parse::<f64>().ok()).ok_or_else(bad);
        let at = number(0).ok().filter(|s| *s >= 0.0).map(Duration::from_secs_f64).ok_or_else(bad)?;
        let command = match (words.get(1).copied(), words.len()) {
            (Some("grab"), 2) => Command::Grab,
            (Some("drag"), 4) => Command::Drag(number(2)?, number(3)?),
            (Some("release"), 2) => Command::Release,
            (Some("leave"), 2) => Command::Leave,
            (Some("roam"), 3) if words[2] == "on" || words[2] == "off" => Command::Roam(words[2] == "on"),
            (Some("dpi"), 3) => Command::Dpi(number(2)? as u32),
            (Some("click"), 2) => Command::Click,
            (Some("rub"), 3) => Command::Rub(number(2)?),
            (Some("do"), 3) => Command::Do(match words[2] {
                "stretch" => Whim::Stretch,
                "yawn" => Whim::Yawn,
                "jump" => Whim::Jump,
                "sleep" => Whim::Sleep,
                "sit" => Whim::Sit,
                "climb" => Whim::Climb,
                "hop" => Whim::Hop,
                _ => return Err(bad()),
            }),
            (Some("away"), 3) if words[2] == "on" || words[2] == "off" => Command::Away(words[2] == "on"),
            (Some("fullscreen"), 3) if words[2] == "on" || words[2] == "off" => Command::Fullscreen(words[2] == "on"),
            (Some("lock"), 3) if words[2] == "on" || words[2] == "off" => Command::Lock(words[2] == "on"),
            (Some("hello"), 2) => Command::Hello,
            (Some("hide"), 3) => Command::Hide(number(2)?),
            (Some("menu"), 3) => Command::Menu(number(2)? as u32),
            (Some("target"), 3) => Command::Target(number(2)? as usize),
            (Some("crowd"), 2) => Command::Crowd,
            (Some("say"), 4..) => {
                let text = words[3..].join(" ");
                let (title, body) = text.split_once(" | ").unwrap_or((&text, ""));
                Command::Say(number(2)?, title.trim().to_string(), body.trim().to_string())
            }
            (Some("bubble"), 3) if words[2] == "click" => Command::BubbleClick,
            (Some("bubble"), 3) if words[2] == "dismiss" => Command::BubbleDismiss,
            (Some("memo"), _) => {
                let id = |i: usize| number(i).ok().filter(|id| *id >= 1.0 && id.fract() == 0.0).map(|id| id as u64);
                let id = |i: usize| id(i).ok_or_else(bad);
                let text = |from: usize| words[from..].join(" ").replace("\\n", "\n");
                Command::Memo(match (words.get(2).copied(), words.len()) {
                    (Some("new"), 3..) => MemoStep::New(text(3)),
                    (Some("text"), 5..) => MemoStep::Text(id(3)?, text(4)),
                    (Some("remind"), 5) if words[4] == "none" => MemoStep::Remind(id(3)?, Remind::None),
                    (Some("remind"), 7) if words[4] == "once" => {
                        MemoStep::Remind(id(3)?, Remind::Once(When::parse(&words[5..7].join(" ")).ok_or_else(bad)?))
                    }
                    (Some("remind"), 6) if words[4] == "daily" => {
                        let (hour, minute) = crate::when::parse_clock(words[5]).ok_or_else(bad)?;
                        MemoStep::Remind(id(3)?, Remind::Daily { hour, minute })
                    }
                    (Some("hold"), 4) => MemoStep::Hold(id(3)?),
                    (Some("tool"), 5) if matches!(words[4], "0" | "1" | "2" | "3") => {
                        MemoStep::Tool(id(3)?, number(4)? as usize)
                    }
                    (Some("hover"), 4) => MemoStep::Hover(id(3)?, None),
                    (Some("hover"), 5) => MemoStep::Hover(id(3)?, Some(number(4)? as usize)),
                    (Some("leave"), 4) => MemoStep::Leave(id(3)?),
                    (Some("edit"), 4) => MemoStep::Edit(id(3)?),
                    (Some("done"), 3) => MemoStep::Done,
                    (Some("move"), 6) => MemoStep::Move(id(3)?, number(4)? as i32, number(5)? as i32),
                    (Some("size"), 6) => MemoStep::Size(id(3)?, number(4)? as u32, number(5)? as u32),
                    (Some("panel"), 5) if matches!(words[4], "color" | "alarm" | "close") => {
                        MemoStep::Panel(id(3)?, words[4].to_string())
                    }
                    (Some("press"), 5) => MemoStep::Press(words[3].to_string(), number(4)? as i32),
                    (Some("custom"), 6) => MemoStep::Custom(number(3)? as f32, number(4)? as f32, number(5)? as f32),
                    (Some("ok"), 4) => MemoStep::Answer(id(3)?, false),
                    (Some("snooze"), 4) => MemoStep::Answer(id(3)?, true),
                    (Some("tidy"), 3) => MemoStep::Tidy,
                    (Some("show"), 3) => MemoStep::Show,
                    (Some("snap"), 3) => MemoStep::Snap,
                    _ => return Err(bad()),
                })
            }
            (Some("toast"), 3) if words[2] == "click" => Command::ToastClick,
            (Some("live"), 6..) if words[2] == "open" => {
                Command::Live(LiveReply::Open { opened: words[3..5].join(" "), title: words[5..].join(" ") })
            }
            (Some("live"), 3) => Command::Live(match words[2] {
                "close" => LiveReply::Close,
                "fail" => LiveReply::Fail,
                "moved" => LiveReply::Moved,
                "refused" => LiveReply::Refused,
                _ => return Err(bad()),
            }),
            (Some("settings"), _) => Command::Settings(match (words.get(2).copied(), words.len()) {
                (Some("open"), 3) => Dialog::Open,
                (Some("close"), 3) => Dialog::Close,
                (Some("reset"), 3) => Dialog::Reset,
                (Some("snap"), 3) => Dialog::Snap,
                (Some("tab"), 4) => Dialog::Tab(number(3)? as usize),
                (Some("toggle"), 4) => Dialog::Toggle(number(3)? as i32),
                (Some("press"), 4) => Dialog::Press(number(3)? as i32),
                (Some("pick"), 5) => Dialog::Pick(number(3)? as i32, number(4)? as usize),
                (Some("type"), 5) => Dialog::Type(number(3)? as i32, number(4)? as i64),
                _ => return Err(bad()),
            }),
            _ => return Err(bad()),
        };
        steps.push(Step { at, command });
    }
    steps.sort_by_key(|step| step.at);
    Ok(steps)
}

/// 첫 꽃핀이 거친 상태와 동작의 기록. 상태나 동작이 바뀔 때마다 한 줄을 적는다.
#[derive(Default)]
pub struct Trace {
    last: Option<(Mode, MotionId)>,
    lines: Vec<String>,
    /// 바닥에 있는 동안 지나간 x의 범위.
    walked: Option<(f64, f64)>,
}

/// 기록이 끝없이 길어지지 않게 하는 한계.
const TRACE_LIMIT: usize = 400;

impl Trace {
    pub fn note(&mut self, seconds: f64, actor: &Actor, motions: &Motions) {
        let (x, y) = actor.position();
        if actor.mode() == Mode::Ground {
            let (low, high) = self.walked.unwrap_or((x, x));
            self.walked = Some((low.min(x), high.max(x)));
        }
        let now = (actor.mode(), actor.motion());
        if self.last != Some(now) && self.lines.len() < TRACE_LIMIT {
            let name = &motions.clip(now.1).name;
            self.lines.push(format!("  {seconds:6.2}초 {:?} {name} ({x:.0}, {y:.0})", now.0));
        }
        self.last = Some(now);
    }

    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec!["첫 꽃핀이 거친 상태와 동작 (시각, 상태, 동작, 발바닥 자리)".to_string()];
        lines.extend(self.lines.iter().cloned());
        if let Some((low, high)) = self.walked {
            lines.push(format!("바닥에서 지나간 x의 범위 {low:.0}~{high:.0} (너비 {:.0})", high - low));
        }
        lines
    }
}

/// 보고서를 파일로 쓴다. 콘솔이 없는 프로그램이라 파일로 남긴다.
pub fn write_report(path: Option<&Path>, lines: &[String]) {
    if let Some(path) = path {
        // 재는 데 쓰는 보조 기능이라, 쓰지 못해도 앱을 멈추지 않는다.
        let _ = std::fs::write(path, lines.join("\n") + "\n");
    }
}

/// 화면에서 앞에 있는 창부터 차례로, 그 창이 `windows`의 몇 번째인지를 늘어놓는다.
pub fn stacking(windows: &[HWND]) -> Vec<usize> {
    let mut order = Vec::new();
    let mut hwnd = unsafe { GetTopWindow(null_mut()) };
    while !hwnd.is_null() {
        order.extend(windows.iter().position(|&window| window == hwnd));
        hwnd = unsafe { GetWindow(hwnd, GW_HWNDNEXT) };
    }
    order
}

/// 꽃핀들의 창을 화면에 놓인 자리대로 겹쳐, 한 장의 그림(BMP)으로 저장한다. `windows`는 뒤에 있는 것부터다.
pub fn crowd_shot(windows: &[&CharacterWindow], path: &Path) {
    let rects: Vec<Rect> = windows.iter().map(|window| window.screen_rect()).collect();
    let Some(all) = rects.iter().copied().reduce(|all, rect| all.union(rect)) else { return };
    let (width, height) = (all.w as usize, all.h as usize);
    // 바탕화면을 흉내 낸 파란 바탕.
    let mut desk = vec![[0x3Bu8, 0x78, 0xDC]; width * height];
    for (window, rect) in windows.iter().zip(&rects) {
        for (row, line) in window.shown().chunks(rect.w as usize).enumerate() {
            let start = (rect.y - all.y) as usize * width + row * width + (rect.x - all.x) as usize;
            for (under, &pixel) in desk[start..start + line.len()].iter_mut().zip(line) {
                blend(under, pixel);
            }
        }
    }
    write_bmp(path, width, &desk);
}

/// 미리 곱한 알파의 픽셀 하나를 불투명한 색 위에 올린다.
pub fn blend(under: &mut [u8; 3], pixel: u32) {
    let alpha = pixel >> 24;
    for (channel, shift) in [(0, 16), (1, 8), (2, 0)] {
        // 미리 곱한 알파라 위의 색은 그대로 더한다.
        let over = (pixel >> shift) & 0xFF;
        under[channel] = (over + under[channel] as u32 * (255 - alpha) / 255).min(255) as u8;
    }
}

/// 한 줄이 `width`개인 색들(빨강, 초록, 파랑)을 그림 파일(BMP)로 쓴다. 폴더가 없으면 만든다.
pub fn write_bmp(path: &Path, width: usize, pixels: &[[u8; 3]]) {
    let height = pixels.len() / width.max(1);
    // 한 줄은 4바이트 단위로 맞춘다.
    let row = (width * 3).div_ceil(4) * 4;
    let mut file = Vec::with_capacity(54 + row * height);
    file.extend_from_slice(b"BM");
    file.extend_from_slice(&((54 + row * height) as u32).to_le_bytes());
    file.extend_from_slice(&[0, 0, 0, 0, 54, 0, 0, 0, 40, 0, 0, 0]);
    file.extend_from_slice(&(width as i32).to_le_bytes());
    // 높이가 음수면 위에서 아래로 놓인 그림이다.
    file.extend_from_slice(&(-(height as i32)).to_le_bytes());
    file.extend_from_slice(&[1, 0, 24, 0]);
    file.extend_from_slice(&[0; 24]);
    for line in pixels.chunks(width.max(1)) {
        for [red, green, blue] in line {
            file.extend_from_slice(&[*blue, *green, *red]);
        }
        file.resize(file.len() + row - width * 3, 0);
    }
    if let Some(folder) = path.parent() {
        let _ = std::fs::create_dir_all(folder);
    }
    let _ = std::fs::write(path, file);
}

/// 화면의 한 점을 눌렀을 때 그 클릭을 `hwnd`가 받는지 알려 준다.
fn receives_click(hwnd: HWND, screen_x: i32, screen_y: i32) -> bool {
    unsafe { WindowFromPoint(POINT { x: screen_x, y: screen_y }) == hwnd }
}

/// 꽃핀의 몸을 누르면 꽃핀이 받고, 투명한 곳을 누르면 뒤 창으로 넘어가는지 본다.
///
/// 창 버퍼에서 불투명한 픽셀과 투명한 픽셀을 하나씩 골라, 그 자리의 클릭을 어느 창이 받는지 묻는다.
pub fn click_check(window: &CharacterWindow) -> String {
    let (rect, pixels) = (window.screen_rect(), window.shown());
    let at = |i: usize| (rect.x + i as i32 % rect.w, rect.y + i as i32 / rect.w);
    let answer = |found: Option<usize>| match found.map(at) {
        Some((x, y)) => receives_click(window.hwnd(), x, y).to_string(),
        None => "그런 픽셀 없음".to_string(),
    };
    let body = answer(pixels.iter().position(|p| p >> 24 == 255));
    let empty = answer(pixels.iter().position(|p| p >> 24 == 0));
    format!("클릭을 받는가: 몸 위 {body}, 투명한 곳 {empty}")
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
pub struct Usage(Meter);

impl Usage {
    pub fn start() -> Usage {
        Usage(Meter::start())
    }

    /// 메모리는 작업 관리자의 "메모리(활성 개인 작업 집합)"와 같은 값이다. CPU는 한 코어를 100%로 본다.
    pub fn summary(&self) -> String {
        let (private, working_set) = usage::memory();
        format!(
            "메모리(개인 작업 집합) {:.2}MiB, 작업 집합 {:.2}MiB, CPU {:.2}% (한 코어 대비, {:.1}초 동안)",
            private as f64 / usage::MIB,
            working_set as f64 / usage::MIB,
            self.0.cpu_percent(),
            self.0.seconds()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_is_read_and_sorted_by_time() {
        let crowd = parse_script("1 menu 7; 2 target 2; 3 crowd").unwrap();
        let commands: Vec<Command> = crowd.iter().map(|step| step.command.clone()).collect();
        assert_eq!(commands, [Command::Menu(7), Command::Target(2), Command::Crowd]);
        let quiet = parse_script("1 fullscreen on; 2 fullscreen off; 3 lock on; 4 lock off; 5 hide 2.5").unwrap();
        let commands: Vec<Command> = quiet.iter().map(|step| step.command.clone()).collect();
        let wanted = [
            Command::Fullscreen(true),
            Command::Fullscreen(false),
            Command::Lock(true),
            Command::Lock(false),
            Command::Hide(2.5),
        ];
        assert_eq!(commands, wanted);
        let more = parse_script("1 click; 2 rub 80.5; 3 do jump; 4 away on; 5 away off").unwrap();
        let commands: Vec<Command> = more.iter().map(|step| step.command.clone()).collect();
        assert_eq!(
            commands,
            [Command::Click, Command::Rub(80.5), Command::Do(Whim::Jump), Command::Away(true), Command::Away(false)]
        );
        let dialog =
            parse_script("1 settings open; 2 settings tab 1; 3 settings pick 1103 2; 4 settings toggle 1201").unwrap();
        let commands: Vec<Command> = dialog.iter().map(|step| step.command.clone()).collect();
        assert_eq!(
            commands,
            [
                Command::Settings(Dialog::Open),
                Command::Settings(Dialog::Tab(1)),
                Command::Settings(Dialog::Pick(1103, 2)),
                Command::Settings(Dialog::Toggle(1201))
            ]
        );
        assert_eq!(parse_script("1 menu 3").unwrap()[0].command, Command::Menu(3));
        let live =
            parse_script("1 live open 2026-11-10 13:00:00 꽃핀 방송; 2 live close; 3 live fail; 4 live moved").unwrap();
        let commands: Vec<Command> = live.iter().map(|step| step.command.clone()).collect();
        let open = LiveReply::Open { opened: "2026-11-10 13:00:00".to_string(), title: "꽃핀 방송".to_string() };
        assert_eq!(commands, [open, LiveReply::Close, LiveReply::Fail, LiveReply::Moved].map(Command::Live));
        assert_eq!(parse_script("1 toast click").unwrap()[0].command, Command::ToastClick);
        let memo = parse_script(
            "1 memo new; 2 memo new 약 먹기\\n물도 마시기; 3 memo text 2 우유 사기; \
             4 memo remind 2 once 2026-11-12 19:00; 5 memo remind 2 daily 23:05; 6 memo remind 2 none; \
             7 memo hold 1; 8 memo tool 1 3; 9 memo hover 2; 10 memo hover 2 1; 11 memo leave 2; 12 memo edit 2; \
             13 memo done; 14 memo move 2 300 -40; 15 memo size 2 300 240; 16 memo panel 2 alarm; \
             17 memo press hour -1; 18 memo custom 0.5 0.25 1; 19 memo ok 2; 20 memo snooze 2; 21 memo tidy; \
             22 memo show; 23 memo snap",
        )
        .unwrap();
        let commands: Vec<Command> = memo.iter().map(|step| step.command.clone()).collect();
        assert_eq!(
            commands,
            [
                MemoStep::New(String::new()),
                MemoStep::New("약 먹기\n물도 마시기".to_string()),
                MemoStep::Text(2, "우유 사기".to_string()),
                MemoStep::Remind(2, Remind::Once(When::new(2026, 11, 12, 19, 0).unwrap())),
                MemoStep::Remind(2, Remind::Daily { hour: 23, minute: 5 }),
                MemoStep::Remind(2, Remind::None),
                MemoStep::Hold(1),
                MemoStep::Tool(1, 3),
                MemoStep::Hover(2, None),
                MemoStep::Hover(2, Some(1)),
                MemoStep::Leave(2),
                MemoStep::Edit(2),
                MemoStep::Done,
                MemoStep::Move(2, 300, -40),
                MemoStep::Size(2, 300, 240),
                MemoStep::Panel(2, "alarm".to_string()),
                MemoStep::Press("hour".to_string(), -1),
                MemoStep::Custom(0.5, 0.25, 1.0),
                MemoStep::Answer(2, false),
                MemoStep::Answer(2, true),
                MemoStep::Tidy,
                MemoStep::Show,
                MemoStep::Snap
            ]
            .map(Command::Memo)
        );
        assert_eq!(parse_script("1 bubble dismiss").unwrap()[0].command, Command::BubbleDismiss);
        // 확인용 시계.
        assert_eq!(parse_clock("1970-01-02 00:01:05"), Some(86_400 + 65));
        assert_eq!(parse_clock("1970-01-01 09:00"), Some(9 * 3_600));
        assert_eq!(
            (parse_clock("1970-01-01"), parse_clock("1970-01-01 09:00:61"), parse_clock("어제 09:00")),
            (None, None, None)
        );
        let steps = parse_script(" 4 release; 1.5 grab ; 2 drag 300 -400;; 9 leave; 0 roam off; 3 dpi 144 ").unwrap();
        let commands: Vec<Command> = steps.iter().map(|step| step.command.clone()).collect();
        assert_eq!(
            commands,
            [
                Command::Roam(false),
                Command::Grab,
                Command::Drag(300.0, -400.0),
                Command::Dpi(144),
                Command::Release,
                Command::Leave
            ]
        );
        assert_eq!(steps[1].at, Duration::from_millis(1500));
    }

    #[test]
    fn bad_script_is_rejected() {
        let wrong = ["grab", "1 fly", "1 drag 300", "-1 grab", "1 roam maybe", "1 grab extra", "1 do dance", "1 rub"];
        let live = ["1 live", "1 live open", "1 live open 2026-11-10 13:00:00", "1 live pause", "1 toast"];
        let memo = [
            "1 memo",
            "1 memo fly",
            "1 memo remind 1",
            "1 memo remind 1 once 2026-02-30 10:00",
            "1 memo remind 1 daily 25:00",
            "1 memo text 0 글",
            "1 memo tool 1 4",
            "1 memo panel 1 open",
        ];
        let live = live.into_iter().chain(memo);
        for text in wrong.into_iter().chain(["1 settings", "1 settings fly", "1 settings pick 1103"]).chain(live) {
            assert!(parse_script(text).is_err(), "{text}");
        }
        assert_eq!(parse_script("").unwrap(), []);
    }
}
