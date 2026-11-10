//! 설정 창.
//!
//! Win32 대화 상자다. 틀은 `res/app.rc`에 있다(맑은 고딕 9pt, 탭 컨트롤 하나와 탭마다 자식 대화 상자).
//! 열 때 만들고 닫으면 없앤다. 닫혀 있는 동안에는 메모리를 쓰지 않는다.
//!
//! 창에서 일어난 일(값을 바꿈, 버튼을 누름)은 대화 상자 함수에서 처리하지 않고 [`DialogEvent`]로 적어
//! 두기만 한다. 메인 루프가 [`take_events`]로 꺼내 처리한다. 값은 [`SettingsWindow::read`]로 한꺼번에
//! 읽으므로, 어느 컨트롤이 바뀌었는지는 가리지 않는다.
//!
//! 수를 적는 칸은 적는 동안에는 글을 건드리지 않는다. 비어 있거나 범위를 벗어난 채로 칸을 떠나면
//! 그때 저장된 값으로 고쳐 적는다([`SettingsWindow::tidy`]).

use std::cell::RefCell;
use std::fmt::Write as _;
use std::path::Path;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{
    EnableThemeDialogTexture, NMHDR, TCIF_TEXT, TCITEMW, TCM_ADJUSTRECT, TCM_GETCURSEL, TCM_INSERTITEMW, TCM_SETCURSEL,
    TCN_SELCHANGE,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetKeyState, SetFocus, VK_CONTROL, VK_SHIFT, VK_TAB,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, BM_SETCHECK, BN_CLICKED, CB_ADDSTRING, CB_GETCURSEL, CB_RESETCONTENT, CB_SETCURSEL, CBN_SELCHANGE,
    CreateDialogParamW, DestroyIcon, DestroyWindow, EN_CHANGE, EN_KILLFOCUS, GetClientRect, GetDlgCtrlID, GetDlgItem,
    GetNextDlgTabItem, GetWindowRect, GetWindowTextW, HICON, IsDialogMessageW, MSG, MapDialogRect, STM_SETICON,
    SW_HIDE, SW_SHOW, SW_SHOWNA, SWP_NOACTIVATE, SendDlgItemMessageW, SendMessageW, SetDlgItemTextW,
    SetForegroundWindow, SetWindowPos, ShowWindow, WM_CLOSE, WM_COMMAND, WM_KEYDOWN, WM_NOTIFY,
};

use crate::dialog::{self, rect_in, resource};
use crate::links::LINKS;
use crate::settings::{self, Key, Settings};
use crate::win::wide;

/// 틀(`res/app.rc`)에 적힌 번호들.
const DIALOG: u16 = 100;
const FIRST_PAGE: u16 = 201;
const ID_TABS: i32 = 101;
const ID_RESET: i32 = 102;
/// 닫기 버튼. Esc를 눌러도 이 번호로 온다.
const ID_CLOSE: i32 = 2;
const ID_LIVE_STATUS: i32 = 1305;
const ID_FIRST_LINK: i32 = 1321;
const ID_ICON: i32 = 1501;
const ID_VERSION: i32 = 1503;
const ID_MEMORY: i32 = 1511;
const ID_CPU: i32 = 1512;
const ID_OPEN_FOLDER: i32 = 1521;
const ID_OPEN_PROJECT: i32 = 1522;

pub const TAB_NAMES: [&str; 5] = ["일반", "꽃핀", "방송", "방해하지 않기", "정보"];
/// 정보 탭. 되돌릴 설정이 없다.
const INFO_TAB: usize = 4;
/// 탭 쪽에 탭 안쪽과 같은 바탕을 입히라는 값(`ETDT_ENABLETAB`).
const TAB_TEXTURE: u32 = 6;
/// 글을 적는 칸에 적을 수 있는 글자 수를 정하는 메시지(`EM_SETLIMITTEXT`).
const SET_TEXT_LIMIT: u32 = 0x00C5;

/// 설정 항목과 컨트롤의 짝.
struct Control {
    key: Key,
    id: i32,
    /// 콤보 상자나 수를 적는 칸 앞의 이름. 흐리게 할 때 함께 흐리게 한다.
    label: Option<i32>,
    /// 이 항목이 켜져 있어야 고를 수 있다. 꺼지면 숨기지 않고 흐리게 한다(배치가 밀리지 않게).
    needs: Option<Key>,
}

const fn check(key: Key, id: i32) -> Control {
    Control { key, id, label: None, needs: None }
}

const fn combo(key: Key, id: i32, label: i32, needs: Option<Key>) -> Control {
    Control { key, id, label: Some(label), needs }
}

/// 수를 직접 적는 칸.
const fn field(key: Key, id: i32, label: i32) -> Control {
    Control { key, id, label: Some(label), needs: None }
}

/// 컨트롤의 종류. 설정 항목의 생김새로 정해진다.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Check,
    Combo,
    Field,
}

impl Control {
    fn kind(&self) -> Kind {
        let spec = settings::spec(self.key);
        if spec.is_flag() {
            Kind::Check
        } else if spec.range.is_some() {
            Kind::Field
        } else {
            Kind::Combo
        }
    }
}

const CONTROLS: [Control; settings::COUNT] = [
    check(Key::StartWithWindows, 1101),
    combo(Key::Size, 1103, 1112, None),
    check(Key::AlwaysOnTop, 1104),
    combo(Key::IdleActions, 1203, 1212, None),
    field(Key::Stay, 1205, 1214),
    combo(Key::MaxCharacters, 1206, 1215, None),
    check(Key::LiveCheck, 1301),
    combo(Key::LiveCheckInterval, 1302, 1311, Some(Key::LiveCheck)),
    combo(Key::LiveStartNotice, 1303, 1312, Some(Key::LiveCheck)),
    combo(Key::LiveEndNotice, 1304, 1313, Some(Key::LiveCheck)),
    check(Key::HideOnFullscreen, 1401),
    check(Key::ReminderToast, 1403),
];

/// 설정 창에서 일어난 일.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogEvent {
    /// 체크 상자, 콤보 상자, 수를 적는 칸의 값이 바뀌었다.
    Changed,
    /// 수를 적는 칸을 떠났다. 적다 만 글을 저장된 값으로 고쳐 적을 때다.
    Tidy,
    /// "기본값으로"를 눌렀다.
    Reset,
    /// 닫기 버튼, 제목 줄의 닫기, Esc.
    Close,
    /// 탭 머리를 눌러 탭을 바꿨다.
    Tab(usize),
    /// 바로 가기 버튼. [`LINKS`]의 몇 번째인지다.
    Link(usize),
    OpenFolder,
    OpenProject,
}

thread_local! {
    static EVENTS: RefCell<Vec<DialogEvent>> = const { RefCell::new(Vec::new()) };
}

fn push(event: DialogEvent) {
    EVENTS.with(|events| events.borrow_mut().push(event));
}

/// 닫기 버튼을 누른 것으로 친다. 만드는 동안 확인용이다.
pub fn push_close() {
    push(DialogEvent::Close);
}

/// 지난번에 꺼낸 뒤로 쌓인 일을 모두 꺼낸다.
pub fn take_events() -> Vec<DialogEvent> {
    EVENTS.with(|events| std::mem::take(&mut *events.borrow_mut()))
}

pub struct SettingsWindow {
    hwnd: HWND,
    tabs: HWND,
    pages: [HWND; TAB_NAMES.len()],
    current: usize,
    icon: HICON,
}

impl SettingsWindow {
    /// 설정 창을 만들어 보여 준다. 가운데가 화면의 `center`에 온다.
    ///
    /// `icon`은 정보 탭에 보여 줄 그림이고, 창이 맡아서 닫힐 때 없앤다. `version`은 그 옆에 적을 글이다.
    pub fn open(
        settings: &Settings,
        tab: usize,
        center: (i32, i32),
        icon: HICON,
        version: &str,
    ) -> Option<SettingsWindow> {
        // 탭 컨트롤을 쓰려면 공용 컨트롤을 한 번 준비시켜야 한다.
        dialog::prepare();
        let instance = unsafe { GetModuleHandleW(null()) };
        // 주인 창을 두지 않는다. 그래야 작업표시줄에 보이고, 다른 창 뒤로 가도 다시 찾기 쉽다.
        let hwnd = unsafe { CreateDialogParamW(instance, resource(DIALOG), null_mut(), Some(main_proc), 0) };
        if hwnd.is_null() {
            unsafe { DestroyIcon(icon) };
            return None;
        }
        let tabs = unsafe { GetDlgItem(hwnd, ID_TABS) };
        for (i, name) in TAB_NAMES.iter().enumerate() {
            let mut text = wide(name);
            let mut item: TCITEMW = unsafe { std::mem::zeroed() };
            item.mask = TCIF_TEXT;
            item.pszText = text.as_mut_ptr();
            unsafe { SendMessageW(tabs, TCM_INSERTITEMW, i, &item as *const TCITEMW as LPARAM) };
        }

        // 탭의 표시 영역에 탭 쪽 다섯 개를 겹쳐 놓고, 고른 탭의 것만 보여 준다.
        let mut area = rect_in(hwnd, tabs);
        unsafe { SendMessageW(tabs, TCM_ADJUSTRECT, 0, &mut area as *mut RECT as LPARAM) };
        let mut pages = [null_mut(); TAB_NAMES.len()];
        for (i, page) in pages.iter_mut().enumerate() {
            *page = unsafe { CreateDialogParamW(instance, resource(FIRST_PAGE + i as u16), hwnd, Some(page_proc), 0) };
            if page.is_null() {
                unsafe {
                    DestroyWindow(hwnd);
                    DestroyIcon(icon);
                }
                return None;
            }
            unsafe {
                EnableThemeDialogTexture(*page, TAB_TEXTURE);
                // 순서를 맨 앞으로 끼워 넣는다. Tab 키가 "탭 머리 → 탭 쪽의 항목 → 기본값으로 → 닫기"로 돈다.
                let (w, h) = (area.right - area.left, area.bottom - area.top);
                SetWindowPos(*page, null_mut(), area.left, area.top, w, h, SWP_NOACTIVATE);
            }
        }

        let mut window = SettingsWindow { hwnd, tabs, pages, current: 0, icon };
        for control in &CONTROLS {
            let spec = settings::spec(control.key);
            let item = window.item(control.id);
            match control.kind() {
                Kind::Check => {}
                Kind::Combo => {
                    unsafe { SendMessageW(item, CB_RESETCONTENT, 0, 0) };
                    for pick in spec.picks {
                        unsafe { SendMessageW(item, CB_ADDSTRING, 0, wide(pick.label).as_ptr() as LPARAM) };
                    }
                }
                Kind::Field => {
                    // 가장 큰 수의 자릿수까지만 적게 한다.
                    let digits = spec.range.map_or(1, |(_, most)| most.to_string().len());
                    unsafe { SendMessageW(item, SET_TEXT_LIMIT, digits, 0) };
                }
            }
        }
        window.fill(settings);
        window.set_text(ID_VERSION, version);
        unsafe {
            SendDlgItemMessageW(window.pages[INFO_TAB], ID_ICON, STM_SETICON, icon as WPARAM, 0);
            // 저장소 주소가 아직 정해지지 않았다. 정해지면 연다.
            EnableWindow(window.item(ID_OPEN_PROJECT), 0);
        }

        dialog::center_on(hwnd, center);
        window.show_tab(tab.min(TAB_NAMES.len() - 1));
        unsafe {
            ShowWindow(hwnd, SW_SHOW);
            SetFocus(tabs);
        }
        Some(window)
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    /// 지금 보고 있는 탭.
    pub fn tab(&self) -> usize {
        self.current
    }

    /// 이미 열려 있는 창을 앞으로 가져온다.
    pub fn bring_to_front(&self) {
        unsafe { SetForegroundWindow(self.hwnd) };
    }

    /// 번호가 `id`인 컨트롤. 번호의 앞 두 자리로 어느 탭의 것인지 안다(11xx는 일반, 12xx는 꽃핀…).
    fn item(&self, id: i32) -> HWND {
        unsafe { GetDlgItem(self.owner_of(id), id) }
    }

    fn owner_of(&self, id: i32) -> HWND {
        match id / 100 {
            page @ 11..=15 => self.pages[(page - 11) as usize],
            _ => self.hwnd,
        }
    }

    fn set_text(&self, id: i32, text: &str) {
        unsafe { SetDlgItemTextW(self.owner_of(id), id, wide(text).as_ptr()) };
    }

    fn text(&self, id: i32) -> String {
        let mut buffer = [0u16; 32];
        let length = unsafe { GetWindowTextW(self.item(id), buffer.as_mut_ptr(), buffer.len() as i32) };
        String::from_utf16_lossy(&buffer[..length.max(0) as usize])
    }

    /// 탭을 바꾼다.
    pub fn show_tab(&mut self, tab: usize) {
        self.current = tab;
        for (i, page) in self.pages.iter().enumerate() {
            unsafe { ShowWindow(*page, if i == tab { SW_SHOWNA } else { SW_HIDE }) };
        }
        unsafe {
            if SendMessageW(self.tabs, TCM_GETCURSEL, 0, 0) as usize != tab {
                SendMessageW(self.tabs, TCM_SETCURSEL, tab, 0);
            }
            // 정보 탭에는 되돌릴 것이 없다.
            EnableWindow(GetDlgItem(self.hwnd, ID_RESET), (tab != INFO_TAB) as i32);
        }
    }

    /// 설정의 값을 컨트롤에 채운다.
    pub fn fill(&self, settings: &Settings) {
        for control in &CONTROLS {
            let picked = settings.get(control.key);
            match control.kind() {
                Kind::Check => unsafe { SendMessageW(self.item(control.id), BM_SETCHECK, picked, 0) },
                Kind::Combo => unsafe { SendMessageW(self.item(control.id), CB_SETCURSEL, picked, 0) },
                Kind::Field => {
                    self.set_text(control.id, &settings.number(control.key).to_string());
                    0
                }
            };
        }
        self.update_dependents(settings);
    }

    /// 수를 적는 칸의 글이 저장된 값과 다르면 저장된 값으로 고쳐 적는다. 칸을 떠났을 때 부른다.
    ///
    /// 비워 두었거나 범위를 벗어난 수를 적은 채로 두면, 보이는 것과 실제 값이 달라지기 때문이다.
    pub fn tidy(&self, settings: &Settings) {
        for control in CONTROLS.iter().filter(|control| control.kind() == Kind::Field) {
            let value = settings.number(control.key).to_string();
            if self.text(control.id) != value {
                self.set_text(control.id, &value);
            }
        }
    }

    /// 컨트롤의 값을 설정으로 읽는다. 달라진 것이 있으면 `true`를 돌려준다.
    pub fn read(&self, settings: &mut Settings) -> bool {
        let before = settings.clone();
        for control in &CONTROLS {
            let message = match control.kind() {
                Kind::Check => BM_GETCHECK,
                Kind::Combo => CB_GETCURSEL,
                Kind::Field => {
                    // 비어 있거나 수가 아니면 바꾸지 않는다. 적는 중일 수 있다.
                    if let Ok(value) = self.text(control.id).trim().parse::<i64>() {
                        settings.set_number(control.key, value);
                    }
                    continue;
                }
            };
            let picked = unsafe { SendMessageW(self.item(control.id), message, 0, 0) };
            // 콤보 상자에서 아무것도 고르지 않았으면 음수가 온다.
            if let Ok(picked) = usize::try_from(picked) {
                settings.set(control.key, picked);
            }
        }
        self.update_dependents(settings);
        *settings != before
    }

    /// 다른 항목에 딸린 항목을 흐리게 하거나 되살린다.
    fn update_dependents(&self, settings: &Settings) {
        for control in &CONTROLS {
            if let Some(needs) = control.needs {
                let on = settings.is_on(needs) as i32;
                unsafe {
                    EnableWindow(self.item(control.id), on);
                    if let Some(label) = control.label {
                        EnableWindow(self.item(label), on);
                    }
                }
            }
        }
    }

    /// 방송 탭의 상태 줄을 고친다. 길면 끝이 줄어 보인다.
    pub fn set_live_status(&self, text: &str) {
        if self.text(ID_LIVE_STATUS) != text {
            self.set_text(ID_LIVE_STATUS, text);
        }
    }

    /// 정보 탭의 "지금 쓰는 자원"을 고친다. 메모리는 MiB, CPU는 한 코어를 100으로 본 값이다.
    pub fn set_usage(&self, memory: f64, cpu: f64) {
        self.set_text(ID_MEMORY, &format!("{memory:.1} MB"));
        self.set_text(ID_CPU, &format!("{cpu:.1}%"));
    }

    /// 이 창이 받아야 할 키보드 입력이면 처리하고 `true`를 돌려준다. 메시지 루프에서 부른다.
    ///
    /// Tab과 Alt+글자는 Windows의 대화 상자 처리에 맡긴다. Ctrl+Tab은 탭 컨트롤만으로는 되지 않아 직접 넘긴다.
    pub fn handles(&mut self, message: &MSG) -> bool {
        let ctrl = || unsafe { GetKeyState(VK_CONTROL as i32) } < 0;
        if message.message == WM_KEYDOWN && message.wParam == VK_TAB as usize && ctrl() {
            let back = unsafe { GetKeyState(VK_SHIFT as i32) } < 0;
            let count = TAB_NAMES.len();
            let next = if back { (self.current + count - 1) % count } else { (self.current + 1) % count };
            self.show_tab(next);
            push(DialogEvent::Tab(next));
            return true;
        }
        unsafe { IsDialogMessageW(self.hwnd, message) != 0 }
    }

    /// 사람이 콤보 상자에서 고른 것처럼 한다. 만드는 동안 확인용이다.
    pub fn pick(&self, id: i32, index: usize) {
        let (owner, control) = (self.owner_of(id), self.item(id));
        unsafe {
            SendMessageW(control, CB_SETCURSEL, index, 0);
            SendMessageW(owner, WM_COMMAND, (CBN_SELCHANGE as usize) << 16 | id as usize, control as LPARAM);
        }
    }

    /// 사람이 체크 상자를 누른 것처럼 한다. 만드는 동안 확인용이다.
    pub fn toggle(&self, id: i32) {
        let control = self.item(id);
        let checked = unsafe { SendMessageW(control, BM_GETCHECK, 0, 0) } == 1;
        unsafe { SendMessageW(control, BM_SETCHECK, (!checked) as usize, 0) };
        self.press(id);
    }

    /// 사람이 칸에 글을 적고 다른 곳으로 간 것처럼 한다. 만드는 동안 확인용이다.
    pub fn type_text(&self, id: i32, text: &str) {
        let (owner, control) = (self.owner_of(id), self.item(id));
        self.set_text(id, text);
        for code in [EN_CHANGE, EN_KILLFOCUS] {
            unsafe { SendMessageW(owner, WM_COMMAND, (code as usize) << 16 | id as usize, control as LPARAM) };
        }
    }

    /// 사람이 버튼을 누른 것처럼 한다. 만드는 동안 확인용이다.
    pub fn press(&self, id: i32) {
        let (owner, control) = (self.owner_of(id), self.item(id));
        unsafe { SendMessageW(owner, WM_COMMAND, (BN_CLICKED as usize) << 16 | id as usize, control as LPARAM) };
    }

    /// 컨트롤마다 실제 좌표(px)와 Tab 키로 도는 순서를 파일에 적는다. 만드는 동안 정렬을 확인하는 데 쓴다.
    pub fn dump(&self, path: &Path) {
        let mut text = String::new();
        let mut unit = RECT { left: 0, top: 0, right: 4, bottom: 8 };
        let mut client = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        let mut frame = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        unsafe {
            MapDialogRect(self.hwnd, &mut unit);
            GetClientRect(self.hwnd, &mut client);
            GetWindowRect(self.hwnd, &mut frame);
        }
        let size = |r: &RECT| format!("x{} y{} w{} h{}", r.left, r.top, r.right - r.left, r.bottom - r.top);
        let _ = writeln!(text, "기준 단위(4x8 DLU) = {}x{} px", unit.right, unit.bottom);
        let _ = writeln!(
            text,
            "창 전체 {}x{} px, 안쪽 {}x{} px",
            frame.right - frame.left,
            frame.bottom - frame.top,
            client.right,
            client.bottom
        );
        let _ = writeln!(text, "탭 컨트롤 {}", size(&rect_in(self.hwnd, self.tabs)));
        let _ = writeln!(text, "탭 쪽 {}", size(&rect_in(self.hwnd, self.pages[0])));
        for id in [ID_RESET, ID_CLOSE] {
            let _ = writeln!(text, "버튼 {id} {}", size(&rect_in(self.hwnd, unsafe { GetDlgItem(self.hwnd, id) })));
        }
        // Tab 키로 도는 순서(지금 보이는 탭 기준).
        let first = unsafe { GetNextDlgTabItem(self.hwnd, null_mut(), 0) };
        let (mut at, mut order) = (first, Vec::new());
        for _ in 0..40 {
            if at.is_null() {
                break;
            }
            order.push(unsafe { GetDlgCtrlID(at) }.to_string());
            at = unsafe { GetNextDlgTabItem(self.hwnd, at, 0) };
            if at == first {
                break;
            }
        }
        let _ =
            writeln!(text, "지금 탭 {} ({}), Tab 순서: {}", self.current, TAB_NAMES[self.current], order.join(" > "));
        for (i, page) in self.pages.iter().enumerate() {
            let _ = writeln!(text, "[{}]", TAB_NAMES[i]);
            dialog::describe_children(*page, &mut text);
        }
        let _ = std::fs::write(path, text);
    }

    /// 창을 그림(BMP)으로 저장한다. 만드는 동안 눈으로 확인하는 데 쓴다.
    pub fn shot(&self, path: &Path) {
        dialog::shot(self.hwnd, path);
    }
}

impl Drop for SettingsWindow {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.hwnd);
            DestroyIcon(self.icon);
        }
    }
}

/// 탭 쪽의 대화 상자 함수.
unsafe extern "system" fn page_proc(_page: HWND, message: u32, wparam: WPARAM, _lparam: LPARAM) -> isize {
    if message == WM_COMMAND {
        let (id, code) = ((wparam & 0xFFFF) as i32, ((wparam >> 16) & 0xFFFF) as u32);
        let links = ID_FIRST_LINK..ID_FIRST_LINK + LINKS.len() as i32;
        match (code, id) {
            (BN_CLICKED, id) if links.contains(&id) => push(DialogEvent::Link((id - ID_FIRST_LINK) as usize)),
            (BN_CLICKED, ID_OPEN_FOLDER) => push(DialogEvent::OpenFolder),
            (BN_CLICKED, ID_OPEN_PROJECT) => push(DialogEvent::OpenProject),
            // 체크 상자를 눌렀거나, 콤보 상자에서 골랐거나, 칸에 적은 글이 바뀌었다.
            (BN_CLICKED, _) | (CBN_SELCHANGE, _) | (EN_CHANGE, _) => push(DialogEvent::Changed),
            (EN_KILLFOCUS, _) => push(DialogEvent::Tidy),
            _ => {}
        }
    }
    0
}

/// 설정 창 뼈대의 대화 상자 함수.
unsafe extern "system" fn main_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> isize {
    match message {
        WM_COMMAND => match (wparam & 0xFFFF) as i32 {
            ID_CLOSE => push(DialogEvent::Close),
            ID_RESET => push(DialogEvent::Reset),
            _ => {}
        },
        WM_NOTIFY => {
            let header = unsafe { &*(lparam as *const NMHDR) };
            if header.idFrom == ID_TABS as usize && header.code == TCN_SELCHANGE {
                let tabs = unsafe { GetDlgItem(hwnd, ID_TABS) };
                push(DialogEvent::Tab(unsafe { SendMessageW(tabs, TCM_GETCURSEL, 0, 0) } as usize));
            }
        }
        WM_CLOSE => push(DialogEvent::Close),
        _ => {}
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    const RC: &str = include_str!("../res/app.rc");

    /// 틀에서 대화 상자 하나의 본문을 찾는다.
    fn dialog_body(id: u16) -> &'static str {
        let start = RC.find(&format!("\n{id} DIALOGEX")).unwrap_or_else(|| panic!("대화 상자 {id}가 없음"));
        let begin = start + RC[start..].find("BEGIN").unwrap();
        &RC[begin..begin + RC[begin..].find("\nEND").unwrap()]
    }

    #[test]
    fn controls_are_in_key_order_and_exist_in_the_resource() {
        for (i, control) in CONTROLS.iter().enumerate() {
            let spec = settings::spec(control.key);
            assert_eq!(control.key as usize, i);
            // 번호의 앞 두 자리가 탭이다.
            assert_eq!((control.id / 100 - 11) as usize, spec.tab, "{}", spec.name);
            let body = dialog_body(FIRST_PAGE + spec.tab as u16);
            let kind = match control.kind() {
                Kind::Check => "AUTOCHECKBOX",
                Kind::Combo => "COMBOBOX",
                Kind::Field => "EDITTEXT",
            };
            let line = body.lines().find(|line| line.contains(&format!(" {},", control.id)));
            assert!(line.is_some_and(|line| line.trim_start().starts_with(kind)), "{} ({})", spec.name, control.id);
            assert_eq!(control.label.is_some(), !spec.is_flag(), "{}", spec.name);
            if let Some(label) = control.label {
                assert!(body.contains(&format!(", {label},")), "{}의 이름 {label}", spec.name);
            }
            if let Some(needs) = control.needs {
                assert!(settings::spec(needs).is_flag() && settings::spec(needs).tab == spec.tab);
            }
        }
    }

    #[test]
    fn shortcut_letters_do_not_clash_within_a_tab() {
        // Alt+글자는 한 탭 안에서 겹치면 안 된다. 뼈대의 "기본값으로(D)"와도 겹치면 안 된다.
        let letters = |body: &str| -> Vec<char> {
            body.match_indices('&').filter_map(|(at, _)| body[at + 1..].chars().next()).collect()
        };
        let frame = letters(dialog_body(DIALOG));
        assert_eq!(frame, ['D']);
        for (tab, name) in TAB_NAMES.iter().enumerate() {
            let mut seen = frame.clone();
            for letter in letters(dialog_body(FIRST_PAGE + tab as u16)) {
                assert!(!seen.contains(&letter), "{name} 탭에서 Alt+{letter}가 겹침");
                seen.push(letter);
            }
        }
    }

    #[test]
    fn every_page_uses_the_same_grid() {
        // 콤보 상자는 모두 B열(x 118, 너비 107), 체크 상자는 x 7에서 B열 끝까지(너비 218)다.
        // 수를 적는 칸은 B열의 왼쪽 끝에서 시작한다.
        for tab in 0..TAB_NAMES.len() {
            for line in dialog_body(FIRST_PAGE + tab as u16).lines().map(str::trim) {
                let numbers: Vec<i32> =
                    line.rsplit('"').next().unwrap_or("").split(',').filter_map(|n| n.trim().parse().ok()).collect();
                if line.starts_with("COMBOBOX") {
                    assert_eq!((numbers[0], numbers[2]), (118, 107), "{line}");
                } else if line.starts_with("EDITTEXT") {
                    assert_eq!(numbers[0], 118, "{line}");
                } else if line.starts_with("AUTOCHECKBOX") {
                    assert_eq!((numbers[1], numbers[3], numbers[4]), (7, 218, 10), "{line}");
                } else if line.starts_with("PUSHBUTTON") {
                    assert!(matches!((numbers[1], numbers[3], numbers[4]), (7, 107, 14) | (118, 107, 14)), "{line}");
                }
            }
        }
    }

    #[test]
    fn link_buttons_match_the_links() {
        let body = dialog_body(FIRST_PAGE + 2);
        for index in 0..LINKS.len() as i32 {
            assert!(body.contains(&format!(", {},", ID_FIRST_LINK + index)));
        }
    }
}
