//! 같이 하기. 옆 꽃핀이 하는 행동을 따라 하고, 여럿이 왼쪽부터 차례로 뛰어 파도를 만든다.
//!
//! 누가 언제 무엇을 할지([`Cue`])만 정한다. 때가 되면 앱이 그 꽃핀에게 시킨다. 그때 그 꽃핀이 할 수 없는
//! 상태면(떠 있거나, 잡혔거나, 다른 행동을 하고 있으면) 하지 않고 넘어간다.

use std::time::{Duration, Instant};

use crate::actor::{Rng, Show, Whim};

/// 꽃핀이 혼자 기지개·하품·점프를 시작하면, 이 확률(보통 빈도 기준)로 가장 가까운 꽃핀 하나가 따라 한다.
const FOLLOW_CHANCE: f64 = 0.30;
/// 가로로 이만큼(그림 1배 기준 px) 안에 있는 꽃핀만 따라 한다.
const FOLLOW_REACH: f64 = 260.0;
/// 보고 따라 하기까지 걸리는 시간(초).
const FOLLOW_DELAY: (f64, f64) = (0.3, 0.8);
/// 파도 점프에서 옆 꽃핀이 뛰기까지의 사이(초).
const WAVE_GAP: f64 = 0.15;
/// 저절로 파도를 타려면 한 모니터에 쉬거나 걷는 꽃핀이 이만큼 있어야 한다.
pub const WAVE_CROWD: usize = 5;
/// 저절로 파도를 타는 사이(초, 보통 빈도 기준). 빈도가 "자주"면 절반, "적게"면 두 배다.
const WAVE_EVERY: (f64, f64) = (240.0, 480.0);
/// 다 같이 기지개나 하품을 할 때, 저마다 조금씩 늦게 시작하는 시간(초). 똑같이 움직이면 어색하다.
const TOGETHER_SPREAD: (f64, f64) = (0.0, 0.35);

/// 정한 때에 한 꽃핀에게 시킬 행동.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cue {
    pub at: Instant,
    /// 시킬 꽃핀([`crate::character::Character::id`]).
    pub who: isize,
    pub show: Show,
}

/// 같이 하기를 따져 볼 꽃핀 한 명.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Member {
    pub who: isize,
    /// 발바닥 가운데의 가로 자리(화면 px).
    pub x: f64,
    /// 있는 모니터. 같은 값이면 같은 모니터다.
    pub monitor: (i32, i32),
    /// 지금 무엇이든 따라 할 수 있는가. 바닥이나 창 위에서 쉬거나 걷고 있고, 머리에 아무도 없을 때다.
    pub free: bool,
    /// 뛸 수 있는가. 다른 꽃핀의 머리 위에서는 뛰지 않는다.
    pub can_jump: bool,
    /// 표시 배율. 거리 상수에 곱한다.
    pub scale: f64,
}

impl Member {
    fn can(&self, show: Show) -> bool {
        self.free && (show != Show::Jump || self.can_jump)
    }
}

/// 혼자 시작한 행동 가운데 옆 꽃핀이 따라 할 수 있는 것.
pub fn followable(whim: Whim) -> Option<Show> {
    match whim {
        Whim::Stretch => Some(Show::Stretch),
        Whim::Yawn => Some(Show::Yawn),
        Whim::Jump => Some(Show::Jump),
        _ => None,
    }
}

/// `members[leader]`가 `show`를 혼자 시작했다. 따라 할 꽃핀이 있으면 그때와 함께 돌려준다.
/// 따라 하는 것은 같은 모니터에서 가로로 가장 가까운 꽃핀 하나다. `rate`는 가끔 하는 행동의 빈도(보통 1)다.
pub fn follow(members: &[Member], leader: usize, show: Show, rate: f64, rng: &mut Rng, now: Instant) -> Option<Cue> {
    let lead = members.get(leader)?;
    if !rng.chance((FOLLOW_CHANCE * rate).min(1.0)) {
        return None;
    }
    let reach = FOLLOW_REACH * lead.scale;
    let near = members
        .iter()
        .enumerate()
        .filter(|&(index, member)| index != leader && member.monitor == lead.monitor && member.can(show))
        .map(|(_, member)| (member, (member.x - lead.x).abs()))
        .filter(|&(_, distance)| distance <= reach);
    let (member, _) = near.min_by(|a, b| a.1.total_cmp(&b.1))?;
    Some(Cue { at: now + Duration::from_secs_f64(rng.range(FOLLOW_DELAY)), who: member.who, show })
}

/// 파도 점프. 뛸 수 있는 꽃핀들이 왼쪽부터 차례로 뛴다. `monitor`를 주면 그 모니터의 꽃핀만 뛴다.
pub fn wave(members: &[Member], monitor: Option<(i32, i32)>, now: Instant) -> Vec<Cue> {
    let mut jumpers: Vec<&Member> =
        members.iter().filter(|member| member.can(Show::Jump) && monitor.is_none_or(|m| m == member.monitor)).collect();
    jumpers.sort_by(|a, b| a.x.total_cmp(&b.x));
    let gap = |index: usize| Duration::from_secs_f64(index as f64 * WAVE_GAP);
    jumpers
        .iter()
        .enumerate()
        .map(|(index, member)| Cue { at: now + gap(index), who: member.who, show: Show::Jump })
        .collect()
}

/// 다 같이 기지개나 하품을 한다. 저마다 조금씩 늦게 시작한다.
pub fn together(members: &[Member], show: Show, rng: &mut Rng, now: Instant) -> Vec<Cue> {
    let able = members.iter().filter(|member| member.can(show));
    able.map(|member| Cue { at: now + Duration::from_secs_f64(rng.range(TOGETHER_SPREAD)), who: member.who, show })
        .collect()
}

/// 다음에 저절로 파도를 탈 때. `rate`가 0이면(가끔 하는 행동을 하지 않으면) 없다.
pub fn next_wave(rate: f64, rng: &mut Rng, now: Instant) -> Option<Instant> {
    (rate > 0.0).then(|| now + Duration::from_secs_f64(rng.range(WAVE_EVERY) / rate))
}

/// 저절로 파도를 탈 모니터. 쉬거나 걷는 꽃핀이 [`WAVE_CROWD`]명 넘게 있는 모니터 가운데 가장 많은 곳이다.
pub fn crowded(members: &[Member]) -> Option<(i32, i32)> {
    let mut counts: Vec<((i32, i32), usize)> = Vec::new();
    for member in members.iter().filter(|member| member.can(Show::Jump)) {
        match counts.iter_mut().find(|(monitor, _)| *monitor == member.monitor) {
            Some((_, count)) => *count += 1,
            None => counts.push((member.monitor, 1)),
        }
    }
    counts.into_iter().filter(|&(_, count)| count >= WAVE_CROWD).max_by_key(|&(_, count)| count).map(|(m, _)| m)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEFT: (i32, i32) = (-2560, 0);
    const MAIN: (i32, i32) = (0, 0);

    fn member(who: isize, x: f64, monitor: (i32, i32)) -> Member {
        Member { who, x, monitor, free: true, can_jump: true, scale: 1.0 }
    }

    #[test]
    fn the_nearest_free_neighbour_on_the_same_monitor_follows() {
        let now = Instant::now();
        let members = [
            member(1, 1000.0, MAIN),
            member(2, 1200.0, MAIN),
            member(3, 1100.0, MAIN),
            // 더 가깝지만 다른 모니터에 있다.
            member(4, 1010.0, LEFT),
            // 가장 가깝지만 지금 따라 할 수 없다.
            Member { free: false, ..member(5, 1050.0, MAIN) },
        ];
        let mut rng = Rng::new(1);
        // 빈도를 크게 주면 늘 따라 한다.
        let cue = follow(&members, 0, Show::Yawn, 10.0, &mut rng, now).unwrap();
        assert_eq!((cue.who, cue.show), (3, Show::Yawn));
        let wait = cue.at - now;
        assert!(wait >= Duration::from_millis(300) && wait <= Duration::from_millis(800), "{wait:?}");
        // 너무 멀면 따라 하지 않는다.
        let far = [member(1, 1000.0, MAIN), member(2, 1300.0, MAIN)];
        assert_eq!(follow(&far, 0, Show::Yawn, 10.0, &mut rng, now), None);
        // 머리 위에 있는 꽃핀은 점프를 따라 하지 않는다.
        let on_head = [member(1, 1000.0, MAIN), Member { can_jump: false, ..member(2, 1100.0, MAIN) }];
        assert_eq!(follow(&on_head, 0, Show::Jump, 10.0, &mut rng, now), None);
        assert!(follow(&on_head, 0, Show::Stretch, 10.0, &mut rng, now).is_some());
    }

    #[test]
    fn follows_about_three_times_in_ten() {
        let now = Instant::now();
        let members = [member(1, 1000.0, MAIN), member(2, 1100.0, MAIN)];
        let mut rng = Rng::new(7);
        let count = (0..1000).filter(|_| follow(&members, 0, Show::Stretch, 1.0, &mut rng, now).is_some()).count();
        assert!(count.abs_diff(300) < 50, "{count}");
        // 가끔 하는 행동을 하지 않게 했으면 따라 하지도 않는다.
        assert!((0..100).all(|_| follow(&members, 0, Show::Stretch, 0.0, &mut rng, now).is_none()));
    }

    #[test]
    fn a_wave_goes_from_left_to_right() {
        let now = Instant::now();
        let members = [
            member(1, 900.0, MAIN),
            member(2, 300.0, MAIN),
            Member { can_jump: false, ..member(3, 500.0, MAIN) },
            member(4, 600.0, MAIN),
            member(5, -1000.0, LEFT),
        ];
        let cues = wave(&members, Some(MAIN), now);
        let order: Vec<(isize, u64)> = cues.iter().map(|cue| (cue.who, (cue.at - now).as_millis() as u64)).collect();
        assert_eq!(order, [(2, 0), (4, 150), (1, 300)]);
        // 모니터를 정하지 않으면 모니터를 건너 왼쪽부터 간다.
        let all: Vec<isize> = wave(&members, None, now).iter().map(|cue| cue.who).collect();
        assert_eq!(all, [5, 2, 4, 1]);
    }

    #[test]
    fn a_crowded_monitor_rides_the_wave_by_itself() {
        let few: Vec<Member> = (0..4).map(|i| member(i, i as f64 * 100.0, MAIN)).collect();
        assert_eq!(crowded(&few), None);
        let mut many: Vec<Member> = (0..6).map(|i| member(i, i as f64 * 100.0, LEFT)).collect();
        many.extend(few);
        assert_eq!(crowded(&many), Some(LEFT));
        let mut rng = Rng::new(3);
        let now = Instant::now();
        let next = next_wave(1.0, &mut rng, now).unwrap() - now;
        assert!(next >= Duration::from_secs(240) && next <= Duration::from_secs(480));
        assert!(next_wave(2.0, &mut rng, now).unwrap() - now <= Duration::from_secs(240));
        assert_eq!(next_wave(0.0, &mut rng, now), None);
    }

    #[test]
    fn together_everyone_starts_within_a_moment() {
        let now = Instant::now();
        let members = [member(1, 0.0, MAIN), Member { free: false, ..member(2, 100.0, MAIN) }, member(3, 200.0, LEFT)];
        let cues = together(&members, Show::Stretch, &mut Rng::new(5), now);
        assert_eq!(cues.iter().map(|cue| cue.who).collect::<Vec<_>>(), [1, 3]);
        assert!(cues.iter().all(|cue| cue.at - now <= Duration::from_millis(350)));
    }
}
