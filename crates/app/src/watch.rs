//! 다른 창에서 일어난 일을 Windows에게서 전해 듣는다.
//!
//! 전해 듣기만 한다(`SetWinEventHook`을 프로그램 밖에서 받는 방식으로 쓴다). 다른 프로그램 안에 아무것도
//! 넣지 않고, 알림은 이 프로그램의 메시지 루프로 온다. 알림이 없는 동안에는 CPU를 쓰지 않는다.
//!
//! 알림을 받은 자리에서는 적어 두기만 하고, 메인 루프가 [`take_events`]로 꺼내 처리한다.

use std::cell::RefCell;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows_sys::Win32::UI::WindowsAndMessaging::{EVENT_SYSTEM_FOREGROUND, WINEVENT_OUTOFCONTEXT};

/// 다른 창에서 일어난 일.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchEvent {
    /// 맨 앞에서 입력을 받는 창이 바뀌었다. 창들의 앞뒤 순서가 바뀌었을 수 있다.
    Foreground,
}

thread_local! {
    static EVENTS: RefCell<Vec<WatchEvent>> = const { RefCell::new(Vec::new()) };
}

/// 지난번에 꺼낸 뒤로 쌓인 일을 모두 꺼낸다.
pub fn take_events() -> Vec<WatchEvent> {
    EVENTS.with(|events| std::mem::take(&mut *events.borrow_mut()))
}

fn push(event: WatchEvent) {
    EVENTS.with(|events| {
        let mut events = events.borrow_mut();
        // 같은 일이 연달아 오면 한 번만 적는다.
        if events.last() != Some(&event) {
            events.push(event);
        }
    });
}

/// 지켜보는 동안 들고 있는다. 버리면 그만 지켜본다.
pub struct Watch {
    foreground: HWINEVENTHOOK,
}

impl Watch {
    /// 지켜보기 시작한다. Windows가 받아 주지 않으면 알림이 오지 않을 뿐, 앱은 그대로 돌아간다.
    pub fn start() -> Watch {
        let foreground = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                null_mut(),
                Some(on_event),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        Watch { foreground }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        if !self.foreground.is_null() {
            unsafe { UnhookWinEvent(self.foreground) };
        }
    }
}

unsafe extern "system" fn on_event(
    _hook: HWINEVENTHOOK,
    event: u32,
    _hwnd: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    if event == EVENT_SYSTEM_FOREGROUND {
        push(WatchEvent::Foreground);
    }
}
