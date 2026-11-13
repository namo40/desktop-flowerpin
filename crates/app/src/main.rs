//! DesktopFlowerpin: 꽃핀이 바탕화면 위를 걸어 다니는 Windows 데스크탑 캐릭터.

// 콘솔 창을 띄우지 않는다.
#![windows_subsystem = "windows"]

mod actor;
mod app;
mod autostart;
mod bubble;
mod character;
mod character_window;
mod checker;
mod clock;
mod desktop;
mod dev;
mod dialog;
mod focus;
mod frames;
mod geom;
mod key;
mod links;
mod live;
mod memos;
mod motions;
mod notices;
mod perch;
mod postit;
mod scale;
mod settings;
mod settings_window;
mod shadow;
mod stage;
mod store;
mod text;
mod together;
mod tray;
mod update;
mod usage;
mod watch;
mod when;
mod win;

use std::time::Instant;

use app::{App, Flow};
use dfp_pack::PackError;
use settings_window::TAB_NAMES;

fn main() {
    // 다른 일을 하기 전에, DLL을 시스템 폴더에서만 찾게 한다.
    win::search_system_folder_only();
    let options = dev::Options::from_args();
    let report = options.report.clone();
    win::enable_per_monitor_dpi();

    // 앱은 하나만 실행한다. 이미 실행 중이면 그쪽에 알리고 끝낸다.
    // 정한 시간만 돌고 끝나는 확인용 실행은 막지 않는다. 재는 동안 다른 실행이 떠 있어도 되게 하려는 것이다.
    let _instance = if options.run_for.is_none() {
        let Some(instance) = tray::claim() else {
            dev::write_report(report.as_deref(), &["이미 실행 중이라 끝냅니다".to_string()]);
            return;
        };
        Some(instance)
    } else {
        None
    };

    match run(options) {
        Ok(lines) => dev::write_report(report.as_deref(), &lines),
        Err(error) => win::fatal(&format!("{error}.")),
    }
}

/// 꽃핀을 띄우고, 끝내라고 할 때까지 돌린다. 보고서에 적을 줄을 돌려준다.
fn run(options: dev::Options) -> Result<Vec<String>, PackError> {
    let mut app = App::new(options)?;
    loop {
        if !app.pump() {
            break;
        }
        match app.frame(Instant::now())? {
            Flow::Continue => {}
            Flow::Quit => break,
            Flow::Menu(x, y) => {
                // 트레이 아이콘을 오른쪽 클릭했을 때의 메뉴.
                let owner = app.menu_owner();
                let live = app.live_menu();
                let update = app.update_menu();
                let state = tray::MenuState {
                    live: live.as_ref(),
                    can_call: app.can_call(),
                    room: app.room(),
                    update: update.as_deref(),
                    can_send: app.can_send(),
                    hidden: app.is_tucked(),
                    focus: app.focus_menu(),
                };
                let item = app::run_modal(&mut app, || tray::pick(owner, x, y, &state));
                if app.picked(item, Instant::now())? {
                    break;
                }
            }
            Flow::MemoMenu(id, x, y) => {
                // 포스트잇을 오른쪽 클릭했을 때의 메뉴.
                let owner = app.menu_owner();
                let items = app.memo_menu(id);
                let picked = app::run_modal(&mut app, || postit::menu(owner, (x, y), &items));
                app.memo_picked(id, picked, (x, y), Instant::now());
            }
            Flow::ConfirmDelete(id) => {
                // 글이 적힌 메모는 지우기 전에 한 번 묻는다.
                let mut yes = app.answers_for_me();
                if let (false, Some(owner)) = (yes, app.memo_hwnd(id)) {
                    yes = app::run_modal(&mut app, || win::confirm(owner, "메모 지우기", "이 메모를 지울까요?"));
                }
                app.delete_confirmed(id, yes, Instant::now());
            }
            Flow::ConfirmReset(tab) => {
                // 되돌리기 전에 한 번 묻는다.
                let mut yes = app.answers_for_me();
                if let (false, Some(owner)) = (yes, app.settings_hwnd()) {
                    let question = format!("\"{}\" 탭의 설정을 기본값으로 되돌릴까요?", TAB_NAMES[tab]);
                    yes = app::run_modal(&mut app, || win::confirm(owner, "기본값으로", &question));
                }
                app.reset_confirmed(tab, yes, Instant::now());
            }
        }
        app.sleep();
    }
    Ok(app.finish())
}
