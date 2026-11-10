//! 이 앱이 쓰는 메모리와 CPU를 잰다. 설정 창의 정보 탭과, 만드는 동안의 보고서가 함께 쓴다.

use std::time::Instant;

use windows_sys::Win32::System::ProcessStatus::{
    K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2,
};
use windows_sys::Win32::System::Threading::GetCurrentProcess;
use windows_sys::Win32::System::WindowsProgramming::QueryProcessCycleTime;

/// 메비바이트(1024 × 1024바이트).
pub const MIB: f64 = 1024.0 * 1024.0;

/// 이 프로세스의 개인 작업 집합과 작업 집합(바이트).
///
/// 앞의 것이 작업 관리자의 "메모리(활성 개인 작업 집합)"와 같은 값이다.
pub fn memory() -> (usize, usize) {
    let mut counters: PROCESS_MEMORY_COUNTERS_EX2 = unsafe { std::mem::zeroed() };
    counters.cb = size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32;
    let pointer = &mut counters as *mut PROCESS_MEMORY_COUNTERS_EX2 as *mut PROCESS_MEMORY_COUNTERS;
    unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), pointer, counters.cb) };
    (counters.PrivateWorkingSetSize, counters.WorkingSetSize)
}

/// 이 프로세스의 모든 스레드가 지금까지 쓴 CPU 사이클 수(커널에서 쓴 것도 들어간다).
fn process_cycles() -> u64 {
    let mut cycles = 0;
    unsafe { QueryProcessCycleTime(GetCurrentProcess(), &mut cycles) };
    cycles
}

/// CPU의 사이클 시계. 프로세스가 쓴 사이클 수와 같은 단위로 흐른다.
fn clock_cycles() -> u64 {
    unsafe { std::arch::x86_64::_rdtsc() }
}

/// 만든 때부터 쓴 CPU를 재는 도구.
///
/// CPU는 프로세스가 실제로 쓴 CPU 사이클 수로 잰다. 흔히 쓰는 `GetProcessTimes`는 약 15.6ms마다
/// 그 순간 돌고 있던 프로세스에 시간을 매기는데, 이 앱은 타이머에 맞춰 깨어나 잠깐 일하고 바로
/// 자므로 그 순간에 거의 걸리지 않아 0%로 나온다.
pub struct Meter {
    since: Instant,
    process_cycles: u64,
    clock_cycles: u64,
}

impl Meter {
    pub fn start() -> Meter {
        Meter { since: Instant::now(), process_cycles: process_cycles(), clock_cycles: clock_cycles() }
    }

    /// 재기 시작한 뒤 흐른 시간(초).
    pub fn seconds(&self) -> f64 {
        self.since.elapsed().as_secs_f64()
    }

    /// 재기 시작한 뒤 쓴 CPU. 한 코어를 100으로 본다.
    pub fn cpu_percent(&self) -> f64 {
        // 같은 동안 프로세스가 쓴 사이클 수를 시계가 센 사이클 수로 나누면 한 코어에서 차지한 몫이다.
        let used = process_cycles().saturating_sub(self.process_cycles) as f64;
        let passed = clock_cycles().saturating_sub(self.clock_cycles).max(1) as f64;
        used / passed * 100.0
    }
}
