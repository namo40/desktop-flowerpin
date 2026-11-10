//! 메모 창.
//!
//! 왼쪽에 메모 목록, 오른쪽에 고른 메모의 글과 알림이 있다. 적거나 고르면 바로 메모에 반영된다.
//! 확인이나 저장 버튼이 없다. 틀은 `res/app.rc`에 있고, 설정 창과 같은 여백과 줄 간격을 쓴다.
//!
//! 설정 창과 마찬가지로, 창에서 일어난 일은 대화 상자 함수에서 처리하지 않고 [`MemoEvent`]로 적어 두기만
//! 한다. 메인 루프가 [`take_events`]로 꺼내 처리한다. 메모를 직접 고치지 않고, 창에 적힌 것을
//! [`MemoWindow::read`]로 돌려주기만 한다.

use std::cell::{Cell, RefCell};
use std::fmt::Write as _;
use std::path::Path;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{HWND, LPARAM, SYSTEMTIME, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{
    DTM_GETSYSTEMTIME, DTM_SETFORMATW, DTM_SETSYSTEMTIME, DTN_DATETIMECHANGE, EM_SETLIMITTEXT, GDT_VALID, NMHDR,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, BM_SETCHECK, BN_CLICKED, CB_ADDSTRING, CB_GETCURSEL, CB_SETCURSEL, CBN_SELCHANGE, CreateDialogParamW,
    DestroyWindow, EN_CHANGE, GetDlgItem, GetWindowTextLengthW, GetWindowTextW, IsDialogMessageW, LB_ADDSTRING,
    LB_DELETESTRING, LB_GETCURSEL, LB_INSERTSTRING, LB_RESETCONTENT, LB_SETCURSEL, LBN_SELCHANGE, MSG, SW_SHOW,
    SendMessageW, SetDlgItemTextW, SetForegroundWindow, ShowWindow, WM_CLOSE, WM_COMMAND, WM_NOTIFY,
};

use crate::dialog::{self, resource};
use crate::memos::{COUNT_LIMIT, Memo, Memos, Remind, TEXT_LIMIT};
use crate::when::When;
use crate::win::wide;

/// 틀(`res/app.rc`)에 적힌 번호들.
const DIALOG: u16 = 300;
const ID_LIST: i32 = 3001;
const ID_NEW: i32 = 3002;
const ID_DELETE: i32 = 3003;
const ID_TEXT: i32 = 3004;
const ID_KIND: i32 = 3005;
const ID_DATE: i32 = 3006;
const ID_TIME: i32 = 3007;
/// 다음에 언제 알리는지 적는 줄.
const ID_NEXT: i32 = 3008;
const ID_HELD: i32 = 3009;
const ID_TEXT_LABEL: i32 = 3012;
const ID_KIND_LABEL: i32 = 3013;
const ID_DATE_LABEL: i32 = 3014;
const ID_TIME_LABEL: i32 = 3015;
/// 닫기 버튼. Esc를 눌러도 이 번호로 온다.
const ID_CLOSE: i32 = 2;

/// 알림 종류. 콤보 상자에 놓이는 차례다.
const KINDS: [&str; 3] = ["없음", "한 번", "날마다"];
const KIND_ONCE: usize = 1;
const KIND_DAILY: usize = 2;
/// 시각 고르는 칸에 보이는 모양. "오후 7:00"처럼 보인다(초는 고르지 않는다).
const TIME_FORMAT: &str = "tt h:mm";

/// 메모 창에서 일어난 일.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoEvent {
    /// 목록에서 다른 메모를 골랐다.
    Select,
    /// 글, 알림, "꽃핀이 들고 있기" 가운데 무엇인가를 바꿨다.
    Edited,
    /// "새 메모"를 눌렀다.
    New,
    /// "지우기"를 눌렀다.
    Delete,
    /// 닫기 버튼, 제목 줄의 닫기, Esc.
    Close,
}

thread_local! {
    static EVENTS: RefCell<Vec<MemoEvent>> = const { RefCell::new(Vec::new()) };
    /// 코드가 창을 채우는 중인가. 그동안 컨트롤이 보내는 "바뀌었다"는 알림은 사람이 바꾼 것이 아니므로 버린다.
    static FILLING: Cell<bool> = const { Cell::new(false) };
}

fn push(event: MemoEvent) {
    if !FILLING.get() {
        EVENTS.with(|events| events.borrow_mut().push(event));
    }
}

/// 닫기 버튼을 누른 것으로 친다. 만드는 동안 확인용이다.
pub fn push_close() {
    push(MemoEvent::Close);
}

/// 지난번에 꺼낸 뒤로 쌓인 일을 모두 꺼낸다.
pub fn take_events() -> Vec<MemoEvent> {
    EVENTS.with(|events| std::mem::take(&mut *events.borrow_mut()))
}

/// 창을 채우는 동안에는 컨트롤의 알림을 일로 적지 않는다.
fn quietly<T>(fill: impl FnOnce() -> T) -> T {
    let before = FILLING.replace(true);
    let result = fill();
    FILLING.set(before);
    result
}

/// 창의 오른쪽에 적혀 있는 것.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Draft {
    pub text: String,
    pub remind: Remind,
    pub held: bool,
}

pub struct MemoWindow {
    hwnd: HWND,
    /// 목록의 줄마다, 그 줄의 메모 번호와 보이는 글.
    rows: Vec<(u64, String)>,
}

impl MemoWindow {
    /// 메모 창을 만들어 보여 준다. 가운데가 화면의 `center`에 온다. 첫 메모가 골라져 있다.
    pub fn open(memos: &Memos, now: When, center: (i32, i32)) -> Option<MemoWindow> {
        dialog::prepare();
        let instance = unsafe { GetModuleHandleW(null()) };
        // 설정 창처럼 주인 창을 두지 않는다. 작업표시줄에 보이고, 다른 창 뒤로 가도 다시 찾기 쉽다.
        let hwnd = unsafe { CreateDialogParamW(instance, resource(DIALOG), null_mut(), Some(window_proc), 0) };
        if hwnd.is_null() {
            return None;
        }
        let mut window = MemoWindow { hwnd, rows: Vec::new() };
        quietly(|| unsafe {
            for kind in KINDS {
                SendMessageW(window.item(ID_KIND), CB_ADDSTRING, 0, wide(kind).as_ptr() as LPARAM);
            }
            SendMessageW(window.item(ID_TIME), DTM_SETFORMATW, 0, wide(TIME_FORMAT).as_ptr() as LPARAM);
            // 줄바꿈은 창 안에서 두 글자(CR LF)라, 줄이 많으면 한도보다 조금 덜 적힌다. 읽을 때 다시 자른다.
            SendMessageW(window.item(ID_TEXT), EM_SETLIMITTEXT, TEXT_LIMIT, 0);
        });
        let first = memos.list().first().map(|memo| memo.id);
        window.fill(memos, first, now);
        dialog::center_on(hwnd, center);
        unsafe {
            ShowWindow(hwnd, SW_SHOW);
            SetFocus(window.item(if first.is_some() { ID_TEXT } else { ID_NEW }));
        }
        Some(window)
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    /// 이미 열려 있는 창을 앞으로 가져온다.
    pub fn bring_to_front(&self) {
        unsafe { SetForegroundWindow(self.hwnd) };
    }

    /// 이 창이 받아야 할 키보드 입력이면 처리하고 `true`를 돌려준다. 메시지 루프에서 부른다.
    pub fn handles(&self, message: &MSG) -> bool {
        unsafe { IsDialogMessageW(self.hwnd, message) != 0 }
    }

    fn item(&self, id: i32) -> HWND {
        unsafe { GetDlgItem(self.hwnd, id) }
    }

    fn set_text(&self, id: i32, text: &str) {
        unsafe { SetDlgItemTextW(self.hwnd, id, wide(text).as_ptr()) };
    }

    fn text(&self, id: i32) -> String {
        let item = self.item(id);
        let length = unsafe { GetWindowTextLengthW(item) }.max(0) as usize;
        let mut buffer = vec![0u16; length + 1];
        let length = unsafe { GetWindowTextW(item, buffer.as_mut_ptr(), buffer.len() as i32) };
        String::from_utf16_lossy(&buffer[..length.max(0) as usize])
    }

    /// 목록을 다시 채우고, 번호가 `select`인 메모를 골라 오른쪽에 보여 준다.
    /// 그런 메모가 없으면 아무것도 고르지 않고 오른쪽을 비운다.
    pub fn fill(&mut self, memos: &Memos, select: Option<u64>, now: When) {
        self.rows = memos.list().iter().map(|memo| (memo.id, memo.title())).collect();
        let row = select.and_then(|id| self.rows.iter().position(|&(other, _)| other == id));
        quietly(|| unsafe {
            let list = self.item(ID_LIST);
            SendMessageW(list, LB_RESETCONTENT, 0, 0);
            for (_, title) in &self.rows {
                SendMessageW(list, LB_ADDSTRING, 0, wide(title).as_ptr() as LPARAM);
            }
            // 고를 줄이 없으면 -1을 넘겨 고른 것을 없앤다.
            SendMessageW(list, LB_SETCURSEL, row.unwrap_or(usize::MAX), 0);
            EnableWindow(self.item(ID_NEW), (self.rows.len() < COUNT_LIMIT) as i32);
        });
        self.show(row.and_then(|row| memos.get(self.rows[row].0)), now);
    }

    /// 목록에서 고른 메모의 번호.
    pub fn selected(&self) -> Option<u64> {
        let row = unsafe { SendMessageW(self.item(ID_LIST), LB_GETCURSEL, 0, 0) };
        usize::try_from(row).ok().and_then(|row| self.rows.get(row)).map(|&(id, _)| id)
    }

    /// 메모 하나를 오른쪽에 보여 준다. `None`이면 비우고 흐리게 한다.
    pub fn show(&self, memo: Option<&Memo>, now: When) {
        // 알림이 없는 메모는, 알림을 켰을 때 고르기 쉽게 다음 정각을 보여 둔다.
        let (_, _, _, _, minute) = now.parts();
        let soon = now.plus_minutes(60 - minute as i64);
        let (kind, at) = match memo.map(|memo| memo.remind) {
            Some(Remind::Once(at)) => (KIND_ONCE, at),
            Some(Remind::Daily { hour, minute }) => {
                let (year, month, day, ..) = now.parts();
                (KIND_DAILY, When::new(year, month, day, hour, minute).unwrap_or(soon))
            }
            _ => (0, soon),
        };
        quietly(|| unsafe {
            self.set_text(ID_TEXT, &memo.map_or(String::new(), |memo| memo.text.replace('\n', "\r\n")));
            SendMessageW(self.item(ID_KIND), CB_SETCURSEL, kind, 0);
            let (year, month, day, hour, minute) = at.parts();
            let mut time: SYSTEMTIME = std::mem::zeroed();
            (time.wYear, time.wMonth, time.wDay) = (year as u16, month as u16, day as u16);
            (time.wHour, time.wMinute) = (hour as u16, minute as u16);
            for id in [ID_DATE, ID_TIME] {
                SendMessageW(
                    self.item(id),
                    DTM_SETSYSTEMTIME,
                    GDT_VALID as usize,
                    &time as *const SYSTEMTIME as LPARAM,
                );
            }
            SendMessageW(self.item(ID_HELD), BM_SETCHECK, memo.is_some_and(|memo| memo.held) as usize, 0);
            for id in [ID_TEXT, ID_TEXT_LABEL, ID_KIND, ID_KIND_LABEL, ID_HELD, ID_DELETE] {
                EnableWindow(self.item(id), memo.is_some() as i32);
            }
        });
        self.show_remind(memo);
    }

    /// 알림 종류에 맞춰 날짜와 시각 칸을 흐리게 하거나 되살리고, 다음에 언제 알리는지 적는다.
    fn show_remind(&self, memo: Option<&Memo>) {
        let (date, time) = match memo.map(|memo| memo.remind) {
            Some(Remind::Once(_)) => (true, true),
            Some(Remind::Daily { .. }) => (false, true),
            _ => (false, false),
        };
        let line = match memo {
            Some(memo) => memo.remind_line(),
            None if self.rows.len() >= COUNT_LIMIT => format!("메모는 {COUNT_LIMIT}개까지 둘 수 있습니다."),
            None => "\"새 메모\"를 눌러 적어 보세요.".to_string(),
        };
        unsafe {
            for (id, on) in [(ID_DATE, date), (ID_DATE_LABEL, date), (ID_TIME, time), (ID_TIME_LABEL, time)] {
                EnableWindow(self.item(id), on as i32);
            }
        }
        if self.text(ID_NEXT) != line {
            self.set_text(ID_NEXT, &line);
        }
    }

    /// 오른쪽에 적혀 있는 것을 읽는다.
    pub fn read(&self) -> Draft {
        let picked = |id: i32| {
            let mut time: SYSTEMTIME = unsafe { std::mem::zeroed() };
            unsafe { SendMessageW(self.item(id), DTM_GETSYSTEMTIME, 0, &mut time as *mut SYSTEMTIME as LPARAM) };
            time
        };
        let (date, time) = (picked(ID_DATE), picked(ID_TIME));
        let (hour, minute) = (time.wHour as u32, time.wMinute as u32);
        let kind = unsafe { SendMessageW(self.item(ID_KIND), CB_GETCURSEL, 0, 0) };
        let remind = match usize::try_from(kind) {
            Ok(KIND_ONCE) => When::new(date.wYear as i32, date.wMonth as u32, date.wDay as u32, hour, minute)
                .map_or(Remind::None, Remind::Once),
            Ok(KIND_DAILY) => Remind::Daily { hour, minute },
            _ => Remind::None,
        };
        Draft {
            text: self.text(ID_TEXT).replace("\r\n", "\n").chars().take(TEXT_LIMIT).collect(),
            remind,
            held: unsafe { SendMessageW(self.item(ID_HELD), BM_GETCHECK, 0, 0) } == 1,
        }
    }

    /// 메모가 바뀐 뒤에 부른다. 목록에 보이는 그 메모의 줄과, 알림에 딸린 칸들을 맞춘다.
    /// 글을 적는 칸은 건드리지 않는다(적는 중일 수 있다).
    pub fn refresh(&mut self, memo: &Memo) {
        let Some(row) = self.rows.iter().position(|&(id, _)| id == memo.id) else { return };
        let title = memo.title();
        if self.rows[row].1 != title {
            // 목록의 한 줄은 고쳐 적을 수 없어서, 지우고 그 자리에 다시 넣는다.
            quietly(|| unsafe {
                let list = self.item(ID_LIST);
                let was_selected = SendMessageW(list, LB_GETCURSEL, 0, 0) == row as isize;
                SendMessageW(list, LB_DELETESTRING, row, 0);
                SendMessageW(list, LB_INSERTSTRING, row, wide(&title).as_ptr() as LPARAM);
                if was_selected {
                    SendMessageW(list, LB_SETCURSEL, row, 0);
                }
            });
            self.rows[row].1 = title;
        }
        if self.selected() == Some(memo.id) {
            self.show_remind(Some(memo));
        }
    }

    /// 글을 적는 칸으로 간다. 새 메모를 만든 뒤에 부른다.
    pub fn focus_text(&self) {
        unsafe { SetFocus(self.item(ID_TEXT)) };
    }

    /// 사람이 목록에서 그 줄을 고른 것처럼 한다. 만드는 동안 확인용이다.
    pub fn select(&self, row: usize) {
        let list = self.item(ID_LIST);
        unsafe {
            SendMessageW(list, LB_SETCURSEL, row, 0);
            SendMessageW(self.hwnd, WM_COMMAND, (LBN_SELCHANGE as usize) << 16 | ID_LIST as usize, list as LPARAM);
        }
    }

    /// 사람이 글을 적은 것처럼 한다. 만드는 동안 확인용이다.
    pub fn type_text(&self, text: &str) {
        // 글을 넣으면 칸이 스스로 "바뀌었다"고 알린다.
        self.set_text(ID_TEXT, &text.replace('\n', "\r\n"));
    }

    /// 사람이 알림을 고른 것처럼 한다. 만드는 동안 확인용이다.
    pub fn pick_remind(&self, remind: Remind, now: When) {
        let (kind, at) = match remind {
            Remind::None => (0, None),
            Remind::Once(at) => (KIND_ONCE, Some(at)),
            Remind::Daily { hour, minute } => {
                let (year, month, day, ..) = now.parts();
                (KIND_DAILY, When::new(year, month, day, hour, minute))
            }
        };
        quietly(|| unsafe {
            SendMessageW(self.item(ID_KIND), CB_SETCURSEL, kind, 0);
            if let Some((year, month, day, hour, minute)) = at.map(When::parts) {
                let mut time: SYSTEMTIME = std::mem::zeroed();
                (time.wYear, time.wMonth, time.wDay) = (year as u16, month as u16, day as u16);
                (time.wHour, time.wMinute) = (hour as u16, minute as u16);
                for id in [ID_DATE, ID_TIME] {
                    let at = &time as *const SYSTEMTIME as LPARAM;
                    SendMessageW(self.item(id), DTM_SETSYSTEMTIME, GDT_VALID as usize, at);
                }
            }
        });
        let kinds = self.item(ID_KIND);
        unsafe {
            SendMessageW(self.hwnd, WM_COMMAND, (CBN_SELCHANGE as usize) << 16 | ID_KIND as usize, kinds as LPARAM)
        };
    }

    /// 사람이 "꽃핀이 들고 있기"를 누른 것처럼 한다. 만드는 동안 확인용이다.
    pub fn toggle_held(&self) {
        let held = self.item(ID_HELD);
        let checked = unsafe { SendMessageW(held, BM_GETCHECK, 0, 0) } == 1;
        unsafe { SendMessageW(held, BM_SETCHECK, (!checked) as usize, 0) };
        self.press(ID_HELD);
    }

    /// 사람이 "새 메모"를 누른 것처럼 한다. 만드는 동안 확인용이다.
    pub fn press_new(&self) {
        self.press(ID_NEW);
    }

    /// 사람이 "지우기"를 누른 것처럼 한다. 만드는 동안 확인용이다.
    pub fn press_delete(&self) {
        self.press(ID_DELETE);
    }

    fn press(&self, id: i32) {
        let control = self.item(id);
        unsafe { SendMessageW(self.hwnd, WM_COMMAND, (BN_CLICKED as usize) << 16 | id as usize, control as LPARAM) };
    }

    /// 컨트롤마다 실제 좌표(px)와 글을 파일에 적는다. 만드는 동안 정렬을 확인하는 데 쓴다.
    pub fn dump(&self, path: &Path) {
        let mut text = String::new();
        let _ = writeln!(text, "[메모 창] 고른 메모 {:?}, 목록 {}줄", self.selected(), self.rows.len());
        dialog::describe_children(self.hwnd, &mut text);
        let _ = std::fs::write(path, text);
    }

    /// 창을 그림(BMP)으로 저장한다. 만드는 동안 눈으로 확인하는 데 쓴다.
    pub fn shot(&self, path: &Path) {
        dialog::shot(self.hwnd, path);
    }
}

impl Drop for MemoWindow {
    fn drop(&mut self) {
        unsafe { DestroyWindow(self.hwnd) };
    }
}

unsafe extern "system" fn window_proc(_hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> isize {
    match message {
        WM_COMMAND => {
            let (id, code) = ((wparam & 0xFFFF) as i32, ((wparam >> 16) & 0xFFFF) as u32);
            match (code, id) {
                (BN_CLICKED, ID_CLOSE) => push(MemoEvent::Close),
                (BN_CLICKED, ID_NEW) => push(MemoEvent::New),
                (BN_CLICKED, ID_DELETE) => push(MemoEvent::Delete),
                (LBN_SELCHANGE, ID_LIST) => push(MemoEvent::Select),
                (EN_CHANGE, ID_TEXT) | (CBN_SELCHANGE, ID_KIND) | (BN_CLICKED, ID_HELD) => push(MemoEvent::Edited),
                _ => {}
            }
        }
        WM_NOTIFY => {
            let header = unsafe { &*(lparam as *const NMHDR) };
            let picker = header.idFrom == ID_DATE as usize || header.idFrom == ID_TIME as usize;
            if picker && header.code == DTN_DATETIMECHANGE {
                push(MemoEvent::Edited);
            }
        }
        WM_CLOSE => push(MemoEvent::Close),
        _ => {}
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    const RC: &str = include_str!("../res/app.rc");

    /// 틀에서 메모 창의 본문을 찾는다.
    fn body() -> &'static str {
        let start = RC.find(&format!("\n{DIALOG} DIALOGEX")).expect("메모 창의 틀이 없음");
        let begin = start + RC[start..].find("BEGIN").unwrap();
        &RC[begin..begin + RC[begin..].find("\nEND").unwrap()]
    }

    #[test]
    fn every_control_exists_in_the_resource() {
        let kinds = [
            (ID_LIST, "LISTBOX"),
            (ID_NEW, "PUSHBUTTON"),
            (ID_DELETE, "PUSHBUTTON"),
            (ID_TEXT, "EDITTEXT"),
            (ID_KIND, "COMBOBOX"),
            (ID_DATE, "CONTROL"),
            (ID_TIME, "CONTROL"),
            (ID_NEXT, "LTEXT"),
            (ID_HELD, "AUTOCHECKBOX"),
            (ID_TEXT_LABEL, "LTEXT"),
            (ID_KIND_LABEL, "LTEXT"),
            (ID_DATE_LABEL, "LTEXT"),
            (ID_TIME_LABEL, "LTEXT"),
            (ID_CLOSE, "DEFPUSHBUTTON"),
        ];
        for (id, kind) in kinds {
            let line = body().lines().map(str::trim).find(|line| line.contains(&format!(" {id},")));
            assert!(line.is_some_and(|line| line.starts_with(kind)), "{id} ({kind})");
        }
        // 날짜와 시각을 고르는 칸은 공용 컨트롤이다.
        assert_eq!(body().matches("\"SysDateTimePick32\"").count(), 2);
    }

    #[test]
    fn shortcut_letters_do_not_clash() {
        let body = body();
        let letters: Vec<char> = body.match_indices('&').filter_map(|(at, _)| body[at + 1..].chars().next()).collect();
        for (index, letter) in letters.iter().enumerate() {
            assert!(!letters[..index].contains(letter), "Alt+{letter}가 겹침");
        }
        assert_eq!(letters.len(), 8);
    }

    #[test]
    fn the_columns_line_up() {
        // 오른쪽의 이름은 x 110에서, 고르는 칸은 x 150에서 시작해 x 243에서 끝난다.
        for line in body().lines().map(str::trim) {
            let numbers: Vec<i32> =
                line.rsplit('"').next().unwrap_or("").split(',').filter_map(|n| n.trim().parse().ok()).collect();
            if line.starts_with("COMBOBOX") || line.starts_with("CONTROL") {
                assert_eq!((numbers[0], numbers[0] + numbers[2]), (150, 243), "{line}");
            } else if line.starts_with("EDITTEXT") {
                assert_eq!((numbers[0], numbers[0] + numbers[2]), (110, 243), "{line}");
            } else if line.starts_with("AUTOCHECKBOX") {
                assert_eq!((numbers[1], numbers[1] + numbers[3]), (110, 243), "{line}");
            }
        }
    }
}
