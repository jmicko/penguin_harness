use crate::x11_control;
use serde::Serialize;
use std::env;
use std::path::PathBuf;
use std::process::Command;
#[cfg(unix)]
use std::{fs, os::unix::fs::PermissionsExt};

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
    pub portal: PortalCheck,
    pub native: crate::native::NativeCheck,
    pub commands: Vec<CommandCheck>,
    pub isolated_mode_ready: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct CommandCheck {
    pub name: String,
    pub path: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct PortalCheck {
    pub desktop_portal_binary: Option<String>,
    pub dbus_reachable: bool,
    pub remote_desktop: PortalInterfaceCheck,
    pub screen_cast: PortalInterfaceCheck,
    pub screenshot: PortalInterfaceCheck,
    pub input_capture: PortalInterfaceCheck,
    pub error: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct PortalInterfaceCheck {
    pub available: bool,
    pub version: Option<u32>,
    pub available_device_types: Option<u32>,
    pub available_source_types: Option<u32>,
    pub available_cursor_modes: Option<u32>,
    pub supported_capabilities: Option<u32>,
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
    let portal = check_portal();
    let native = crate::native::check();
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
        "gdbus",
        "busctl",
        "pipewire",
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
        if portal.remote_desktop.available && portal.screen_cast.available {
            notes.push(
                "The current desktop session reports Wayland; use portal_start for native Wayland screen/input control."
                    .to_string(),
            );
        } else {
            notes.push(
                "The current desktop session reports Wayland, but RemoteDesktop/ScreenCast portal support was not detected."
                    .to_string(),
            );
        }
    }
    if xtest_version.is_none() {
        notes.push("XTEST is unavailable; keyboard and mouse synthesis will fail.".to_string());
    }
    if !isolated_mode_ready {
        notes.push("Isolated mode needs Xvfb or Xephyr plus a window manager.".to_string());
    }
    if !native.input.writable {
        notes.push(
            "Native unattended input needs write access to /dev/uinput; install the udev rule or run a trusted helper."
                .to_string(),
        );
    }
    if !native
        .screenshot
        .backends
        .iter()
        .any(|backend| backend.available)
    {
        notes.push("No native screenshot backend was detected.".to_string());
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
        portal,
        native,
        commands,
        isolated_mode_ready,
        notes,
    }
}

pub fn command_path(name: &str) -> Option<String> {
    let path_var = env::var_os("PATH")?;
    env::split_paths(&path_var)
        .map(|dir| dir.join(name))
        .find(is_executable)
        .map(|path| path.display().to_string())
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

pub(crate) fn check_portal() -> PortalCheck {
    let mut check = PortalCheck {
        desktop_portal_binary: desktop_portal_binary(),
        ..PortalCheck::default()
    };

    let Some(gdbus) = command_path("gdbus") else {
        check.error = Some("gdbus is not available; cannot inspect xdg-desktop-portal".to_string());
        return check;
    };

    let output = Command::new(gdbus)
        .args([
            "introspect",
            "--session",
            "--dest",
            "org.freedesktop.portal.Desktop",
            "--object-path",
            "/org/freedesktop/portal/desktop",
            "--only-properties",
        ])
        .output();

    let output = match output {
        Ok(output) => output,
        Err(error) => {
            check.error = Some(format!("failed to run gdbus portal introspection: {error}"));
            return check;
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        check.error = Some(if stderr.is_empty() {
            "xdg-desktop-portal introspection failed".to_string()
        } else {
            stderr
        });
        return check;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    check.dbus_reachable = true;
    check.remote_desktop = parse_portal_interface(&text, "org.freedesktop.portal.RemoteDesktop");
    check.screen_cast = parse_portal_interface(&text, "org.freedesktop.portal.ScreenCast");
    check.screenshot = parse_portal_interface(&text, "org.freedesktop.portal.Screenshot");
    check.input_capture = parse_portal_interface(&text, "org.freedesktop.portal.InputCapture");
    check
}

fn parse_portal_interface(text: &str, name: &str) -> PortalInterfaceCheck {
    let marker = format!("interface {name} {{");
    let Some(start) = text.find(&marker) else {
        return PortalInterfaceCheck::default();
    };
    let rest = &text[start + marker.len()..];
    let end = rest
        .find("\n  interface ")
        .or_else(|| rest.find("\n};"))
        .unwrap_or(rest.len());
    let section = &rest[..end];

    PortalInterfaceCheck {
        available: true,
        version: parse_u32_property(section, "version"),
        available_device_types: parse_u32_property(section, "AvailableDeviceTypes"),
        available_source_types: parse_u32_property(section, "AvailableSourceTypes"),
        available_cursor_modes: parse_u32_property(section, "AvailableCursorModes"),
        supported_capabilities: parse_u32_property(section, "SupportedCapabilities"),
    }
}

fn parse_u32_property(section: &str, name: &str) -> Option<u32> {
    section.lines().find_map(|line| {
        let (_, value) = line.split_once(name)?;
        let (_, value) = value.split_once('=')?;
        let digits = value
            .trim_start()
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect::<String>();
        digits.parse().ok()
    })
}

fn is_executable(path: &PathBuf) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
}

fn desktop_portal_binary() -> Option<String> {
    command_path("xdg-desktop-portal").or_else(|| {
        ["/usr/libexec", "/usr/lib"]
            .into_iter()
            .map(|dir| PathBuf::from(dir).join("xdg-desktop-portal"))
            .find(is_executable)
            .map(|path| path.display().to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::parse_portal_interface;

    #[test]
    fn parses_portal_interface_properties() {
        let text = r#"
node /org/freedesktop/portal/desktop {
  interface org.freedesktop.portal.RemoteDesktop {
    properties:
      readonly u version = 2;
      readonly u AvailableDeviceTypes = 7;
  };
  interface org.freedesktop.portal.ScreenCast {
    properties:
      readonly u version = 5;
      readonly u AvailableSourceTypes = 7;
      readonly u AvailableCursorModes = 7;
  };
};
"#;

        let remote = parse_portal_interface(text, "org.freedesktop.portal.RemoteDesktop");
        assert!(remote.available);
        assert_eq!(remote.version, Some(2));
        assert_eq!(remote.available_device_types, Some(7));

        let screencast = parse_portal_interface(text, "org.freedesktop.portal.ScreenCast");
        assert!(screencast.available);
        assert_eq!(screencast.available_source_types, Some(7));
        assert_eq!(screencast.available_cursor_modes, Some(7));

        let missing = parse_portal_interface(text, "org.freedesktop.portal.InputCapture");
        assert!(!missing.available);
    }
}
