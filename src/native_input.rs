use crate::types::MouseButton;
use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::mem;
use std::os::fd::{AsRawFd, RawFd};
use std::path::Path;
use std::thread;
use std::time::Duration;

const UINPUT_PATH: &str = "/dev/uinput";

const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const EV_REL: u16 = 0x02;

const SYN_REPORT: u16 = 0x00;

const REL_X: i32 = 0x00;
const REL_Y: i32 = 0x01;
const REL_HWHEEL: i32 = 0x06;
const REL_WHEEL: i32 = 0x08;

const BTN_LEFT: i32 = 0x110;
const BTN_RIGHT: i32 = 0x111;
const BTN_MIDDLE: i32 = 0x112;

const KEY_LEFTCTRL: i32 = 29;
const KEY_LEFTSHIFT: i32 = 42;
const KEY_LEFTALT: i32 = 56;

const BUS_USB: u16 = 0x03;
const UINPUT_MAX_NAME_SIZE: usize = 80;
const EDGE_MOVE: i32 = 32_000;

const IOC_NRBITS: u32 = 8;
const IOC_TYPEBITS: u32 = 8;
const IOC_SIZEBITS: u32 = 14;

const IOC_NRSHIFT: u32 = 0;
const IOC_TYPESHIFT: u32 = IOC_NRSHIFT + IOC_NRBITS;
const IOC_SIZESHIFT: u32 = IOC_TYPESHIFT + IOC_TYPEBITS;
const IOC_DIRSHIFT: u32 = IOC_SIZESHIFT + IOC_SIZEBITS;

const IOC_NONE: u32 = 0;
const IOC_WRITE: u32 = 1;
const UINPUT_IOCTL_BASE: u32 = b'U' as u32;

const UI_DEV_CREATE: libc::c_ulong = ioc(IOC_NONE, UINPUT_IOCTL_BASE, 1, 0);
const UI_DEV_DESTROY: libc::c_ulong = ioc(IOC_NONE, UINPUT_IOCTL_BASE, 2, 0);
const UI_DEV_SETUP: libc::c_ulong = iow::<UinputSetup>(UINPUT_IOCTL_BASE, 3);
const UI_SET_EVBIT: libc::c_ulong = iow::<libc::c_int>(UINPUT_IOCTL_BASE, 100);
const UI_SET_KEYBIT: libc::c_ulong = iow::<libc::c_int>(UINPUT_IOCTL_BASE, 101);
const UI_SET_RELBIT: libc::c_ulong = iow::<libc::c_int>(UINPUT_IOCTL_BASE, 102);

const fn ioc(dir: u32, ty: u32, nr: u32, size: u32) -> libc::c_ulong {
    ((dir << IOC_DIRSHIFT) | (ty << IOC_TYPESHIFT) | (nr << IOC_NRSHIFT) | (size << IOC_SIZESHIFT))
        as libc::c_ulong
}

const fn iow<T>(ty: u32, nr: u32) -> libc::c_ulong {
    ioc(IOC_WRITE, ty, nr, mem::size_of::<T>() as u32)
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct InputId {
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UinputSetup {
    id: InputId,
    name: [u8; UINPUT_MAX_NAME_SIZE],
    ff_effects_max: u32,
}

impl Default for UinputSetup {
    fn default() -> Self {
        Self {
            id: InputId::default(),
            name: [0; UINPUT_MAX_NAME_SIZE],
            ff_effects_max: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct InputEvent {
    time: libc::timeval,
    type_: u16,
    code: u16,
    value: i32,
}

#[derive(Debug, Serialize)]
pub struct NativeInputCheck {
    pub path: String,
    pub exists: bool,
    pub writable: bool,
    pub open_error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct NativeInputResult {
    pub message: String,
}

pub struct NativeInputDevice {
    file: File,
    destroyed: bool,
}

impl NativeInputDevice {
    pub fn create() -> Result<Self> {
        let file = OpenOptions::new()
            .write(true)
            .open(UINPUT_PATH)
            .with_context(|| format!("open {UINPUT_PATH} for native uinput control"))?;
        let mut device = Self {
            file,
            destroyed: false,
        };
        device.configure()?;
        thread::sleep(Duration::from_millis(250));
        Ok(device)
    }

    pub fn click_screen(
        &mut self,
        x: i32,
        y: i32,
        button: MouseButton,
    ) -> Result<NativeInputResult> {
        self.move_to_screen(x, y)?;
        thread::sleep(Duration::from_millis(40));
        self.button(button, true)?;
        thread::sleep(Duration::from_millis(60));
        self.button(button, false)?;
        Ok(NativeInputResult {
            message: format!("native clicked screen coordinate {x},{y}"),
        })
    }

    pub fn double_click_screen(
        &mut self,
        x: i32,
        y: i32,
        button: MouseButton,
    ) -> Result<NativeInputResult> {
        self.click_screen(x, y, button)?;
        thread::sleep(Duration::from_millis(120));
        self.click_screen(x, y, button)?;
        Ok(NativeInputResult {
            message: format!("native double-clicked screen coordinate {x},{y}"),
        })
    }

    pub fn drag_screen(
        &mut self,
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        button: MouseButton,
    ) -> Result<NativeInputResult> {
        self.move_to_screen(x1, y1)?;
        thread::sleep(Duration::from_millis(40));
        self.button(button, true)?;
        for step in 1..=18 {
            let t = f64::from(step) / 18.0;
            let x = f64::from(x1) + f64::from(x2 - x1) * t;
            let y = f64::from(y1) + f64::from(y2 - y1) * t;
            self.move_to_screen(x.round() as i32, y.round() as i32)?;
            thread::sleep(Duration::from_millis(16));
        }
        self.button(button, false)?;
        Ok(NativeInputResult {
            message: format!("native dragged screen coordinate {x1},{y1} to {x2},{y2}"),
        })
    }

    pub fn scroll_screen(&mut self, x: i32, y: i32, amount: i32) -> Result<NativeInputResult> {
        self.move_to_screen(x, y)?;
        self.emit(EV_REL, REL_WHEEL as u16, amount)?;
        self.sync()?;
        Ok(NativeInputResult {
            message: format!("native scrolled {amount} tick(s) at screen coordinate {x},{y}"),
        })
    }

    pub fn type_text(&mut self, text: &str) -> Result<NativeInputResult> {
        for ch in text.chars() {
            self.type_char(ch)
                .with_context(|| format!("type native character {ch:?}"))?;
            thread::sleep(Duration::from_millis(6));
        }
        Ok(NativeInputResult {
            message: format!("native typed {} character(s)", text.chars().count()),
        })
    }

    pub fn press_key(&mut self, key: &str) -> Result<NativeInputResult> {
        if let Some(chord) = key_chord(key)? {
            for modifier in &chord.modifiers {
                self.key(*modifier, true)?;
                thread::sleep(Duration::from_millis(8));
            }
            self.tap_key(chord.keycode)?;
            for modifier in chord.modifiers.iter().rev() {
                thread::sleep(Duration::from_millis(8));
                self.key(*modifier, false)?;
            }
        } else {
            let keycode =
                keycode_for_key(key).ok_or_else(|| anyhow!("unsupported native key {key:?}"))?;
            self.tap_key(keycode)?;
        }
        Ok(NativeInputResult {
            message: format!("native pressed {key}"),
        })
    }

    fn configure(&mut self) -> Result<()> {
        ioctl_int(self.fd(), UI_SET_EVBIT, i32::from(EV_KEY))?;
        ioctl_int(self.fd(), UI_SET_EVBIT, i32::from(EV_REL))?;
        ioctl_int(self.fd(), UI_SET_EVBIT, i32::from(EV_SYN))?;

        for code in supported_keycodes() {
            ioctl_int(self.fd(), UI_SET_KEYBIT, code)?;
        }
        for button in [BTN_LEFT, BTN_RIGHT, BTN_MIDDLE] {
            ioctl_int(self.fd(), UI_SET_KEYBIT, button)?;
        }
        for rel in [REL_X, REL_Y, REL_WHEEL, REL_HWHEEL] {
            ioctl_int(self.fd(), UI_SET_RELBIT, rel)?;
        }

        let mut setup = UinputSetup {
            id: InputId {
                bustype: BUS_USB,
                vendor: 0x1209,
                product: 0x5048,
                version: 1,
            },
            ..UinputSetup::default()
        };
        set_name(&mut setup.name, "Penguin Harness Virtual Input");
        ioctl_ptr(self.fd(), UI_DEV_SETUP, &setup)?;
        ioctl_none(self.fd(), UI_DEV_CREATE)?;
        Ok(())
    }

    fn move_to_screen(&mut self, x: i32, y: i32) -> Result<()> {
        if x < 0 || y < 0 {
            bail!("native absolute screen coordinates must be non-negative");
        }
        self.move_relative(-EDGE_MOVE, -EDGE_MOVE)?;
        thread::sleep(Duration::from_millis(20));
        self.move_relative(x, y)
    }

    fn move_relative(&mut self, dx: i32, dy: i32) -> Result<()> {
        self.emit(EV_REL, REL_X as u16, dx)?;
        self.emit(EV_REL, REL_Y as u16, dy)?;
        self.sync()
    }

    fn button(&mut self, button: MouseButton, pressed: bool) -> Result<()> {
        self.emit(EV_KEY, native_button(button) as u16, pressed_value(pressed))?;
        self.sync()
    }

    fn type_char(&mut self, ch: char) -> Result<()> {
        if ch == '\n' {
            return self.tap_key(KEY_ENTER);
        }
        if ch == '\t' {
            return self.tap_key(KEY_TAB);
        }
        let stroke = char_stroke(ch).ok_or_else(|| {
            anyhow!("native text input currently supports US-layout printable ASCII")
        })?;
        if stroke.shift {
            self.key(KEY_LEFTSHIFT, true)?;
        }
        self.tap_key(stroke.keycode)?;
        if stroke.shift {
            self.key(KEY_LEFTSHIFT, false)?;
        }
        Ok(())
    }

    fn tap_key(&mut self, keycode: i32) -> Result<()> {
        self.key(keycode, true)?;
        thread::sleep(Duration::from_millis(20));
        self.key(keycode, false)
    }

    fn key(&mut self, keycode: i32, pressed: bool) -> Result<()> {
        self.emit(EV_KEY, keycode as u16, pressed_value(pressed))?;
        self.sync()
    }

    fn emit(&mut self, type_: u16, code: u16, value: i32) -> Result<()> {
        let event = InputEvent {
            time: libc::timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
            type_,
            code,
            value,
        };
        let bytes = unsafe {
            std::slice::from_raw_parts(
                (&event as *const InputEvent).cast::<u8>(),
                mem::size_of::<InputEvent>(),
            )
        };
        self.file.write_all(bytes).context("write uinput event")
    }

    fn sync(&mut self) -> Result<()> {
        self.emit(EV_SYN, SYN_REPORT, 0)
    }

    fn fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }

    fn destroy(&mut self) {
        if !self.destroyed {
            let _ = unsafe { libc::ioctl(self.fd(), UI_DEV_DESTROY) };
            self.destroyed = true;
        }
    }
}

impl Drop for NativeInputDevice {
    fn drop(&mut self) {
        self.destroy();
    }
}

#[derive(Clone, Debug)]
struct KeyChord {
    modifiers: Vec<i32>,
    keycode: i32,
}

#[derive(Clone, Copy, Debug)]
struct CharStroke {
    keycode: i32,
    shift: bool,
}

pub fn check() -> NativeInputCheck {
    let path = Path::new(UINPUT_PATH);
    match OpenOptions::new().write(true).open(path) {
        Ok(_) => NativeInputCheck {
            path: UINPUT_PATH.to_string(),
            exists: path.exists(),
            writable: true,
            open_error: None,
        },
        Err(error) => NativeInputCheck {
            path: UINPUT_PATH.to_string(),
            exists: path.exists(),
            writable: false,
            open_error: Some(error.to_string()),
        },
    }
}

pub fn with_device<T>(f: impl FnOnce(&mut NativeInputDevice) -> Result<T>) -> Result<T> {
    let mut device = NativeInputDevice::create()?;
    f(&mut device)
}

fn ioctl_none(fd: RawFd, request: libc::c_ulong) -> Result<()> {
    let result = unsafe { libc::ioctl(fd, request) };
    if result == -1 {
        return Err(std::io::Error::last_os_error()).context("uinput ioctl");
    }
    Ok(())
}

fn ioctl_int(fd: RawFd, request: libc::c_ulong, value: i32) -> Result<()> {
    let result = unsafe { libc::ioctl(fd, request, value) };
    if result == -1 {
        return Err(std::io::Error::last_os_error()).context("uinput ioctl");
    }
    Ok(())
}

fn ioctl_ptr<T>(fd: RawFd, request: libc::c_ulong, value: &T) -> Result<()> {
    let result = unsafe { libc::ioctl(fd, request, value as *const T) };
    if result == -1 {
        return Err(std::io::Error::last_os_error()).context("uinput ioctl");
    }
    Ok(())
}

fn set_name(buffer: &mut [u8; UINPUT_MAX_NAME_SIZE], name: &str) {
    let bytes = name.as_bytes();
    let len = bytes.len().min(UINPUT_MAX_NAME_SIZE - 1);
    buffer[..len].copy_from_slice(&bytes[..len]);
}

fn supported_keycodes() -> Vec<i32> {
    let mut codes = vec![
        KEY_ESC,
        KEY_BACKSPACE,
        KEY_TAB,
        KEY_ENTER,
        KEY_LEFTCTRL,
        KEY_LEFTSHIFT,
        KEY_LEFTALT,
        KEY_SPACE,
        KEY_DELETE,
        KEY_HOME,
        KEY_END,
        KEY_PAGEUP,
        KEY_PAGEDOWN,
        KEY_UP,
        KEY_DOWN,
        KEY_LEFT,
        KEY_RIGHT,
    ];
    codes.extend(2..=11);
    codes.extend([12, 13]);
    codes.extend(16..=25);
    codes.extend([26, 27]);
    codes.extend(30..=38);
    codes.extend([39, 40, 41, 43]);
    codes.extend(44..=50);
    codes.extend(51..=53);
    codes.extend(59..=68);
    codes.extend([87, 88]);
    codes.sort_unstable();
    codes.dedup();
    codes
}

fn native_button(button: MouseButton) -> i32 {
    match button {
        MouseButton::Left => BTN_LEFT,
        MouseButton::Right => BTN_RIGHT,
        MouseButton::Middle => BTN_MIDDLE,
    }
}

fn key_chord(key: &str) -> Result<Option<KeyChord>> {
    let normalized = normalize_key(key);
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
        .ok_or_else(|| anyhow!("unsupported native key chord target {:?}", key_part[0]))?;
    Ok(Some(KeyChord { modifiers, keycode }))
}

fn modifier_keycode(key: &str) -> Option<i32> {
    match key {
        "ctrl" | "control" => Some(KEY_LEFTCTRL),
        "shift" => Some(KEY_LEFTSHIFT),
        "alt" => Some(KEY_LEFTALT),
        _ => None,
    }
}

fn normalize_key(key: &str) -> String {
    key.trim().to_ascii_lowercase().replace('_', "-")
}

const KEY_ESC: i32 = 1;
const KEY_BACKSPACE: i32 = 14;
const KEY_TAB: i32 = 15;
const KEY_ENTER: i32 = 28;
const KEY_SPACE: i32 = 57;
const KEY_HOME: i32 = 102;
const KEY_UP: i32 = 103;
const KEY_PAGEUP: i32 = 104;
const KEY_LEFT: i32 = 105;
const KEY_RIGHT: i32 = 106;
const KEY_END: i32 = 107;
const KEY_DOWN: i32 = 108;
const KEY_PAGEDOWN: i32 = 109;
const KEY_DELETE: i32 = 111;

fn keycode_for_key(key: &str) -> Option<i32> {
    let normalized = normalize_key(key);
    match normalized.as_str() {
        "escape" | "esc" => Some(KEY_ESC),
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
        "backspace" => Some(KEY_BACKSPACE),
        "tab" => Some(KEY_TAB),
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
        "enter" | "return" => Some(KEY_ENTER),
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
        "space" => Some(KEY_SPACE),
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
        "home" => Some(KEY_HOME),
        "up" => Some(KEY_UP),
        "page-up" | "pageup" => Some(KEY_PAGEUP),
        "left" => Some(KEY_LEFT),
        "right" => Some(KEY_RIGHT),
        "end" => Some(KEY_END),
        "down" => Some(KEY_DOWN),
        "page-down" | "pagedown" => Some(KEY_PAGEDOWN),
        "delete" | "del" => Some(KEY_DELETE),
        _ => None,
    }
}

fn char_stroke(ch: char) -> Option<CharStroke> {
    let stroke = match ch {
        'a'..='z' => CharStroke {
            keycode: 30 + qwerty_alpha_offset(ch),
            shift: false,
        },
        'A'..='Z' => CharStroke {
            keycode: 30 + qwerty_alpha_offset(ch.to_ascii_lowercase()),
            shift: true,
        },
        '1'..='9' => CharStroke {
            keycode: (ch as i32 - '1' as i32) + 2,
            shift: false,
        },
        '0' => CharStroke {
            keycode: 11,
            shift: false,
        },
        ' ' => CharStroke {
            keycode: KEY_SPACE,
            shift: false,
        },
        '!' => shifted(2),
        '@' => shifted(3),
        '#' => shifted(4),
        '$' => shifted(5),
        '%' => shifted(6),
        '^' => shifted(7),
        '&' => shifted(8),
        '*' => shifted(9),
        '(' => shifted(10),
        ')' => shifted(11),
        '-' => plain(12),
        '_' => shifted(12),
        '=' => plain(13),
        '+' => shifted(13),
        '[' => plain(26),
        '{' => shifted(26),
        ']' => plain(27),
        '}' => shifted(27),
        '\\' => plain(43),
        '|' => shifted(43),
        ';' => plain(39),
        ':' => shifted(39),
        '\'' => plain(40),
        '"' => shifted(40),
        '`' => plain(41),
        '~' => shifted(41),
        ',' => plain(51),
        '<' => shifted(51),
        '.' => plain(52),
        '>' => shifted(52),
        '/' => plain(53),
        '?' => shifted(53),
        _ => return None,
    };
    Some(stroke)
}

fn qwerty_alpha_offset(ch: char) -> i32 {
    match ch {
        'a' => 0,
        's' => 1,
        'd' => 2,
        'f' => 3,
        'g' => 4,
        'h' => 5,
        'j' => 6,
        'k' => 7,
        'l' => 8,
        'z' => 14,
        'x' => 15,
        'c' => 16,
        'v' => 17,
        'b' => 18,
        'n' => 19,
        'm' => 20,
        'q' => -14,
        'w' => -13,
        'e' => -12,
        'r' => -11,
        't' => -10,
        'y' => -9,
        'u' => -8,
        'i' => -7,
        'o' => -6,
        'p' => -5,
        _ => unreachable!("called with non-alpha char"),
    }
}

fn plain(keycode: i32) -> CharStroke {
    CharStroke {
        keycode,
        shift: false,
    }
}

fn shifted(keycode: i32) -> CharStroke {
    CharStroke {
        keycode,
        shift: true,
    }
}

fn pressed_value(pressed: bool) -> i32 {
    if pressed { 1 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::{char_stroke, key_chord, keycode_for_key};

    #[test]
    fn native_keycodes_are_evdev_codes() {
        assert_eq!(keycode_for_key("Enter"), Some(28));
        assert_eq!(keycode_for_key("s"), Some(31));
        assert_eq!(keycode_for_key("F5"), Some(63));

        let chord = key_chord("Ctrl-S").unwrap().unwrap();
        assert_eq!(chord.modifiers, vec![29]);
        assert_eq!(chord.keycode, 31);
    }

    #[test]
    fn native_text_uses_us_qwerty_key_positions() {
        let a = char_stroke('a').unwrap();
        assert_eq!(a.keycode, 30);
        assert!(!a.shift);

        let uppercase = char_stroke('A').unwrap();
        assert_eq!(uppercase.keycode, 30);
        assert!(uppercase.shift);

        let exclamation = char_stroke('!').unwrap();
        assert_eq!(exclamation.keycode, 2);
        assert!(exclamation.shift);

        let slash = char_stroke('/').unwrap();
        assert_eq!(slash.keycode, 53);
        assert!(!slash.shift);
    }
}
