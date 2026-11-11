//! 치지직에 꽃핀 방송 상태를 묻는다. 이 앱이 인터넷을 쓰는 코드는 여기 하나뿐이다.
//!
//! 묻는 곳은 둘이다. 치지직(방송 상태, 정해 둔 간격마다)과 GitHub(새 버전이 나왔는지, 하루에 한 번).
//! 새 버전은 [`ask_once`]로 묻고, 무엇을 묻고 답을 어떻게 읽는지는 [`crate::update`]에 있다. 아래는 방송 상태를
//! 묻는 쪽의 이야기다.
//!
//! 묻는 일은 따로 둔 스레드가 한다. 연결이 느리거나 끊겨도 꽃핀이 멈추지 않게 하려는 것이다.
//! 이 스레드는 시킬 때만 한 번 묻고 다시 잔다. 언제 물을지는 메인 루프가 정한다.
//! 방송 상태 확인을 꺼 두면 이 스레드를 만들지 않는다.
//!
//! 묻는 주소는 치지직이 공식으로 공개한 API가 아니라 치지직 웹이 쓰는 주소다. 치지직의 허락을 받은 것이
//! 아니다. 그래서 다음을 지킨다.
//! - 어떤 프로그램인지 이름을 밝힌다(User-Agent).
//! - 로그인 정보와 쿠키를 보내지 않고, 받은 쿠키도 버린다.
//! - 정해 둔 주소 하나만, 사용자가 고른 간격(가장 짧아야 1분)으로 묻는다.
//! - 다른 주소로 넘겨도 따라가지 않는다.
//! - 막히거나 응답이 달라지면 돌아가는 길을 찾지 않고 멈춘다(판단은 [`crate::live`]가 한다).

use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Networking::WinHttp::{
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_DISABLE_AUTHENTICATION, WINHTTP_DISABLE_COOKIES,
    WINHTTP_DISABLE_REDIRECTS, WINHTTP_FLAG_SECURE, WINHTTP_OPTION_DISABLE_FEATURE, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest,
    WinHttpQueryDataAvailable, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
    WinHttpSetOption, WinHttpSetTimeouts,
};
use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_LOWEST};
use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_NULL};

use crate::live::{self, Reply};
use crate::win::wide;

/// 기다리는 한계(ms). 주소 찾기와 연결은 5초, 보내기와 받기는 10초다.
const CONNECT_TIMEOUT: i32 = 5_000;
const TRANSFER_TIMEOUT: i32 = 10_000;
/// 응답 본문은 방송 상태가 1.5KB쯤, 릴리스 정보가 몇 KB다. 이보다 훨씬 크면 다른 것이 온 것이므로 읽다가 그만둔다.
const BODY_LIMIT: usize = 64 * 1024;

/// 방송 상태를 묻는 스레드와 이어진 끈. 버리면 스레드도 끝난다.
pub struct Checker {
    orders: Sender<()>,
    replies: Receiver<Reply>,
}

impl Checker {
    /// 묻는 스레드를 만든다. 답이 오면 `wake` 창에 빈 메시지를 보내 메인 루프를 깨운다.
    pub fn start(wake: HWND) -> Option<Checker> {
        let (orders, ordered) = mpsc::channel::<()>();
        let (answers, replies) = mpsc::channel::<Reply>();
        // 창 핸들은 숫자로 넘긴다. 다른 스레드에서는 메시지를 보내는 데만 쓴다.
        let wake = wake as isize;
        let run = move || {
            // 꽃핀을 그리는 일보다 뒤로 미룬다.
            unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_LOWEST) };
            while ordered.recv().is_ok() {
                let answer = fetch(live::HOST, &live::path());
                let reply = answer.map_or(Reply::Failed, |(status, body)| live::judge(status, &body));
                if answers.send(reply).is_err() {
                    break;
                }
                unsafe { PostMessageW(wake as HWND, WM_NULL, 0, 0) };
            }
        };
        thread::Builder::new().name("live-check".to_string()).spawn(run).ok()?;
        Some(Checker { orders, replies })
    }

    /// 지금 한 번 물으라고 시킨다. 답은 나중에 [`Checker::take`]로 받는다.
    pub fn ask(&self) {
        let _ = self.orders.send(());
    }

    /// 와 있는 답을 하나 꺼낸다.
    pub fn take(&self) -> Option<Reply> {
        self.replies.try_recv().ok()
    }
}

/// 주소 하나를 한 번만 묻고 끝나는 스레드를 만든다. 새 버전 확인처럼 드물게 묻는 데 쓴다.
///
/// 답은 돌려준 끈으로 온다. HTTP 상태 번호와 응답 본문이고, 연결하지 못했으면 `None`이다. 답이 오면 `wake` 창에
/// 빈 메시지를 보내 메인 루프를 깨운다. 방송 상태를 물을 때와 같이 쿠키와 로그인 정보를 보내지 않고, 다른
/// 주소로 넘겨도 따라가지 않는다.
pub fn ask_once(host: &'static str, path: &'static str, wake: HWND) -> Option<Receiver<Option<(u32, String)>>> {
    let (answers, replies) = mpsc::channel();
    let wake = wake as isize;
    let run = move || {
        unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_LOWEST) };
        let _ = answers.send(fetch(host, path));
        unsafe { PostMessageW(wake as HWND, WM_NULL, 0, 0) };
    };
    thread::Builder::new().name("update-check".to_string()).spawn(run).ok()?;
    Some(replies)
}

/// WinHTTP에서 받은 것. 버릴 때 닫는다.
struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

/// 주소 하나(`https://host/path`)를 한 번 불러 HTTP 상태 번호와 응답 본문을 돌려준다. 연결하지 못했거나 읽지 못했으면
/// `None`이다. 상태 번호가 200이 아니면 본문은 읽지 않고 비워 둔다.
///
/// 시스템의 프록시 설정과 TLS를 그대로 쓴다.
fn fetch(host: &str, path: &str) -> Option<(u32, String)> {
    let agent = wide(&format!("DesktopFlowerpin/{}", env!("CARGO_PKG_VERSION")));
    let (host, verb, path) = (wide(host), wide("GET"), wide(path));
    unsafe {
        let session = Handle(WinHttpOpen(agent.as_ptr(), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, null(), null(), 0));
        if session.0.is_null() {
            return None;
        }
        WinHttpSetTimeouts(session.0, CONNECT_TIMEOUT, CONNECT_TIMEOUT, TRANSFER_TIMEOUT, TRANSFER_TIMEOUT);
        let connection = Handle(WinHttpConnect(session.0, host.as_ptr(), 443, 0));
        if connection.0.is_null() {
            return None;
        }
        let request = Handle(WinHttpOpenRequest(
            connection.0,
            verb.as_ptr(),
            path.as_ptr(),
            null(),
            null(),
            null(),
            WINHTTP_FLAG_SECURE,
        ));
        if request.0.is_null() {
            return None;
        }
        // 쿠키를 주고받지 않고, 로그인 정보를 보내지 않고, 다른 주소로 넘겨도 따라가지 않는다.
        let off: u32 = WINHTTP_DISABLE_COOKIES | WINHTTP_DISABLE_AUTHENTICATION | WINHTTP_DISABLE_REDIRECTS;
        let size = size_of::<u32>() as u32;
        WinHttpSetOption(request.0, WINHTTP_OPTION_DISABLE_FEATURE, &off as *const u32 as *const c_void, size);
        if WinHttpSendRequest(request.0, null(), 0, null(), 0, 0, 0) == 0
            || WinHttpReceiveResponse(request.0, null_mut()) == 0
        {
            return None;
        }
        let (mut status, mut status_size) = (0u32, size_of::<u32>() as u32);
        let status_at = &mut status as *mut u32 as *mut c_void;
        let wanted = WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER;
        if WinHttpQueryHeaders(request.0, wanted, null(), status_at, &mut status_size, null_mut()) == 0 {
            return None;
        }
        if status != 200 {
            return Some((status, String::new()));
        }
        let mut body: Vec<u8> = Vec::new();
        loop {
            let mut waiting = 0u32;
            if WinHttpQueryDataAvailable(request.0, &mut waiting) == 0 {
                return None;
            }
            if waiting == 0 {
                break;
            }
            let (start, mut got) = (body.len(), 0u32);
            if start + waiting as usize > BODY_LIMIT {
                return None;
            }
            body.resize(start + waiting as usize, 0);
            if WinHttpReadData(request.0, body[start..].as_mut_ptr() as *mut c_void, waiting, &mut got) == 0 {
                return None;
            }
            body.truncate(start + got as usize);
        }
        Some((status, String::from_utf8(body).ok()?))
    }
}
