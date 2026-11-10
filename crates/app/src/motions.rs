//! 팩의 동작을 재생하는 데 필요한 값만 추린 표.
//!
//! 동작은 장마다 보여 줄 시간이 다르다(대부분 41~42ms, 어떤 장은 더 길다). 그래서 "몇 번째 장인가"는
//! 장 번호를 세지 않고 동작을 시작한 뒤 흐른 시간으로 정한다.

use dfp_pack::{Pack, PackError};

/// 동작 번호. [`Motions::clip`]으로 재생 정보를 찾는다.
pub type MotionId = u16;

/// 동작 하나의 재생 정보.
#[derive(Clone, Debug)]
pub struct Clip {
    pub name: String,
    /// 팩의 장 표에서 이 동작의 첫 장.
    pub first_frame: usize,
    /// 장마다 시작하는 시각(ms). 길이는 장 수와 같다.
    starts: Vec<f64>,
    /// 한 바퀴의 길이(ms).
    pub total_ms: f64,
    /// 끝나면 처음부터 되풀이한다.
    pub looping: bool,
    /// 끝난 뒤(반복 동작이면 그만둘 때) 이어지는 동작.
    pub next: Option<MotionId>,
    /// 몸을 띄우기 시작하는 시각(ms). 점프에만 있다.
    pub launch_ms: Option<f64>,
    /// 발 자리 보정(1배 px). 그림을 이만큼 옆으로 옮겨 그린다.
    pub foot_shift: f64,
    /// 한 바퀴에 가는 거리(1배 px). 제자리 동작은 0이다.
    pub stride: f64,
}

impl Clip {
    pub fn new(name: &str, first_frame: usize, durations_ms: &[u16], looping: bool) -> Clip {
        let mut starts = Vec::with_capacity(durations_ms.len());
        let mut total = 0.0;
        for &duration in durations_ms {
            starts.push(total);
            total += duration as f64;
        }
        Clip {
            name: name.to_string(),
            first_frame,
            starts,
            total_ms: total,
            looping,
            next: None,
            launch_ms: None,
            foot_shift: 0.0,
            stride: 0.0,
        }
    }

    /// 한 바퀴 안에서의 시각. 반복 동작은 돌아오고, 한 번짜리는 끝에서 멈춘다.
    fn local(&self, t_ms: f64) -> f64 {
        if self.looping { t_ms.rem_euclid(self.total_ms) } else { t_ms.clamp(0.0, self.total_ms) }
    }

    /// 동작을 시작하고 `t_ms`가 지났을 때 보여 줄 장. 동작 안에서의 순서다.
    pub fn frame_at(&self, t_ms: f64) -> usize {
        let local = self.local(t_ms);
        // 시작 시각이 `local`보다 늦지 않은 마지막 장.
        self.starts.partition_point(|&start| start <= local).clamp(1, self.starts.len()) - 1
    }

    /// 다음 장으로 넘어갈 때까지 남은 시간(ms). 한 번짜리 동작의 마지막 장에서는 동작이 끝날 때까지다.
    pub fn until_next_frame(&self, t_ms: f64) -> f64 {
        let local = self.local(t_ms);
        let frame = self.frame_at(t_ms);
        self.starts.get(frame + 1).copied().unwrap_or(self.total_ms) - local
    }

    /// 한 번짜리 동작이 끝났는가.
    pub fn is_over(&self, t_ms: f64) -> bool {
        !self.looping && t_ms >= self.total_ms
    }
}

/// 팩의 모든 동작. 자주 쓰는 동작은 번호를 미리 찾아 둔다.
pub struct Motions {
    clips: Vec<Clip>,
    pub idle: MotionId,
    pub idle2: MotionId,
    pub walk_w: MotionId,
    pub walk_e: MotionId,
    pub hang: MotionId,
    pub hang_to_fall: MotionId,
    pub fall: MotionId,
    pub land: MotionId,
    pub land_to_idle2: MotionId,
    pub spawn: MotionId,
    pub spawn_to_fall: MotionId,
    pub despawn: MotionId,
}

impl Motions {
    /// 팩에서 동작 표를 만든다. 꼭 있어야 하는 동작이 없으면 그 이름을 담아 실패한다.
    pub fn from_pack(pack: &Pack<'_>) -> Result<Motions, PackError> {
        let clips = pack
            .motions()
            .iter()
            .map(|motion| {
                let frames = &pack.frames()[motion.first_frame..motion.first_frame + motion.frame_count];
                let durations: Vec<u16> = frames.iter().map(|frame| frame.duration_ms).collect();
                let mut clip = Clip::new(&motion.name, motion.first_frame, &durations, motion.looping);
                clip.next = motion.next;
                clip.launch_ms = motion.launch_ms.map(f64::from);
                clip.foot_shift = motion.foot_shift as f64;
                clip.stride = motion.stride as f64;
                clip
            })
            .collect();
        Motions::new(clips)
    }

    pub fn new(clips: Vec<Clip>) -> Result<Motions, PackError> {
        let find = |name: &str| {
            let found = clips.iter().position(|clip| clip.name == name && !clip.starts.is_empty());
            found.map(|id| id as MotionId).ok_or_else(|| PackError::NoSuchMotion(name.to_string()))
        };
        Ok(Motions {
            idle: find("idle")?,
            idle2: find("idle2")?,
            walk_w: find("walk_w")?,
            walk_e: find("walk_e")?,
            hang: find("hang")?,
            hang_to_fall: find("hang_to_fall")?,
            fall: find("fall")?,
            land: find("land")?,
            land_to_idle2: find("land_to_idle2")?,
            spawn: find("spawn")?,
            spawn_to_fall: find("spawn_to_fall")?,
            despawn: find("despawn")?,
            clips,
        })
    }

    pub fn clip(&self, id: MotionId) -> &Clip {
        &self.clips[id as usize]
    }

    /// 이름으로 동작 번호를 찾는다.
    pub fn find(&self, name: &str) -> Option<MotionId> {
        self.clips.iter().position(|clip| clip.name == name).map(|id| id as MotionId)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// 시험용 동작 표. 이름과 다음 동작은 진짜 팩과 같고, 길이는 계산하기 쉬운 값이다.
    pub fn sample() -> Motions {
        let spec: &[(&str, &[u16], bool, Option<&str>)] = &[
            ("idle", &[250, 250], true, None),
            ("idle2", &[100, 100, 100, 100], true, None),
            ("walk_w", &[250, 250, 250, 250], true, None),
            ("walk_e", &[250, 250, 250, 250], true, None),
            ("hang", &[100, 100], true, Some("hang_to_fall")),
            ("hang_to_fall", &[200, 200, 200], false, Some("fall")),
            ("fall", &[100, 100], true, Some("land")),
            ("land", &[100, 300], false, Some("land_to_idle2")),
            ("land_to_idle2", &[100, 100], false, Some("idle2")),
            ("spawn", &[500, 500], false, Some("spawn_to_fall")),
            ("spawn_to_fall", &[250, 250], false, Some("fall")),
            ("despawn", &[1000, 1000], false, None),
            ("stretch", &[300, 300], false, Some("idle2")),
            ("edge_in", &[100, 100], false, Some("edge_loop")),
            ("edge_loop", &[100, 100], true, Some("edge_out")),
            ("edge_out", &[100, 100], false, Some("idle2")),
        ];
        let mut clips: Vec<Clip> = Vec::new();
        let mut first = 0;
        for (name, durations, looping, _) in spec {
            clips.push(Clip::new(name, first, durations, *looping));
            first += durations.len();
        }
        for (i, (_, _, _, next)) in spec.iter().enumerate() {
            clips[i].next = next.map(|name| spec.iter().position(|s| s.0 == name).unwrap() as MotionId);
        }
        for clip in &mut clips {
            match clip.name.as_str() {
                "walk_w" => (clip.foot_shift, clip.stride) = (5.0, 40.0),
                "walk_e" => (clip.foot_shift, clip.stride) = (-4.0, 40.0),
                _ => {}
            }
        }
        Motions::new(clips).unwrap()
    }

    #[test]
    fn frame_follows_time_not_count() {
        // 장마다 시간이 다르다: 0~100ms, 100~400ms, 400~450ms
        let clip = Clip::new("x", 0, &[100, 300, 50], false);
        assert_eq!(clip.total_ms, 450.0);
        assert_eq!([0.0, 99.9, 100.0, 399.9, 400.0, 449.0].map(|t| clip.frame_at(t)), [0, 0, 1, 1, 2, 2]);
    }

    #[test]
    fn one_shot_stops_on_the_last_frame() {
        let clip = Clip::new("x", 0, &[100, 300, 50], false);
        assert_eq!(clip.frame_at(450.0), 2);
        assert_eq!(clip.frame_at(9999.0), 2);
        assert!(!clip.is_over(449.9) && clip.is_over(450.0));
    }

    #[test]
    fn loop_comes_around() {
        let clip = Clip::new("x", 0, &[100, 300, 50], true);
        assert_eq!([450.0, 549.0, 550.0, 900.0 + 420.0].map(|t| clip.frame_at(t)), [0, 0, 1, 2]);
        assert!(!clip.is_over(1e9));
    }

    #[test]
    fn time_to_the_next_frame() {
        let clip = Clip::new("x", 0, &[100, 300, 50], true);
        assert_eq!(clip.until_next_frame(0.0), 100.0);
        assert_eq!(clip.until_next_frame(60.0), 40.0);
        assert_eq!(clip.until_next_frame(100.0), 300.0);
        // 마지막 장에서는 한 바퀴가 끝날 때까지.
        assert_eq!(clip.until_next_frame(420.0), 30.0);
        assert_eq!(clip.until_next_frame(450.0 + 60.0), 40.0);
    }

    #[test]
    fn missing_motion_is_named() {
        let clips = vec![Clip::new("idle", 0, &[100], true)];
        assert_eq!(Motions::new(clips).err(), Some(PackError::NoSuchMotion("idle2".to_string())));
    }

    #[test]
    fn sample_table_resolves_names_and_next() {
        let motions = sample();
        assert_eq!(motions.clip(motions.land).next, Some(motions.land_to_idle2));
        assert_eq!(motions.clip(motions.despawn).next, None);
        assert_eq!(motions.find("stretch").map(|id| motions.clip(id).name.as_str()), Some("stretch"));
        assert_eq!(motions.find("nothing"), None);
    }
}
