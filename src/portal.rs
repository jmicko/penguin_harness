use crate::session;
use crate::types::MouseButton;
use anyhow::{Context, Result, anyhow, bail};
use ashpd::desktop::PersistMode;
use ashpd::desktop::screencast::CursorMode;
use ashpd::desktop::screenshot::Screenshot;
use image::GenericImageView;
use lamco_portal::{PortalConfig, PortalManager, PortalSessionHandle};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::runtime::{Builder, Runtime};
use url::Url;

const KEY_PRESS: bool = true;
const KEY_RELEASE: bool = false;
const BUTTON_PRESS: bool = true;
const BUTTON_RELEASE: bool = false;

pub struct PortalController {
    runtime: Runtime,
    state: Option<PortalState>,
}

struct PortalState {
    manager: PortalManager,
    session: PortalSessionHandle,
    restore_token: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PortalSessionResult {
    pub active: bool,
    pub session_id: String,
    pub streams: Vec<PortalStreamInfo>,
    pub pipewire_fd: i32,
    pub restore_token_available: bool,
}

#[derive(Debug, Serialize)]
pub struct PortalStreamInfo {
    pub node_id: u32,
    pub position: (i32, i32),
    pub size: (u32, u32),
    pub source_type: String,
}

#[derive(Debug, Serialize)]
pub struct PortalScreenshotResult {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub source_uri: String,
}

#[derive(Debug, Serialize)]
pub struct PortalActionResult {
    pub message: String,
}

impl PortalController {
    pub fn new() -> Result<Self> {
        let runtime = Builder::new_multi_thread()
            .enable_all()
            .build()
            .context("create portal async runtime")?;
        Ok(Self {
            runtime,
            state: None,
        })
    }

    pub fn start(&mut self) -> Result<PortalSessionResult> {
        let (manager, session, restore_token) = self.runtime.block_on(async {
            let config = PortalConfig {
                cursor_mode: CursorMode::Embedded,
                persist_mode: PersistMode::DoNot,
                ..PortalConfig::default()
            };
            let manager = PortalManager::new(config).await?;
            let session_name = format!("penguin-harness-{}", session::now_ms());
            let (session, restore_token) = manager.create_session(session_name, None).await?;
            Ok::<_, anyhow::Error>((manager, session, restore_token))
        })?;
        self.state = Some(PortalState {
            manager,
            session,
            restore_token,
        });
        self.status()
    }

    pub fn status(&self) -> Result<PortalSessionResult> {
        let state = self
            .state
            .as_ref()
            .ok_or_else(|| anyhow!("portal session is not active; call portal_start first"))?;
        Ok(session_result(state))
    }

    pub fn screenshot(&self) -> Result<PortalScreenshotResult> {
        let response = self.runtime.block_on(async {
            let request = Screenshot::request()
                .interactive(false)
                .modal(false)
                .send()
                .await?;
            let response = request.response()?;
            Ok::<_, anyhow::Error>(response)
        })?;
        let uri = response.uri().as_str().to_string();
        let source = file_uri_to_path(&uri)?;
        let path = session::timestamped_png_path("portal-screen")?;
        fs::copy(&source, &path).with_context(|| {
            format!(
                "copy portal screenshot {} to {}",
                source.display(),
                path.display()
            )
        })?;
        let image = image::open(&path)
            .with_context(|| format!("read portal screenshot {}", path.display()))?;
        let (width, height) = image.dimensions();
        Ok(PortalScreenshotResult {
            path,
            width,
            height,
            source_uri: uri,
        })
    }

    pub fn click_screen(&self, x: i32, y: i32, button: MouseButton) -> Result<PortalActionResult> {
        self.runtime.block_on(async {
            let state = self.active_state()?;
            let target = stream_target(state, x, y)?;
            state
                .manager
                .remote_desktop()
                .notify_pointer_motion_absolute(
                    state.session.ashpd_session(),
                    target.stream_node_id,
                    target.x,
                    target.y,
                )
                .await?;
            tokio::time::sleep(Duration::from_millis(40)).await;
            let button = evdev_button(button);
            state
                .manager
                .remote_desktop()
                .notify_pointer_button(state.session.ashpd_session(), button, BUTTON_PRESS)
                .await?;
            tokio::time::sleep(Duration::from_millis(60)).await;
            state
                .manager
                .remote_desktop()
                .notify_pointer_button(state.session.ashpd_session(), button, BUTTON_RELEASE)
                .await?;
            Ok::<_, anyhow::Error>(())
        })?;
        Ok(PortalActionResult {
            message: format!("portal clicked screen coordinate {x},{y}"),
        })
    }

    pub fn double_click_screen(
        &self,
        x: i32,
        y: i32,
        button: MouseButton,
    ) -> Result<PortalActionResult> {
        self.click_screen(x, y, button)?;
        std::thread::sleep(Duration::from_millis(120));
        self.click_screen(x, y, button)?;
        Ok(PortalActionResult {
            message: format!("portal double-clicked screen coordinate {x},{y}"),
        })
    }

    pub fn drag_screen(
        &self,
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        button: MouseButton,
    ) -> Result<PortalActionResult> {
        self.runtime.block_on(async {
            let state = self.active_state()?;
            let button = evdev_button(button);
            let start = stream_target(state, x1, y1)?;
            state
                .manager
                .remote_desktop()
                .notify_pointer_motion_absolute(
                    state.session.ashpd_session(),
                    start.stream_node_id,
                    start.x,
                    start.y,
                )
                .await?;
            tokio::time::sleep(Duration::from_millis(50)).await;
            state
                .manager
                .remote_desktop()
                .notify_pointer_button(state.session.ashpd_session(), button, BUTTON_PRESS)
                .await?;
            for step in 1..=12 {
                let t = f64::from(step) / 12.0;
                let x = f64::from(x1) + f64::from(x2 - x1) * t;
                let y = f64::from(y1) + f64::from(y2 - y1) * t;
                let target = stream_target(state, x.round() as i32, y.round() as i32)?;
                state
                    .manager
                    .remote_desktop()
                    .notify_pointer_motion_absolute(
                        state.session.ashpd_session(),
                        target.stream_node_id,
                        target.x,
                        target.y,
                    )
                    .await?;
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            state
                .manager
                .remote_desktop()
                .notify_pointer_button(state.session.ashpd_session(), button, BUTTON_RELEASE)
                .await?;
            Ok::<_, anyhow::Error>(())
        })?;
        Ok(PortalActionResult {
            message: format!("portal dragged screen coordinate {x1},{y1} to {x2},{y2}"),
        })
    }

    pub fn scroll_screen(&self, x: i32, y: i32, amount: i32) -> Result<PortalActionResult> {
        self.runtime.block_on(async {
            let state = self.active_state()?;
            let target = stream_target(state, x, y)?;
            state
                .manager
                .remote_desktop()
                .notify_pointer_motion_absolute(
                    state.session.ashpd_session(),
                    target.stream_node_id,
                    target.x,
                    target.y,
                )
                .await?;
            let dy = -f64::from(amount) * 15.0;
            state
                .manager
                .remote_desktop()
                .notify_pointer_axis(state.session.ashpd_session(), 0.0, dy)
                .await?;
            Ok::<_, anyhow::Error>(())
        })?;
        Ok(PortalActionResult {
            message: format!("portal scrolled {amount} tick(s) at screen coordinate {x},{y}"),
        })
    }

    pub fn type_text(&self, text: &str) -> Result<PortalActionResult> {
        self.runtime.block_on(async {
            let state = self.active_state()?;
            for ch in text.chars() {
                let keysym = match ch {
                    '\n' => keysym_for_key("Enter")?,
                    '\t' => keysym_for_key("Tab")?,
                    _ => char_keysym(ch),
                };
                send_keysym(state, keysym).await?;
                tokio::time::sleep(Duration::from_millis(8)).await;
            }
            Ok::<_, anyhow::Error>(())
        })?;
        Ok(PortalActionResult {
            message: format!("portal typed {} character(s)", text.chars().count()),
        })
    }

    pub fn press_key(&self, key: &str) -> Result<PortalActionResult> {
        self.runtime.block_on(async {
            let state = self.active_state()?;
            if let Some(chord) = key_chord(key)? {
                for modifier in &chord.modifiers {
                    keycode_event(state, *modifier, KEY_PRESS).await?;
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                send_keycode(state, chord.keycode).await?;
                for modifier in chord.modifiers.iter().rev() {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    keycode_event(state, *modifier, KEY_RELEASE).await?;
                }
            } else if let Some(keycode) = keycode_for_key(key) {
                send_keycode(state, keycode).await?;
            } else {
                send_keysym(state, keysym_for_key(key)?).await?;
            }
            Ok::<_, anyhow::Error>(())
        })?;
        Ok(PortalActionResult {
            message: format!("portal pressed {key}"),
        })
    }

    fn active_state(&self) -> Result<&PortalState> {
        self.state
            .as_ref()
            .ok_or_else(|| anyhow!("portal session is not active; call portal_start first"))
    }
}

struct PointerTarget {
    stream_node_id: u32,
    x: f64,
    y: f64,
}

struct KeyChord {
    modifiers: Vec<i32>,
    keycode: i32,
}

fn session_result(state: &PortalState) -> PortalSessionResult {
    PortalSessionResult {
        active: true,
        session_id: state.session.session_id().to_string(),
        streams: state
            .session
            .streams()
            .iter()
            .map(|stream| PortalStreamInfo {
                node_id: stream.node_id,
                position: stream.position,
                size: stream.size,
                source_type: format!("{:?}", stream.source_type),
            })
            .collect(),
        pipewire_fd: state.session.pipewire_fd(),
        restore_token_available: state.restore_token.is_some(),
    }
}

fn stream_target(state: &PortalState, x: i32, y: i32) -> Result<PointerTarget> {
    let stream = state
        .session
        .streams()
        .iter()
        .find(|stream| {
            let left = stream.position.0;
            let top = stream.position.1;
            let right = left.saturating_add(stream.size.0 as i32);
            let bottom = top.saturating_add(stream.size.1 as i32);
            x >= left && x < right && y >= top && y < bottom
        })
        .or_else(|| state.session.streams().first())
        .ok_or_else(|| anyhow!("portal session has no screen streams"))?;

    let local_x = x.saturating_sub(stream.position.0);
    let local_y = y.saturating_sub(stream.position.1);
    let max_x = stream.size.0.saturating_sub(1) as i32;
    let max_y = stream.size.1.saturating_sub(1) as i32;
    Ok(PointerTarget {
        stream_node_id: stream.node_id,
        x: f64::from(local_x.clamp(0, max_x)),
        y: f64::from(local_y.clamp(0, max_y)),
    })
}

async fn send_keysym(state: &PortalState, keysym: i32) -> Result<()> {
    key_event(state, keysym, KEY_PRESS).await?;
    tokio::time::sleep(Duration::from_millis(20)).await;
    key_event(state, keysym, KEY_RELEASE).await
}

async fn send_keycode(state: &PortalState, keycode: i32) -> Result<()> {
    keycode_event(state, keycode, KEY_PRESS).await?;
    tokio::time::sleep(Duration::from_millis(20)).await;
    keycode_event(state, keycode, KEY_RELEASE).await
}

async fn key_event(state: &PortalState, keysym: i32, pressed: bool) -> Result<()> {
    state
        .manager
        .remote_desktop()
        .notify_keyboard_keysym(state.session.ashpd_session(), keysym, pressed)
        .await
        .map_err(Into::into)
}

async fn keycode_event(state: &PortalState, keycode: i32, pressed: bool) -> Result<()> {
    state
        .manager
        .remote_desktop()
        .notify_keyboard_keycode(state.session.ashpd_session(), keycode, pressed)
        .await
        .map_err(Into::into)
}

fn char_keysym(ch: char) -> i32 {
    if (' '..='~').contains(&ch) {
        ch as i32
    } else {
        0x0100_0000_i32.saturating_add(ch as i32)
    }
}

fn keysym_for_key(key: &str) -> Result<i32> {
    let normalized = key.trim().to_ascii_lowercase().replace('_', "-");
    let key = if let Some(rest) = normalized
        .strip_prefix("ctrl-")
        .or_else(|| normalized.strip_prefix("control-"))
    {
        rest
    } else {
        normalized.as_str()
    };
    let keysym = match key {
        "enter" | "return" => 0xff0d,
        "escape" | "esc" => 0xff1b,
        "tab" => 0xff09,
        "backspace" => 0xff08,
        "delete" | "del" => 0xffff,
        "left" => 0xff51,
        "up" => 0xff52,
        "right" => 0xff53,
        "down" => 0xff54,
        "home" => 0xff50,
        "end" => 0xff57,
        "page-up" | "pageup" => 0xff55,
        "page-down" | "pagedown" => 0xff56,
        "control-l" | "ctrl-l" => 0xffe3,
        "control-r" | "ctrl-r" => 0xffe4,
        "shift-l" => 0xffe1,
        "shift-r" => 0xffe2,
        "space" => 0x20,
        "f1" => 0xffbe,
        "f2" => 0xffbf,
        "f3" => 0xffc0,
        "f4" => 0xffc1,
        "f5" => 0xffc2,
        "f6" => 0xffc3,
        "f7" => 0xffc4,
        "f8" => 0xffc5,
        "f9" => 0xffc6,
        "f10" => 0xffc7,
        "f11" => 0xffc8,
        "f12" => 0xffc9,
        single if single.chars().count() == 1 => {
            return Ok(char_keysym(single.chars().next().expect("count checked")));
        }
        _ => bail!("unsupported portal key {key:?}"),
    };
    Ok(keysym)
}

fn key_chord(key: &str) -> Result<Option<KeyChord>> {
    let normalized = key.trim().to_ascii_lowercase().replace('_', "-");
    let parts = normalized
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.len() < 2 {
        return Ok(None);
    }

    let (modifier_parts, key_part) = parts.split_at(parts.len() - 1);
    let mut modifiers = Vec::new();
    for modifier in modifier_parts {
        let Some(keycode) = modifier_keycode(modifier) else {
            return Ok(None);
        };
        modifiers.push(keycode);
    }
    let keycode = keycode_for_key(key_part[0])
        .ok_or_else(|| anyhow!("unsupported portal key chord target {:?}", key_part[0]))?;
    Ok(Some(KeyChord { modifiers, keycode }))
}

fn modifier_keycode(key: &str) -> Option<i32> {
    match key {
        "ctrl" | "control" => Some(29),
        "shift" => Some(42),
        "alt" => Some(56),
        _ => None,
    }
}

fn keycode_for_key(key: &str) -> Option<i32> {
    let normalized = key.trim().to_ascii_lowercase().replace('_', "-");
    match normalized.as_str() {
        "escape" | "esc" => Some(1),
        "1" => Some(2),
        "2" => Some(3),
        "3" => Some(4),
        "4" => Some(5),
        "5" => Some(6),
        "6" => Some(7),
        "7" => Some(8),
        "8" => Some(9),
        "9" => Some(10),
        "0" => Some(11),
        "backspace" => Some(14),
        "tab" => Some(15),
        "q" => Some(16),
        "w" => Some(17),
        "e" => Some(18),
        "r" => Some(19),
        "t" => Some(20),
        "y" => Some(21),
        "u" => Some(22),
        "i" => Some(23),
        "o" => Some(24),
        "p" => Some(25),
        "enter" | "return" => Some(28),
        "a" => Some(30),
        "s" => Some(31),
        "d" => Some(32),
        "f" => Some(33),
        "g" => Some(34),
        "h" => Some(35),
        "j" => Some(36),
        "k" => Some(37),
        "l" => Some(38),
        "z" => Some(44),
        "x" => Some(45),
        "c" => Some(46),
        "v" => Some(47),
        "b" => Some(48),
        "n" => Some(49),
        "m" => Some(50),
        "space" => Some(57),
        "f1" => Some(59),
        "f2" => Some(60),
        "f3" => Some(61),
        "f4" => Some(62),
        "f5" => Some(63),
        "f6" => Some(64),
        "f7" => Some(65),
        "f8" => Some(66),
        "f9" => Some(67),
        "f10" => Some(68),
        "f11" => Some(87),
        "f12" => Some(88),
        "home" => Some(102),
        "up" => Some(103),
        "page-up" | "pageup" => Some(104),
        "left" => Some(105),
        "right" => Some(106),
        "end" => Some(107),
        "down" => Some(108),
        "page-down" | "pagedown" => Some(109),
        "delete" | "del" => Some(111),
        _ => None,
    }
}

fn evdev_button(button: MouseButton) -> i32 {
    match button {
        MouseButton::Left => 0x110,
        MouseButton::Right => 0x111,
        MouseButton::Middle => 0x112,
    }
}

fn file_uri_to_path(uri: &str) -> Result<PathBuf> {
    let url = Url::parse(uri).with_context(|| format!("parse portal screenshot URI {uri}"))?;
    if url.scheme() != "file" {
        bail!(
            "portal screenshot returned unsupported URI scheme {}",
            url.scheme()
        );
    }
    url.to_file_path()
        .map_err(|_| anyhow!("portal screenshot URI is not a local path: {uri}"))
}

#[allow(dead_code)]
fn ensure_png(path: &Path) -> Result<()> {
    image::open(path)
        .with_context(|| format!("read PNG {}", path.display()))
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::{key_chord, keycode_for_key};

    #[test]
    fn portal_keycodes_cover_common_shortcuts() {
        assert_eq!(keycode_for_key("Enter"), Some(28));
        assert_eq!(keycode_for_key("s"), Some(31));
        assert_eq!(keycode_for_key("F5"), Some(63));

        let chord = key_chord("Ctrl-S").unwrap().unwrap();
        assert_eq!(chord.modifiers, vec![29]);
        assert_eq!(chord.keycode, 31);

        let chord = key_chord("Ctrl-Shift-P").unwrap().unwrap();
        assert_eq!(chord.modifiers, vec![29, 42]);
        assert_eq!(chord.keycode, 25);
    }
}
