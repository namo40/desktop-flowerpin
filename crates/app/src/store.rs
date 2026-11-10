//! 설정과 상태를 파일로 남긴다.
//!
//! `%APPDATA%\DesktopFlowerpin\` 아래에 파일을 둔다.
//! - `settings.json`: 사용자가 고른 설정([`Settings`]).
//! - `state.json`: 앱이 기억하는 것([`State`]). 꽃핀이 마지막으로 있던 자리 같은 것이다.
//!
//! 둘을 나눈 까닭은, "기본값으로"가 상태를 건드리지 않게 하고, 자주 바뀌는 상태를 쓰다가 설정 파일이
//! 깨지는 일을 막으려는 것이다.
//!
//! 저장은 바뀐 뒤 잠깐 모았다가 한 번에 한다. 임시 파일에 다 쓴 뒤 이름을 바꿔치기해서, 쓰는 도중에
//! 꺼져도 파일이 깨지지 않는다. 읽지 못하는 파일은 `settings.bad.json`처럼 이름을 바꿔 남겨 두고
//! 기본값으로 시작한다.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::settings::Settings;

const FOLDER: &str = "DesktopFlowerpin";
const SETTINGS_FILE: &str = "settings.json";
const STATE_FILE: &str = "state.json";
const STATE_VERSION: i64 = 1;

/// 바뀐 뒤 이만큼 모았다가 저장한다. 설정 창에서 여러 항목을 잇달아 바꿔도 한 번만 쓴다.
const SAVE_DELAY: Duration = Duration::from_millis(500);

/// 앱이 기억하는 것.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    /// 기본 꽃핀이 마지막으로 있던 자리(발바닥, 화면 좌표). 다시 켜면 이 자리 위에 나타난다.
    pub character: Option<(i32, i32)>,
    /// 설정 창에서 마지막으로 보던 탭.
    pub settings_tab: usize,
    /// 마지막으로 안내한 방송의 시작 시각. 앱을 다시 켜도 같은 방송을 또 안내하지 않는 데 쓴다.
    pub announced_live: Option<String>,
}

impl State {
    pub fn to_json(&self) -> String {
        let mut text = format!("{{\n  \"version\": {STATE_VERSION}");
        if let Some((x, y)) = self.character {
            text.push_str(&format!(",\n  \"character\": {{ \"x\": {x}, \"y\": {y} }}"));
        }
        text.push_str(&format!(",\n  \"settings_tab\": {}", self.settings_tab));
        if let Some(opened) = &self.announced_live {
            // 응답에 적혀 있던 글이다. 따옴표 같은 것이 섞여 있어도 파일이 깨지지 않게 적는다.
            text.push_str(&format!(",\n  \"announced_live\": {}", Value::String(opened.clone())));
        }
        text.push_str("\n}\n");
        text
    }

    /// 파일의 글을 읽는다. JSON이 아니면 `None`이다. 빠졌거나 이상한 값은 기본값으로 둔다.
    pub fn from_json(text: &str) -> Option<State> {
        let value: Value = serde_json::from_str(text).ok()?;
        let object = value.as_object()?;
        let coordinate = |name: &str| object.get("character")?.get(name)?.as_i64().and_then(|v| i32::try_from(v).ok());
        Some(State {
            character: coordinate("x").zip(coordinate("y")),
            settings_tab: object.get("settings_tab").and_then(Value::as_u64).map_or(0, |tab| tab as usize),
            announced_live: object.get("announced_live").and_then(Value::as_str).map(str::to_string),
        })
    }
}

/// 파일을 읽은 결과.
#[derive(Debug, PartialEq, Eq)]
enum Loaded<T> {
    /// 파일이 없다. 처음 켰을 때다.
    Missing,
    Ok(T),
    /// 파일이 있지만 읽을 수 없다.
    Broken,
}

fn load<T>(path: &Path, parse: impl Fn(&str) -> Option<T>) -> Loaded<T> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text).map_or(Loaded::Broken, Loaded::Ok),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Loaded::Missing,
        // 읽을 수 없는 글(UTF-8이 아님 등)도 깨진 것으로 본다.
        Err(_) => Loaded::Broken,
    }
}

/// 깨진 파일을 `이름.bad.json`으로 바꿔 남긴다. 사용자가 고친 값을 되살려 볼 수 있게 지우지 않는다.
fn set_aside(path: &Path) {
    let _ = std::fs::rename(path, path.with_extension("bad.json"));
}

/// 임시 파일에 다 쓴 뒤 이름을 바꿔치기한다.
fn write(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, text)?;
    std::fs::rename(&temporary, path)
}

/// 설정과 상태를 두는 기본 폴더. `%APPDATA%`를 알 수 없으면 `None`이다.
pub fn default_folder() -> Option<PathBuf> {
    std::env::var_os("APPDATA").filter(|value| !value.is_empty()).map(|base| PathBuf::from(base).join(FOLDER))
}

pub struct Store {
    /// 파일을 두는 폴더. 없으면 아무것도 읽거나 쓰지 않는다(만드는 동안 확인용 실행).
    folder: Option<PathBuf>,
    /// 설정과 상태를 저장할 시각. 바뀐 것이 없으면 `None`이다.
    settings_due: Option<Instant>,
    state_due: Option<Instant>,
}

impl Store {
    /// 폴더의 파일을 읽는다. 읽으면서 있었던 일(파일이 깨져 있었음 등)을 함께 돌려준다.
    pub fn open(folder: Option<PathBuf>) -> (Store, Settings, State, Vec<String>) {
        let mut notes = Vec::new();
        let mut settings = Settings::default();
        let mut state = State::default();
        if let Some(folder) = &folder {
            let path = folder.join(SETTINGS_FILE);
            match load(&path, Settings::from_json) {
                Loaded::Ok(loaded) => settings = loaded,
                Loaded::Missing => {}
                Loaded::Broken => {
                    set_aside(&path);
                    notes.push(format!("{SETTINGS_FILE}을 읽지 못해 기본값으로 시작합니다"));
                }
            }
            let path = folder.join(STATE_FILE);
            match load(&path, State::from_json) {
                Loaded::Ok(loaded) => state = loaded,
                Loaded::Missing => {}
                Loaded::Broken => {
                    set_aside(&path);
                    notes.push(format!("{STATE_FILE}을 읽지 못해 처음 상태로 시작합니다"));
                }
            }
        }
        (Store { folder, settings_due: None, state_due: None }, settings, state, notes)
    }

    pub fn folder(&self) -> Option<&Path> {
        self.folder.as_deref()
    }

    /// 설정이 바뀌었다고 알린다. 잠깐 뒤에 저장한다.
    pub fn settings_changed(&mut self, now: Instant) {
        self.settings_due.get_or_insert(now + SAVE_DELAY);
    }

    /// 상태가 바뀌었다고 알린다. 잠깐 뒤에 저장한다.
    pub fn state_changed(&mut self, now: Instant) {
        self.state_due.get_or_insert(now + SAVE_DELAY);
    }

    /// 저장하려고 깨어나야 할 시각.
    pub fn due(&self) -> Option<Instant> {
        match (self.settings_due, self.state_due) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// 저장할 때가 된 것을 저장한다. `now`가 `None`이면 미뤄 둔 것을 모두 바로 저장한다(끝낼 때).
    pub fn save(&mut self, now: Option<Instant>, settings: &Settings, state: &State) {
        let ready = |due: Option<Instant>| due.is_some_and(|due| now.is_none_or(|now| now >= due));
        if ready(self.settings_due) {
            self.settings_due = None;
            self.write(SETTINGS_FILE, &settings.to_json());
        }
        if ready(self.state_due) {
            self.state_due = None;
            self.write(STATE_FILE, &state.to_json());
        }
    }

    fn write(&self, name: &str, text: &str) {
        if let Some(folder) = &self.folder {
            // 쓰지 못해도(디스크가 가득 참 등) 앱을 멈추지 않는다. 다음에 바뀔 때 다시 쓴다.
            let _ = write(&folder.join(name), text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Key;

    /// 시험마다 따로 쓰는 빈 폴더.
    fn folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("dfp-store-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        folder
    }

    #[test]
    fn state_reads_back_the_same() {
        let state = State {
            character: Some((-1200, 1392)),
            settings_tab: 3,
            // 따옴표와 역빗금이 섞여 있어도 그대로 돌아온다.
            announced_live: Some(r#"2026-11-09 "16:11:06" \"#.to_string()),
        };
        assert_eq!(State::from_json(&state.to_json()), Some(state));
        let empty = State::default();
        assert_eq!(State::from_json(&empty.to_json()), Some(empty));
    }

    #[test]
    fn odd_state_values_fall_back() {
        // 자리의 한쪽 값만 있거나 숫자가 아니면 자리가 없는 것으로 친다.
        let state = State::from_json(r#"{"character": {"x": 10}, "settings_tab": "two", "later": true}"#).unwrap();
        assert_eq!(state, State::default());
        let state = State::from_json(r#"{"announced_live": 20261109}"#).unwrap();
        assert_eq!(state, State::default());
        assert_eq!(State::from_json("not json"), None);
    }

    #[test]
    fn first_run_starts_with_defaults_and_writes_nothing() {
        let folder = folder("first");
        let (_, settings, state, notes) = Store::open(Some(folder.clone()));
        assert_eq!((settings, state), (Settings::default(), State::default()));
        assert!(notes.is_empty());
        assert!(!folder.exists());
    }

    #[test]
    fn saved_values_come_back_next_time() {
        let folder = folder("saved");
        let (mut store, mut settings, mut state, _) = Store::open(Some(folder.clone()));
        settings.set(Key::Size, 2);
        state.character = Some((300, 1032));
        let now = Instant::now();
        store.settings_changed(now);
        store.state_changed(now);
        store.save(None, &settings, &state);

        let (_, loaded_settings, loaded_state, notes) = Store::open(Some(folder.clone()));
        assert_eq!((loaded_settings, loaded_state), (settings, state));
        assert!(notes.is_empty());
        // 임시 파일은 남지 않는다.
        let names: Vec<String> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().all(|name| !name.ends_with(".tmp")), "{names:?}");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn saving_waits_and_writes_once() {
        let folder = folder("delay");
        let (mut store, settings, state, _) = Store::open(Some(folder.clone()));
        let start = Instant::now();
        assert_eq!(store.due(), None);
        // 잇달아 바뀌어도 저장할 시각은 처음 바뀐 때부터 센다.
        store.settings_changed(start);
        store.settings_changed(start + Duration::from_millis(300));
        assert_eq!(store.due(), Some(start + SAVE_DELAY));
        // 아직 때가 아니면 쓰지 않는다.
        store.save(Some(start + Duration::from_millis(400)), &settings, &state);
        assert!(!folder.join(SETTINGS_FILE).exists());
        store.save(Some(start + SAVE_DELAY), &settings, &state);
        assert!(folder.join(SETTINGS_FILE).exists());
        // 상태는 바뀌지 않았으므로 쓰지 않는다.
        assert!(!folder.join(STATE_FILE).exists());
        assert_eq!(store.due(), None);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn broken_files_are_set_aside_and_defaults_are_used() {
        let folder = folder("broken");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(SETTINGS_FILE), "{ \"size\": 125, ").unwrap();
        std::fs::write(folder.join(STATE_FILE), [0xFFu8, 0xFE, 0x00]).unwrap();
        let (_, settings, state, notes) = Store::open(Some(folder.clone()));
        assert_eq!((settings, state), (Settings::default(), State::default()));
        assert_eq!(notes.len(), 2);
        // 깨진 파일은 지우지 않고 이름을 바꿔 남긴다.
        assert!(folder.join("settings.bad.json").exists() && folder.join("state.bad.json").exists());
        assert!(!folder.join(SETTINGS_FILE).exists() && !folder.join(STATE_FILE).exists());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn without_a_folder_nothing_is_read_or_written() {
        let (mut store, settings, state, notes) = Store::open(None);
        assert!(notes.is_empty() && store.folder().is_none());
        store.settings_changed(Instant::now());
        store.save(None, &settings, &state);
        assert_eq!(store.due(), None);
    }
}
