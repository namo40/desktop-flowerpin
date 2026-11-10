//! 다음에 할 일이 있을 때까지 CPU를 쓰지 않고 기다리는 타이머.

use std::ptr::{null, null_mut};
use std::time::Instant;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::Threading::{
    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CreateWaitableTimerExW, INFINITE, SetWaitableTimer, TIMER_ALL_ACCESS,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, QS_ALLINPUT};

use crate::win::fatal;

/// 정한 시각이 되거나 창 메시지가 오면 깨워 주는 타이머.
///
/// 애니메이션은 장마다 약 42ms 간격이라, 보통 타이머(약 16ms 단위)로는 장 시간이 들쭉날쭉해진다.
/// 그래서 고해상도 대기 타이머를 쓴다. 시스템 전체의 타이머 간격을 바꾸는 `timeBeginPeriod`는 쓰지 않는다.
pub struct WakeTimer {
    handle: HANDLE,
}

impl WakeTimer {
    pub fn new() -> WakeTimer {
        // 고해상도 타이머는 Windows 10 1803부터 있다. 없으면 보통 타이머로 만든다.
        let mut handle =
            unsafe { CreateWaitableTimerExW(null(), null(), CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS) };
        if handle.is_null() {
            handle = unsafe { CreateWaitableTimerExW(null(), null(), 0, TIMER_ALL_ACCESS) };
        }
        if handle.is_null() {
            fatal("타이머를 만들지 못했습니다.");
        }
        WakeTimer { handle }
    }

    /// `due`가 되거나 창 메시지가 올 때까지 잔다. `due`가 없으면 메시지가 올 때까지 잔다.
    pub fn sleep_until(&self, due: Option<Instant>) {
        let Some(due) = due else {
            unsafe { MsgWaitForMultipleObjectsEx(0, null(), INFINITE, QS_ALLINPUT, MWMO_INPUTAVAILABLE) };
            return;
        };
        let remaining = due.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return;
        }
        // 음수는 "지금부터 이만큼 뒤"라는 뜻이고, 단위는 100나노초다.
        let relative = -((remaining.as_nanos() / 100).max(1) as i64);
        unsafe {
            SetWaitableTimer(self.handle, &relative, 0, None, null_mut(), 0);
            MsgWaitForMultipleObjectsEx(1, &self.handle, INFINITE, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        }
    }
}

impl Drop for WakeTimer {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.handle) };
    }
}
