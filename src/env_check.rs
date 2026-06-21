use crate::x11_control;
use serde::Serialize;
use std::env;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Serialize)]
pub struct EnvironmentCheck {
    pub display: Option<String>,
    pub xdg_session_type: Option<String>,
    pub xdg_current_desktop: Option<String>,
    pub wayland_display: Option<String>,
    pub xauthority: Option<String>,
    pub xwayland_auth_candidates: Vec<String>,
    pub x11_connects: bool,
    pub xtest_version: Option<String>,
    pub screen_size: Option<(u16, u16)>,
    pub commands: Vec<CommandCheck>,
    pub isolated_mode_ready: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct CommandCheck {
    pub name: String,
    pub path: Option<String>,
}

pub fn check() -> EnvironmentCheck {
    let display = env::var("DISPLAY").ok().filter(|v| !v.is_empty());
    let xdg_session_type = env::var("XDG_SESSION_TYPE").ok().filter(|v| !v.is_empty());
    let xdg_current_desktop = env::var("XDG_CURRENT_DESKTOP")
        .ok()
        .filter(|v| !v.is_empty());
    let wayland_display = env::var("WAYLAND_DISPLAY").ok().filter(|v| !v.is_empty());
    let xauthority = env::var("XAUTHORITY").ok().filter(|v| !v.is_empty());
    let xwayland_auth_candidates = xwayland_auth_candidates();
    let xtest_version = x11_control::check_xtest().ok();
    let screen_size = x11_control::screen_size().ok();
    let x11_connects = screen_size.is_some();
    let commands = [
        "x-terminal-emulator",
        "ptyxis",
        "mate-terminal",
        "gnome-terminal",
        "xfce4-terminal",
        "xterm",
        "Xvfb",
        "Xephyr",
        "openbox",
        "fluxbox",
        "i3",
    ]
    .into_iter()
    .map(check_command)
    .collect::<Vec<_>>();

    let has_xvfb_or_xephyr = command_path("Xvfb").is_some() || command_path("Xephyr").is_some();
    let has_wm = ["openbox", "fluxbox", "i3", "matchbox-window-manager"]
        .into_iter()
        .any(|cmd| command_path(cmd).is_some());
    let isolated_mode_ready = has_xvfb_or_xephyr && has_wm;

    let mut notes = Vec::new();
    if display.is_none() {
        notes.push("DISPLAY is not set; real desktop mode cannot connect to X11.".to_string());
    }
    if !x11_connects {
        notes.push("Could not connect to the X11 display from this process.".to_string());
    }
    if display.is_some()
        && xauthority.is_none()
        && !xwayland_auth_candidates.is_empty()
        && matches!(xdg_session_type.as_deref(), Some("wayland"))
    {
        notes.push(
            "XAUTHORITY is unset, but GNOME/Mutter Xwayland auth candidates exist; launch Codex from the graphical terminal or set XAUTHORITY for Xwayland control."
                .to_string(),
        );
    }
    if matches!(xdg_session_type.as_deref(), Some("wayland")) {
        notes.push(
            "The current desktop session reports Wayland; this harness requires X11 or an XWayland-compatible path for input synthesis."
                .to_string(),
        );
    }
    if xtest_version.is_none() {
        notes.push("XTEST is unavailable; keyboard and mouse synthesis will fail.".to_string());
    }
    if !isolated_mode_ready {
        notes.push("Isolated mode needs Xvfb or Xephyr plus a window manager.".to_string());
    }

    EnvironmentCheck {
        display,
        xdg_session_type,
        xdg_current_desktop,
        wayland_display,
        xauthority,
        xwayland_auth_candidates,
        x11_connects,
        xtest_version,
        screen_size,
        commands,
        isolated_mode_ready,
        notes,
    }
}

pub fn command_path(name: &str) -> Option<String> {
    let output = Command::new("which").arg(name).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
}

fn check_command(name: &str) -> CommandCheck {
    CommandCheck {
        name: name.to_string(),
        path: command_path(name),
    }
}

fn xwayland_auth_candidates() -> Vec<String> {
    let Some(runtime_dir) = env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(runtime_dir) else {
        return Vec::new();
    };

    let mut candidates = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(".mutter-Xwaylandauth."))
        })
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    candidates.sort();
    candidates
}
