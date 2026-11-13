//! 쉬는 시간 알림 판: 꽃핀 옆에 뜨는 작은 판.
//!
//! 포스트잇의 알림 판과 같은 방식으로 그리는 창이다. 지금 상태와 남은 시간을 보여 주고, 집중 시간과 쉬는 시간을
//! 고르고, 시작하고 멈추고 그만둔다. 타이머는 앱이 들고 있다. 여기서는 앱이 알려 준 모습([`FocusModel`])을
//! 그리고, 무엇을 눌렀는지([`FocusAct`])만 알린다. 판을 닫아도 타이머는 돈다.
//!
//! 단추가 아닌 곳(제목 줄, 빈 바탕)을 잡아 끌면 옮겨진다. 한 번 옮기면 앱이 켜져 있는 동안은 다시 열 때 그 자리에
//! 뜬다. 제목 줄의 꽃 핀으로 판을 늘 다른 창 위에 둘지 켜고 끈다. 끄면 다른 창에 가려지고, 판을 누르면 앞으로
//! 나온다.

use super::panel::{Action, FocusModel, Kind, Panel, Surface};
use super::window::{self, Cursor, Event};
use super::{Outcome, Postits};
use crate::geom::Rect;
use crate::win;

/// 쉬는 시간 알림 판에서 누른 것.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusAct {
    /// 집중 시간(`rest`가 아니면)이나 쉬는 시간을 그 분으로 고른다.
    Pick {
        rest: bool,
        minutes: u32,
    },
    /// 집중 시간이나 쉬는 시간을 화살표나 휠로 몇 칸 바꾼다.
    Step {
        rest: bool,
        steps: i32,
    },
    /// 저절로 이어 가기를 켜고 끈다.
    Auto,
    Start,
    /// 잠깐 멈추거나(`true`) 다시 흐르게 한다.
    Pause(bool),
    Stop,
    /// 지금 하는 것을 끝내고 바로 다음으로 넘어간다(지금 쉬기, 바로 집중).
    Skip,
    /// 판을 늘 다른 창 위에 둘지 바꾼다.
    Pin,
}

pub struct FocusPanel {
    panel: Panel,
    at: (i32, i32),
    scale: f32,
    model: FocusModel,
    /// 끌고 있다. 누른 곳의 커서와 판 왼쪽 위의 차이(화면 px)다.
    drag: Option<(i32, i32)>,
}

impl FocusPanel {
    fn size_px(&self) -> (i32, i32) {
        let size = Panel::size(Kind::Focus);
        ((size.0 * self.scale).round() as i32, (size.1 * self.scale).round() as i32)
    }
}

/// 판에서 누른 것을 앱에 알릴 것으로 바꾼다. 닫기는 판이 알아서 한다.
fn act(action: Action) -> Option<FocusAct> {
    Some(match action {
        Action::FocusPick(which, minutes) => FocusAct::Pick { rest: which == 1, minutes },
        Action::FocusStep(which, steps) => FocusAct::Step { rest: which == 1, steps },
        Action::FocusAuto => FocusAct::Auto,
        Action::FocusGo(0) => FocusAct::Start,
        Action::FocusGo(1) => FocusAct::Pause(true),
        Action::FocusGo(2) => FocusAct::Pause(false),
        Action::FocusGo(3) => FocusAct::Stop,
        Action::FocusGo(4) => FocusAct::Skip,
        Action::FocusPin => FocusAct::Pin,
        _ => return None,
    })
}

impl Postits {
    /// 쉬는 시간 알림 판을 꽃핀 옆에 띄운다. `head`는 머리 꼭대기의 가운데(화면 좌표), `half_body`는 몸의
    /// 반폭, `dpi`는 그 모니터의 화면 배율이다. 오른쪽에 띄우고, 자리가 없으면 왼쪽에 띄운다.
    /// 이미 떠 있으면 그 꽃핀 옆으로 옮겨 다시 띄운다. 판을 옮긴 적이 있으면(앱이 켜져 있는 동안) 그 자리에 띄운다.
    pub fn open_focus(&mut self, (head, half_body): ((i32, i32), i32), dpi: u32, model: FocusModel) {
        self.close_focus();
        window::start();
        let scale = dpi.max(96) as f32 / 96.0;
        let blank = Panel::new(std::ptr::null_mut(), Kind::Focus, std::ptr::null_mut());
        let mut panel = FocusPanel { panel: blank, at: (0, 0), scale, model, drag: None };
        let (wide, tall) = panel.size_px();
        let kept = self.focus_spot.filter(|&spot| window::work_area(spot, false).is_some());
        let near = kept.unwrap_or(head);
        let work = window::work_area(near, true).unwrap_or((0, 0, 1920, 1040));
        let (x, y) = match kept {
            Some(spot) => spot,
            None => {
                let gap = (12.0 * scale) as i32;
                let mut x = head.0 + half_body + gap;
                if x + wide > work.2 {
                    x = head.0 - half_body - gap - wide;
                }
                (x, head.1 - (30.0 * scale) as i32)
            }
        };
        let x = x.clamp(work.0, (work.2 - wide).max(work.0));
        let y = y.clamp(work.1, (work.3 - tall).max(work.1));
        panel.at = (x, y);
        panel.panel.hwnd = window::create(panel.at, (wide, tall), model.top);
        let hwnd = panel.panel.hwnd;
        self.focus = Some(panel);
        self.paint_focus();
        window::show(hwnd);
    }

    /// 쉬는 시간 알림 판을 닫는다.
    pub fn close_focus(&mut self) {
        if let Some(panel) = self.focus.take() {
            window::destroy(panel.panel.hwnd);
        }
    }

    /// 쉬는 시간 알림 판이 떠 있는가.
    pub fn has_focus(&self) -> bool {
        self.focus.is_some()
    }

    /// 판이 보여 줄 것이 바뀌었으면 다시 그린다. 항상 위가 바뀌었으면 판이 놓이는 층도 바꾼다.
    pub fn set_focus(&mut self, model: FocusModel) {
        let Some(panel) = &mut self.focus else { return };
        if panel.model != model {
            if panel.model.top != model.top {
                window::set_layer(panel.panel.hwnd, model.top);
            }
            panel.model = model;
            self.paint_focus();
        }
    }

    /// 판을 화면의 `at`(왼쪽 위)으로 옮긴다. 판이 모니터 밖으로 나가지 않게 맞춘다.
    fn move_focus(&mut self, at: (i32, i32)) {
        let Some(panel) = &mut self.focus else { return };
        let (wide, tall) = panel.size_px();
        let work = window::work_area((at.0 + wide / 2, at.1 + 12), true).unwrap_or((0, 0, 1920, 1040));
        let x = at.0.clamp(work.0, (work.2 - wide).max(work.0));
        let y = at.1.clamp(work.1, (work.3 - tall).max(work.1));
        if (x, y) != panel.at {
            panel.at = (x, y);
            window::move_to(panel.panel.hwnd, panel.at);
        }
    }

    /// 판을 공용 버퍼에 그린다. 그린 그림의 크기(px)를 돌려준다.
    fn draw_focus(&mut self) -> Option<(i32, i32)> {
        self.focus.as_ref()?;
        self.ready();
        let (panel, tools) = (self.focus.as_mut()?, self.tools.as_mut()?);
        let size = panel.size_px();
        tools.canvas.fit(size.0, size.1);
        let stride = tools.canvas.width as usize;
        let mut surface = Surface::new(tools.canvas.pixels(), stride, size, panel.scale);
        let model = panel.model;
        panel.panel.paint_focus(&mut surface, &model);
        Some(size)
    }

    fn paint_focus(&mut self) {
        let Some(size) = self.draw_focus() else { return };
        if let (Some(panel), Some(tools)) = (&self.focus, &self.tools) {
            tools.canvas.present(panel.panel.hwnd, panel.at, size);
        }
    }

    /// 판에서 누른 것을 처리한다. 닫기면 닫고, 아니면 앱에 알린다.
    fn focus_act(&mut self, action: Action) {
        if action == Action::Close {
            self.close_focus();
        } else if let Some(act) = act(action) {
            self.outcomes.push(Outcome::Focus(act));
        }
    }

    /// 쉬는 시간 알림 판에서 일어난 일이면 처리하고 `true`를 돌려준다.
    pub(super) fn focus_event(&mut self, event: Event) -> bool {
        let Some(panel) = &mut self.focus else { return false };
        let (hwnd, scale) = (panel.panel.hwnd, panel.scale);
        let local = |x: i32, y: i32| (x as f32 / scale, y as f32 / scale);
        match event {
            // 화살표를 빨리 누르면 두 번째는 두 번 클릭으로 온다. 그것도 한 번 누른 것으로 친다.
            Event::Press { hwnd: on, x, y } | Event::Double { hwnd: on, x, y } if on == hwnd => {
                // 늘 위에 두지 않는 판은 보통 창처럼, 누르면 다른 창들 앞으로 나온다.
                if !panel.model.top && !self.quiet {
                    window::raise(hwnd);
                }
                let (u, v) = local(x, y);
                match panel.panel.hit(u, v) {
                    Some(action) => {
                        window::stop_pressing();
                        self.focus_act(action);
                    }
                    // 단추가 아닌 곳을 누르면 끌기 시작한다. 뗄 때까지 움직임을 받는다.
                    None => {
                        let cursor = win::cursor_position();
                        panel.drag = Some((cursor.0 - panel.at.0, cursor.1 - panel.at.1));
                    }
                }
            }
            Event::Move { hwnd: on, .. } if on == hwnd && panel.drag.is_some() => {
                let (offset, cursor) = (panel.drag.unwrap_or_default(), win::cursor_position());
                self.move_focus((cursor.0 - offset.0, cursor.1 - offset.1));
            }
            Event::Lift { hwnd: on } if on == hwnd && panel.drag.is_some() => {
                panel.drag = None;
                self.focus_spot = Some(panel.at);
            }
            Event::Move { hwnd: on, x, y } if on == hwnd => {
                let (u, v) = local(x, y);
                let hot = panel.panel.hit(u, v);
                window::set_cursor(if hot.is_some() { Cursor::Hand } else { Cursor::Arrow });
                if panel.panel.hot != hot {
                    panel.panel.hot = hot;
                    self.paint_focus();
                }
            }
            Event::Leave { hwnd: on } if on == hwnd => {
                if panel.panel.hot.take().is_some() {
                    self.paint_focus();
                }
            }
            // 시간 칸 위에서 휠을 굴리면 그 값이 바뀐다.
            Event::Wheel { hwnd: on, x, y, up } if on == hwnd => {
                let (u, v) = local(x, y);
                if let Some(action) = panel.panel.wheel(u, v, if up { 1 } else { -1 }) {
                    self.focus_act(action);
                }
            }
            Event::Lift { hwnd: on } | Event::Menu { hwnd: on } if on == hwnd => {}
            _ => return false,
        }
        true
    }

    /// 확인용: 판에서 그 이름의 것을 누른다. 이름은 `close`, `start`, `pause`, `resume`, `stop`, `skip`, `auto`,
    /// `pin`, `focus 분`, `rest 분`, `focus+`, `focus-`, `rest+`, `rest-`다. 판의 그림 위에서 그 자리를 찾아 누른다.
    /// `drag 가로`면 판을 그만큼 오른쪽으로 끌어 옮긴 것으로 친다.
    pub fn focus_script(&mut self, word: &str, minutes: Option<u32>) {
        if word == "drag" {
            let Some(panel) = &self.focus else { return };
            let at = panel.at;
            self.move_focus((at.0 + minutes.unwrap_or(0) as i32, at.1));
            self.focus_spot = self.focus.as_ref().map(|panel| panel.at);
            return;
        }
        let action = match (word, minutes) {
            ("close", _) => Action::Close,
            ("start", _) => Action::FocusGo(0),
            ("pause", _) => Action::FocusGo(1),
            ("resume", _) => Action::FocusGo(2),
            ("stop", _) => Action::FocusGo(3),
            ("skip", _) => Action::FocusGo(4),
            ("auto", _) => Action::FocusAuto,
            ("pin", _) => Action::FocusPin,
            ("focus", Some(minutes)) => Action::FocusPick(0, minutes),
            ("rest", Some(minutes)) => Action::FocusPick(1, minutes),
            ("focus+", _) => Action::FocusStep(0, 1),
            ("focus-", _) => Action::FocusStep(0, -1),
            ("rest+", _) => Action::FocusStep(1, 1),
            ("rest-", _) => Action::FocusStep(1, -1),
            _ => return,
        };
        let Some(panel) = &self.focus else { return };
        let Some(rect) = panel.panel.spot(action) else { return };
        let (hwnd, scale) = (panel.panel.hwnd, panel.scale);
        let x = ((rect.0 + rect.2) / 2.0 * scale).round() as i32;
        let y = ((rect.1 + rect.3) / 2.0 * scale).round() as i32;
        self.focus_event(Event::Press { hwnd, x, y });
    }

    /// 확인용: 판이 어디에 떠 있고 늘 위에 있는지를 적은 한 줄. 떠 있지 않으면 `None`이다.
    pub fn describe_focus(&self) -> Option<String> {
        let panel = self.focus.as_ref()?;
        let (wide, tall) = panel.size_px();
        Some(format!("쉬는 시간 알림 판: 자리 {:?}, 크기 {wide}x{tall}, 항상 위 {}", panel.at, panel.model.top))
    }

    /// 확인용: 판의 그림과 자리. 꽃핀들의 그림 위에 얹어 보는 데 쓴다.
    pub fn focus_shot(&mut self) -> Option<(Rect, Vec<u32>)> {
        let size = self.draw_focus()?;
        let (panel, tools) = (self.focus.as_ref()?, self.tools.as_mut()?);
        let stride = tools.canvas.width as usize;
        let pixels = tools.canvas.pixels();
        let rows = (0..size.1 as usize).flat_map(|y| pixels[y * stride..y * stride + size.0 as usize].iter().copied());
        Some((Rect::new(panel.at.0, panel.at.1, size.0, size.1), rows.collect()))
    }
}
