//! Windows를 시작할 때 이 앱도 함께 켜지게 하는 일.
//!
//! 로그인한 사용자의 레지스트리(`HKEY_CURRENT_USER`)에 실행 파일의 자리를 적어 둔다. 관리자 권한이 필요 없고,
//! 다른 사용자에게는 영향을 주지 않는다. 사용자가 설정에서 켰을 때만 적고, 끄면 지운다.

use std::path::Path;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ, RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW,
    RegDeleteValueW, RegGetValueW, RegSetValueExW,
};

use crate::win::wide;

/// Windows가 로그인할 때 실행할 프로그램을 읽는 곳.
pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// 그곳에 적는 이름.
const NAME: &str = "DesktopFlowerpin";

/// 레지스트리에 적을 글. 자리에 빈칸이 있어도 한 덩어리로 읽히게 따옴표로 감싼다.
pub fn command(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

/// `key`(`HKEY_CURRENT_USER` 아래의 자리)를 연다. 없으면 만든다.
fn open(key: &str) -> Option<HKEY> {
    let mut handle: HKEY = null_mut();
    let access = KEY_QUERY_VALUE | KEY_SET_VALUE;
    let opened = unsafe {
        RegCreateKeyExW(HKEY_CURRENT_USER, wide(key).as_ptr(), 0, null(), 0, access, null(), &mut handle, null_mut())
    };
    (opened == ERROR_SUCCESS).then_some(handle)
}

/// 지금 적혀 있는 글. 적혀 있지 않으면 `None`이다.
pub fn registered(key: &str) -> Option<String> {
    let mut text = [0u16; 1024];
    let mut bytes = size_of_val(&text) as u32;
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            wide(key).as_ptr(),
            wide(NAME).as_ptr(),
            RRF_RT_REG_SZ,
            null_mut(),
            text.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if read != ERROR_SUCCESS {
        return None;
    }
    // 돌려받은 길이에는 끝의 0이 들어 있다.
    let length = (bytes as usize / 2).saturating_sub(1).min(text.len());
    Some(String::from_utf16_lossy(&text[..length]))
}

/// 함께 켜지게 하거나(`exe`가 있을 때) 그러지 않게 한다. 이미 그렇게 되어 있으면 아무것도 쓰지 않는다.
/// 바꿨으면 `true`를 돌려준다.
///
/// 켜 둔 채로 실행 파일을 다른 곳으로 옮겼으면, 다음에 켰을 때 새 자리로 고쳐 적는다.
pub fn set(key: &str, exe: Option<&Path>) -> bool {
    let wanted = exe.map(command);
    if registered(key) == wanted {
        return false;
    }
    let Some(handle) = open(key) else { return false };
    let name = wide(NAME);
    let done = match wanted {
        Some(text) => {
            let text = wide(&text);
            let bytes = (text.len() * 2) as u32;
            unsafe { RegSetValueExW(handle, name.as_ptr(), 0, REG_SZ, text.as_ptr().cast(), bytes) }
        }
        None => unsafe { RegDeleteValueW(handle, name.as_ptr()) },
    };
    unsafe { RegCloseKey(handle) };
    done == ERROR_SUCCESS
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::System::Registry::RegDeleteKeyW;

    use super::*;

    #[test]
    fn the_path_is_quoted() {
        let exe = Path::new(r"C:\Program Files\꽃핀\DesktopFlowerpin.exe");
        assert_eq!(command(exe), "\"C:\\Program Files\\꽃핀\\DesktopFlowerpin.exe\"");
    }

    #[test]
    fn turning_it_on_and_off_writes_and_removes_the_entry() {
        // 진짜 자리는 건드리지 않고, 시험용 자리에 적어 본다.
        let key = format!(r"Software\DesktopFlowerpinTest\Run{}", std::process::id());
        assert_eq!(registered(&key), None);
        let exe = Path::new(r"D:\어딘가 있는\DesktopFlowerpin.exe");
        assert!(set(&key, Some(exe)));
        assert_eq!(registered(&key), Some(command(exe)));
        // 이미 그렇게 되어 있으면 다시 쓰지 않는다.
        assert!(!set(&key, Some(exe)));
        // 실행 파일을 옮겼으면 새 자리로 고쳐 적는다.
        let moved = Path::new(r"E:\DesktopFlowerpin.exe");
        assert!(set(&key, Some(moved)));
        assert_eq!(registered(&key), Some(command(moved)));
        assert!(set(&key, None));
        assert_eq!(registered(&key), None);
        assert!(!set(&key, None));
        // 시험용 자리를 치운다. 윗자리는 다른 시험이 쓰고 있지 않을 때만 지워진다.
        unsafe {
            RegDeleteKeyW(HKEY_CURRENT_USER, wide(&key).as_ptr());
            RegDeleteKeyW(HKEY_CURRENT_USER, wide(r"Software\DesktopFlowerpinTest").as_ptr());
        }
    }
}
