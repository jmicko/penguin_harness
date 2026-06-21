use crate::types::{Geometry, MouseButton, WindowId};
use crate::window;
use anyhow::{Context, Result, anyhow, bail};
use std::thread;
use std::time::Duration;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    AtomEnum, ConnectionExt as XprotoConnectionExt, InputFocus, KEY_PRESS_EVENT, KEY_RELEASE_EVENT,
};
use x11rb::protocol::xtest::ConnectionExt as XtestConnectionExt;
use x11rb::rust_connection::RustConnection;

const BUTTON_PRESS_EVENT: u8 = 4;
const BUTTON_RELEASE_EVENT: u8 = 5;
const MOTION_NOTIFY_EVENT: u8 = 6;

pub fn check_xtest() -> Result<String> {
    let (conn, _) = connect()?;
    let reply = conn
        .xtest_get_version(2, 2)
        .context("request XTEST version")?
        .reply()
        .context("read XTEST version")?;
    Ok(format!("{}.{}", reply.major_version, reply.minor_version))
}

pub fn screen_size() -> Result<(u16, u16)> {
    let (conn, screen_num) = connect()?;
    let setup = conn.setup();
    let screen = &setup.roots[screen_num];
    Ok((screen.width_in_pixels, screen.height_in_pixels))
}

pub fn focus(window_id: WindowId) -> Result<()> {
    window::ensure_window_exists(window_id)?;
    let _ = window::focus_window(window_id);
    let (conn, _) = connect()?;
    conn.set_input_focus(InputFocus::PARENT, window_id.0, x11rb::CURRENT_TIME)
        .with_context(|| format!("set X input focus to {window_id}"))?
        .check()
        .with_context(|| format!("check X input focus request for {window_id}"))?;
    conn.flush()?;
    thread::sleep(Duration::from_millis(75));
    ensure_active_window(window_id)?;
    Ok(())
}

pub fn type_text(window_id: WindowId, text: &str) -> Result<()> {
    focus(window_id)?;
    type_text_active(text)
}

pub fn type_text_active(text: &str) -> Result<()> {
    let keyboard = KeyboardMap::load()?;
    for ch in text.chars() {
        if ch == '\n' {
            press_key_active("Enter")?;
        } else if ch == '\t' {
            press_key_active("Tab")?;
        } else {
            let keysym = char_keysym(ch).ok_or_else(|| anyhow!("unsupported character {ch:?}"))?;
            keyboard.send_keysym(keysym)?;
        }
    }
    Ok(())
}

pub fn press_key(window_id: WindowId, key: &str) -> Result<()> {
    focus(window_id)?;
    press_key_active(key)
}

pub fn press_key_active(key: &str) -> Result<()> {
    let keyboard = KeyboardMap::load()?;
    let stroke = named_keystroke(key).ok_or_else(|| anyhow!("unsupported key {key:?}"))?;
    keyboard.send_stroke(stroke)?;
    Ok(())
}

pub fn click(window_id: WindowId, x: i32, y: i32, button: MouseButton) -> Result<()> {
    focus(window_id)?;
    let geometry = window::get_geometry(window_id)?;
    let (root_x, root_y) = relative_to_root(geometry, x, y);
    click_screen(root_x as i32, root_y as i32, button)
}

pub fn click_screen(x: i32, y: i32, button: MouseButton) -> Result<()> {
    let (conn, screen_num) = connect()?;
    let root = conn.setup().roots[screen_num].root;
    let root_x = x as i16;
    let root_y = y as i16;
    fake_input(&conn, MOTION_NOTIFY_EVENT, 0, root, root_x, root_y)?;
    fake_input(
        &conn,
        BUTTON_PRESS_EVENT,
        button.xtest_button(),
        root,
        root_x,
        root_y,
    )?;
    fake_input(
        &conn,
        BUTTON_RELEASE_EVENT,
        button.xtest_button(),
        root,
        root_x,
        root_y,
    )?;
    conn.flush()?;
    Ok(())
}

pub fn double_click(window_id: WindowId, x: i32, y: i32, button: MouseButton) -> Result<()> {
    click(window_id, x, y, button)?;
    thread::sleep(Duration::from_millis(90));
    click(window_id, x, y, button)
}

pub fn double_click_screen(x: i32, y: i32, button: MouseButton) -> Result<()> {
    click_screen(x, y, button)?;
    thread::sleep(Duration::from_millis(90));
    click_screen(x, y, button)
}

pub fn drag(
    window_id: WindowId,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    button: MouseButton,
) -> Result<()> {
    focus(window_id)?;
    let geometry = window::get_geometry(window_id)?;
    let (start_x, start_y) = relative_to_root(geometry, x1, y1);
    let (end_x, end_y) = relative_to_root(geometry, x2, y2);
    drag_screen(
        start_x as i32,
        start_y as i32,
        end_x as i32,
        end_y as i32,
        button,
    )
}

pub fn drag_screen(x1: i32, y1: i32, x2: i32, y2: i32, button: MouseButton) -> Result<()> {
    let (conn, screen_num) = connect()?;
    let root = conn.setup().roots[screen_num].root;
    let start_x = x1 as i16;
    let start_y = y1 as i16;
    let end_x = x2 as i16;
    let end_y = y2 as i16;
    fake_input(&conn, MOTION_NOTIFY_EVENT, 0, root, start_x, start_y)?;
    fake_input(
        &conn,
        BUTTON_PRESS_EVENT,
        button.xtest_button(),
        root,
        start_x,
        start_y,
    )?;
    let steps = 12;
    for step in 1..=steps {
        let x = start_x + (((end_x - start_x) as i32 * step) / steps) as i16;
        let y = start_y + (((end_y - start_y) as i32 * step) / steps) as i16;
        fake_input(&conn, MOTION_NOTIFY_EVENT, 0, root, x, y)?;
        conn.flush()?;
        thread::sleep(Duration::from_millis(15));
    }
    fake_input(
        &conn,
        BUTTON_RELEASE_EVENT,
        button.xtest_button(),
        root,
        end_x,
        end_y,
    )?;
    conn.flush()?;
    Ok(())
}

pub fn scroll(window_id: WindowId, x: i32, y: i32, amount: i32) -> Result<()> {
    focus(window_id)?;
    let geometry = window::get_geometry(window_id)?;
    let (root_x, root_y) = relative_to_root(geometry, x, y);
    scroll_screen(root_x as i32, root_y as i32, amount)
}

pub fn scroll_screen(x: i32, y: i32, amount: i32) -> Result<()> {
    let (conn, screen_num) = connect()?;
    let root = conn.setup().roots[screen_num].root;
    let root_x = x as i16;
    let root_y = y as i16;
    fake_input(&conn, MOTION_NOTIFY_EVENT, 0, root, root_x, root_y)?;
    let button = if amount >= 0 { 4 } else { 5 };
    for _ in 0..amount.abs().max(1) {
        fake_input(&conn, BUTTON_PRESS_EVENT, button, root, root_x, root_y)?;
        fake_input(&conn, BUTTON_RELEASE_EVENT, button, root, root_x, root_y)?;
    }
    conn.flush()?;
    Ok(())
}

pub fn active_window() -> Result<Option<WindowId>> {
    let (conn, screen_num) = connect()?;
    let root = conn.setup().roots[screen_num].root;
    let active_atom = conn
        .intern_atom(false, b"_NET_ACTIVE_WINDOW")?
        .reply()
        .context("intern _NET_ACTIVE_WINDOW")?
        .atom;
    let reply = conn
        .get_property(false, root, active_atom, AtomEnum::WINDOW, 0, 1)?
        .reply()
        .context("read _NET_ACTIVE_WINDOW")?;
    Ok(reply
        .value32()
        .and_then(|mut values| values.next())
        .filter(|id| *id != 0)
        .map(WindowId))
}

fn ensure_active_window(window_id: WindowId) -> Result<()> {
    let active = active_window()?;
    if active == Some(window_id) || active.is_none() {
        return Ok(());
    }

    bail!(
        "refusing to send input: active X11 window is {:?}, not target {}",
        active,
        window_id
    )
}

fn connect() -> Result<(RustConnection, usize)> {
    x11rb::connect(None).context("connect to X11 display")
}

fn relative_to_root(geometry: Geometry, x: i32, y: i32) -> (i16, i16) {
    (
        geometry.x.saturating_add(x) as i16,
        geometry.y.saturating_add(y) as i16,
    )
}

fn fake_input(
    conn: &RustConnection,
    type_: u8,
    detail: u8,
    root: u32,
    root_x: i16,
    root_y: i16,
) -> Result<()> {
    conn.xtest_fake_input(type_, detail, x11rb::CURRENT_TIME, root, root_x, root_y, 0)
        .context("send XTEST input event")?
        .check()
        .context("check XTEST input event")
}

struct KeyboardMap {
    conn: RustConnection,
    shift_keycode: u8,
    control_keycode: u8,
    entries: Vec<KeyEntry>,
}

#[derive(Clone, Copy)]
struct KeyEntry {
    keycode: u8,
    keysym: u32,
    needs_shift: bool,
}

impl KeyboardMap {
    fn load() -> Result<Self> {
        let (conn, _) = connect()?;
        let setup = conn.setup();
        let min = setup.min_keycode;
        let max = setup.max_keycode;
        let count = max - min + 1;
        let reply = conn
            .get_keyboard_mapping(min, count)
            .context("get X keyboard mapping")?
            .reply()
            .context("read X keyboard mapping")?;
        let per = reply.keysyms_per_keycode as usize;
        let mut entries = Vec::new();
        for (index, keysym) in reply.keysyms.iter().enumerate() {
            if *keysym == 0 {
                continue;
            }
            let keycode = min + (index / per) as u8;
            let slot = index % per;
            entries.push(KeyEntry {
                keycode,
                keysym: *keysym,
                needs_shift: slot % 2 == 1,
            });
        }
        let shift_keycode = entries
            .iter()
            .find(|entry| entry.keysym == 0xffe1 || entry.keysym == 0xffe2)
            .map(|entry| entry.keycode)
            .ok_or_else(|| anyhow!("could not find Shift keycode in X keyboard mapping"))?;
        let control_keycode = entries
            .iter()
            .find(|entry| entry.keysym == 0xffe3 || entry.keysym == 0xffe4)
            .map(|entry| entry.keycode)
            .ok_or_else(|| anyhow!("could not find Control keycode in X keyboard mapping"))?;
        Ok(Self {
            conn,
            shift_keycode,
            control_keycode,
            entries,
        })
    }

    fn send_stroke(&self, stroke: KeyStroke) -> Result<()> {
        if stroke.ctrl {
            self.fake_key(self.control_keycode, true)?;
        }
        self.send_keysym(stroke.keysym)?;
        if stroke.ctrl {
            self.fake_key(self.control_keycode, false)?;
            self.conn.flush()?;
        }
        Ok(())
    }

    fn send_keysym(&self, keysym: u32) -> Result<()> {
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.keysym == keysym)
            .or_else(|| {
                self.entries
                    .iter()
                    .find(|entry| equivalent_keysym(entry.keysym, keysym))
            })
            .ok_or_else(|| anyhow!("could not map keysym 0x{keysym:x} to a keycode"))?;

        if entry.needs_shift {
            self.fake_key(self.shift_keycode, true)?;
        }
        self.fake_key(entry.keycode, true)?;
        self.fake_key(entry.keycode, false)?;
        if entry.needs_shift {
            self.fake_key(self.shift_keycode, false)?;
        }
        self.conn.flush()?;
        thread::sleep(Duration::from_millis(8));
        Ok(())
    }

    fn fake_key(&self, keycode: u8, press: bool) -> Result<()> {
        self.conn
            .xtest_fake_input(
                if press {
                    KEY_PRESS_EVENT
                } else {
                    KEY_RELEASE_EVENT
                },
                keycode,
                x11rb::CURRENT_TIME,
                0,
                0,
                0,
                0,
            )
            .context("send XTEST key event")?
            .check()
            .context("check XTEST key event")
    }
}

#[derive(Clone, Copy)]
struct KeyStroke {
    keysym: u32,
    ctrl: bool,
}

fn equivalent_keysym(mapped: u32, requested: u32) -> bool {
    mapped.is_ascii_lowercase_key() && requested == mapped.to_ascii_uppercase_key()
}

trait KeysymAscii {
    fn is_ascii_lowercase_key(&self) -> bool;
    fn to_ascii_uppercase_key(self) -> u32;
}

impl KeysymAscii for u32 {
    fn is_ascii_lowercase_key(&self) -> bool {
        (b'a' as u32..=b'z' as u32).contains(self)
    }

    fn to_ascii_uppercase_key(self) -> u32 {
        if self.is_ascii_lowercase_key() {
            self - 32
        } else {
            self
        }
    }
}

fn char_keysym(ch: char) -> Option<u32> {
    if ch.is_ascii_graphic() || ch == ' ' {
        Some(ch as u32)
    } else {
        None
    }
}

fn named_keystroke(key: &str) -> Option<KeyStroke> {
    let normalized = key.trim().to_ascii_lowercase().replace('_', "-");
    let mut ctrl = false;
    let key = if let Some(rest) = normalized
        .strip_prefix("ctrl-")
        .or_else(|| normalized.strip_prefix("control-"))
    {
        ctrl = true;
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
        "space" => 0x20,
        "left" | "arrow-left" => 0xff51,
        "up" | "arrow-up" => 0xff52,
        "right" | "arrow-right" => 0xff53,
        "down" | "arrow-down" => 0xff54,
        "home" => 0xff50,
        "end" => 0xff57,
        "page-up" | "pgup" => 0xff55,
        "page-down" | "pgdn" => 0xff56,
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
        one if one.chars().count() == 1 => one.chars().next()? as u32,
        _ => return None,
    };

    Some(KeyStroke { keysym, ctrl })
}
