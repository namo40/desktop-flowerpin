//! 화면에 보이는 다른 프로그램의 창들을 읽는다.
//!
//! 창이 어디에 있고 무엇이 앞에 있는지만 본다. 창의 제목이나 내용은 읽지 않는다.
//! 읽은 것에서 올라설 수 있는 윗변을 고르는 일은 [`crate::perch`]가 한다.

use std::ffi::c_void;

use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT};
use windows_sys::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GW_OWNER, GWL_EXSTYLE, GWL_STYLE, GetClassNameW, GetWindow, GetWindowLongPtrW, GetWindowRect,
    IsIconic, IsWindowVisible, WS_CAPTION, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};
use windows_sys::core::BOOL;

use crate::character_window;
use crate::geom::Rect;
use crate::perch::Pane;

/// 화면에 보이는 창들. 앞에 있는 창부터 차례로 들어 있다.
#[derive(Default)]
pub struct Desktop {
    panes: Vec<Pane>,
    /// 읽은 것이 달라질 때마다 하나씩 는다. 달라졌을 때만 다시 계산하게 하려는 것이다.
    version: u64,
}

impl Desktop {
    pub fn panes(&self) -> &[Pane] {
        &self.panes
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    /// 보이는 창을 모두 다시 읽는다.
    pub fn scan(&mut self) {
        let mut panes: Vec<Pane> = Vec::with_capacity(self.panes.len());
        unsafe { EnumWindows(Some(add_pane), &mut panes as *mut Vec<Pane> as LPARAM) };
        self.replace(panes);
    }

    /// 창 하나만 다시 읽는다. 올라서 있는 창이 움직였는지, 없어졌는지를 자주 보는 데 쓴다.
    /// 앞뒤 순서는 다시 읽지 않는다.
    pub fn track(&mut self, id: isize) {
        let Some(at) = self.panes.iter().position(|pane| pane.id == id) else { return };
        match read(id as HWND) {
            Some(pane) if pane == self.panes[at] => {}
            Some(pane) => {
                self.panes[at] = pane;
                self.version += 1;
            }
            None => {
                self.panes.remove(at);
                self.version += 1;
            }
        }
    }

    /// 아무 창도 없는 것으로 친다.
    pub fn clear(&mut self) {
        self.replace(Vec::new());
    }

    fn replace(&mut self, panes: Vec<Pane>) {
        if panes != self.panes {
            self.panes = panes;
            self.version += 1;
        }
    }
}

unsafe extern "system" fn add_pane(hwnd: HWND, list: LPARAM) -> BOOL {
    let panes = unsafe { &mut *(list as *mut Vec<Pane>) };
    panes.extend(read(hwnd));
    1
}

fn class_of(hwnd: HWND) -> String {
    let mut name = [0u16; 64];
    let length = unsafe { GetClassNameW(hwnd, name.as_mut_ptr(), name.len() as i32) };
    String::from_utf16_lossy(&name[..length.max(0) as usize])
}

/// 창 하나를 읽는다. 화면에 보이지 않는 창이면 `None`이다.
fn read(hwnd: HWND) -> Option<Pane> {
    if unsafe { IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 } {
        return None;
    }
    // 다른 가상 데스크톱에 있거나 숨겨 둔 앱 창은 "보임"으로 나오지만 실제로는 그려지지 않는다.
    let mut cloaked = 0u32;
    let size = size_of::<u32>() as u32;
    unsafe { DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED as u32, &mut cloaked as *mut u32 as *mut c_void, size) };
    if cloaked != 0 {
        return None;
    }
    let ex_style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
    // 클릭이 그대로 통과하는 투명한 덮개 창(화면 전체를 덮는 오버레이 따위)은 없는 것으로 친다.
    if ex_style & WS_EX_TRANSPARENT != 0 && ex_style & WS_EX_LAYERED != 0 {
        return None;
    }
    let class = class_of(hwnd);
    // 꽃핀 자신과 바탕화면은 창으로 치지 않는다.
    // 포스트잇과 글을 적는 칸도 이 프로그램의 창이다. 꽃핀이 올라설 창으로 치지 않는다.
    let ours = class == character_window::CLASS_NAME || class.starts_with(crate::postit::NOTE_CLASS);
    if ours || class == "Progman" || class == "WorkerW" {
        return None;
    }

    // 창의 테두리 바깥에는 보이지 않는 여백(크기를 바꾸려고 잡는 곳)이 있다. 눈에 보이는 범위를 읽는다.
    let mut frame = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    let size = size_of::<RECT>() as u32;
    let read_frame = unsafe {
        DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS as u32, &mut frame as *mut RECT as *mut c_void, size)
    };
    if read_frame != 0 && unsafe { GetWindowRect(hwnd, &mut frame) } == 0 {
        return None;
    }
    let rect = Rect::new(frame.left, frame.top, frame.right - frame.left, frame.bottom - frame.top);
    if rect.is_empty() {
        return None;
    }

    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32;
    let taskbar = class == "Shell_TrayWnd" || class == "Shell_SecondaryTrayWnd";
    // 제목 줄이 있는 창이거나, 주인 없이 혼자 떠 있는 보통 창만 창으로 친다.
    // 메뉴, 풍선 도움말, 펼친 목록처럼 잠깐 떴다 사라지는 것은 둘 다 아니다. 이런 것은 없는 것으로 친다.
    // 가리는 것으로 치면, 꽃핀을 오른쪽 클릭해 메뉴를 띄우기만 해도 선 자리가 가려진 것이 되어 창에서 떨어진다.
    let captioned = style & WS_CAPTION == WS_CAPTION;
    let on_its_own = unsafe { GetWindow(hwnd, GW_OWNER) }.is_null() && ex_style & WS_EX_TOOLWINDOW == 0;
    if !taskbar && !captioned && !on_its_own {
        return None;
    }
    // 작업표시줄은 다른 것을 가리기만 하고, 그 위는 창 윗변이 아니라 바닥이다.
    Some(Pane { id: hwnd as isize, rect, standable: !taskbar })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_desktop_can_be_read() {
        let mut desktop = Desktop::default();
        desktop.scan();
        // 작업표시줄이든 무엇이든 보이는 창이 하나는 있다.
        assert!(!desktop.panes().is_empty());
        assert!(desktop.panes().iter().all(|pane| !pane.rect.is_empty()));
        // 같은 것을 다시 읽으면 달라지지 않은 것으로 친다(그사이에 창이 바뀌지 않았다면).
        let version = desktop.version();
        desktop.clear();
        assert_eq!(desktop.version(), version + 1);
        assert!(desktop.panes().is_empty());
    }

    #[test]
    fn tracking_a_window_that_is_not_listed_changes_nothing() {
        let mut desktop = Desktop::default();
        desktop.track(12345);
        assert_eq!((desktop.version(), desktop.panes().len()), (0, 0));
    }
}
