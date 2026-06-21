use crate::env_check;
use crate::session::{self, Session};
use crate::types::{Mode, MouseButton, WindowId, WindowInfo};
use crate::{screenshot as x11_screenshot, window, x11_control};
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::env;
use std::io;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

#[derive(Debug, Deserialize)]
pub struct LaunchRequest {
    pub command: Vec<String>,
    #[serde(default)]
    pub mode: Mode,
    pub title_hint: Option<String>,
    pub wait_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct LaunchResult {
    pub session: Session,
    pub window: Option<WindowInfo>,
}

#[derive(Debug, Serialize)]
pub struct ScreenshotResult {
    pub session_id: Option<String>,
    pub window_id: WindowId,
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Serialize)]
pub struct ScreenScreenshotResult {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Serialize)]
pub struct SimpleResult {
    pub session_id: Option<String>,
    pub window_id: Option<WindowId>,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct TargetRequest {
    pub session_id: Option<String>,
    pub window_id: Option<WindowId>,
}

#[derive(Debug, Deserialize)]
pub struct WindowQuery {
    pub title_contains: Option<String>,
    pub pid: Option<u32>,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct ActiveWindowResult {
    pub window_id: Option<WindowId>,
    pub window: Option<WindowInfo>,
}

pub fn check_environment() -> env_check::EnvironmentCheck {
    env_check::check()
}

pub fn list_windows() -> Result<Vec<WindowInfo>> {
    window::list_windows()
}

pub fn find_windows(query: WindowQuery) -> Result<Vec<WindowInfo>> {
    let title = query
        .title_contains
        .map(|title| title.to_ascii_lowercase())
        .filter(|title| !title.trim().is_empty());
    let mut windows = window::list_windows()?
        .into_iter()
        .filter(|window| {
            title
                .as_ref()
                .is_none_or(|title| window.title.to_ascii_lowercase().contains(title))
        })
        .filter(|window| query.pid.is_none_or(|pid| window.pid == Some(pid)))
        .collect::<Vec<_>>();
    if let Some(limit) = query.limit {
        windows.truncate(limit);
    }
    Ok(windows)
}

pub fn window_info(target: TargetRequest) -> Result<WindowInfo> {
    let (window_id, _) = resolve_target(&target)?;
    window::get_window_info(window_id)
}

pub fn active_window() -> Result<ActiveWindowResult> {
    let window_id = x11_control::active_window()?;
    let window = window_id.and_then(|id| window::get_window_info(id).ok());
    Ok(ActiveWindowResult { window_id, window })
}

pub fn launch_app(request: LaunchRequest) -> Result<LaunchResult> {
    session::ensure_dirs()?;
    if request.command.is_empty() {
        bail!("launch command cannot be empty");
    }
    match request.mode {
        Mode::Real => launch_real(request),
        Mode::Isolated => launch_isolated(request),
    }
}

pub fn screenshot(target: TargetRequest) -> Result<ScreenshotResult> {
    let (window_id, mut loaded_session) = resolve_target(&target)?;
    window::ensure_window_exists(window_id)?;
    let path = session::timestamped_png_path(
        loaded_session
            .as_ref()
            .map(|s| s.id.as_str())
            .unwrap_or("window"),
    )?;
    let geometry = window::get_geometry(window_id)?;
    let width = u16::try_from(geometry.width)
        .with_context(|| format!("window {window_id} is too wide to capture"))?;
    let height = u16::try_from(geometry.height)
        .with_context(|| format!("window {window_id} is too tall to capture"))?;
    let capture = x11_screenshot::capture_window(window_id, width, height, &path)
        .with_context(|| format!("capture screenshot for {window_id}"))?;

    if let Some(session) = loaded_session.as_mut() {
        session.screenshots.push(path.clone());
        session.save()?;
    }

    Ok(ScreenshotResult {
        session_id: loaded_session.map(|s| s.id),
        window_id,
        path,
        width: capture.width,
        height: capture.height,
    })
}

pub fn screenshot_screen() -> Result<ScreenScreenshotResult> {
    let path = session::timestamped_png_path("screen")?;
    let capture =
        x11_screenshot::capture_screen(&path).context("capture root screen screenshot")?;
    Ok(ScreenScreenshotResult {
        path,
        width: capture.width,
        height: capture.height,
    })
}

pub fn focus(target: TargetRequest) -> Result<SimpleResult> {
    let (window_id, session) = resolve_target(&target)?;
    x11_control::focus(window_id)?;
    Ok(SimpleResult {
        session_id: session.map(|s| s.id),
        window_id: Some(window_id),
        message: format!("focused {window_id}"),
    })
}

pub fn close_window(target: TargetRequest) -> Result<SimpleResult> {
    let (window_id, session) = resolve_target(&target)?;
    window::close_window(window_id)?;
    let closed = window::wait_until_gone(window_id, Duration::from_secs(2))?;
    Ok(SimpleResult {
        session_id: session.map(|s| s.id),
        window_id: Some(window_id),
        message: if closed {
            format!("closed {window_id}")
        } else {
            format!("requested close for {window_id}, but it is still present")
        },
    })
}

pub fn type_text(target: TargetRequest, text: &str) -> Result<SimpleResult> {
    let (window_id, session) = resolve_target(&target)?;
    x11_control::type_text(window_id, text)?;
    Ok(SimpleResult {
        session_id: session.map(|s| s.id),
        window_id: Some(window_id),
        message: format!("typed {} character(s)", text.chars().count()),
    })
}

pub fn type_text_active(text: &str) -> Result<SimpleResult> {
    x11_control::type_text_active(text)?;
    Ok(SimpleResult {
        session_id: None,
        window_id: x11_control::active_window()?,
        message: format!(
            "typed {} character(s) into active window",
            text.chars().count()
        ),
    })
}

pub fn press_key(target: TargetRequest, key: &str) -> Result<SimpleResult> {
    let (window_id, session) = resolve_target(&target)?;
    x11_control::press_key(window_id, key)?;
    Ok(SimpleResult {
        session_id: session.map(|s| s.id),
        window_id: Some(window_id),
        message: format!("pressed {key}"),
    })
}

pub fn press_key_active(key: &str) -> Result<SimpleResult> {
    x11_control::press_key_active(key)?;
    Ok(SimpleResult {
        session_id: None,
        window_id: x11_control::active_window()?,
        message: format!("pressed {key} in active window"),
    })
}

pub fn click(target: TargetRequest, x: i32, y: i32, button: MouseButton) -> Result<SimpleResult> {
    let (window_id, session) = resolve_target(&target)?;
    x11_control::click(window_id, x, y, button)?;
    Ok(SimpleResult {
        session_id: session.map(|s| s.id),
        window_id: Some(window_id),
        message: format!("clicked {x},{y} in {window_id}"),
    })
}

pub fn click_screen(x: i32, y: i32, button: MouseButton) -> Result<SimpleResult> {
    x11_control::click_screen(x, y, button)?;
    Ok(SimpleResult {
        session_id: None,
        window_id: x11_control::active_window()?,
        message: format!("clicked screen coordinate {x},{y}"),
    })
}

pub fn double_click(
    target: TargetRequest,
    x: i32,
    y: i32,
    button: MouseButton,
) -> Result<SimpleResult> {
    let (window_id, session) = resolve_target(&target)?;
    x11_control::double_click(window_id, x, y, button)?;
    Ok(SimpleResult {
        session_id: session.map(|s| s.id),
        window_id: Some(window_id),
        message: format!("double-clicked {x},{y} in {window_id}"),
    })
}

pub fn double_click_screen(x: i32, y: i32, button: MouseButton) -> Result<SimpleResult> {
    x11_control::double_click_screen(x, y, button)?;
    Ok(SimpleResult {
        session_id: None,
        window_id: x11_control::active_window()?,
        message: format!("double-clicked screen coordinate {x},{y}"),
    })
}

pub fn drag(
    target: TargetRequest,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    button: MouseButton,
) -> Result<SimpleResult> {
    let (window_id, session) = resolve_target(&target)?;
    x11_control::drag(window_id, x1, y1, x2, y2, button)?;
    Ok(SimpleResult {
        session_id: session.map(|s| s.id),
        window_id: Some(window_id),
        message: format!("dragged {x1},{y1} to {x2},{y2} in {window_id}"),
    })
}

pub fn drag_screen(
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    button: MouseButton,
) -> Result<SimpleResult> {
    x11_control::drag_screen(x1, y1, x2, y2, button)?;
    Ok(SimpleResult {
        session_id: None,
        window_id: x11_control::active_window()?,
        message: format!("dragged screen coordinate {x1},{y1} to {x2},{y2}"),
    })
}

pub fn scroll(target: TargetRequest, x: i32, y: i32, amount: i32) -> Result<SimpleResult> {
    let (window_id, session) = resolve_target(&target)?;
    x11_control::scroll(window_id, x, y, amount)?;
    Ok(SimpleResult {
        session_id: session.map(|s| s.id),
        window_id: Some(window_id),
        message: format!("scrolled {amount} tick(s) at {x},{y} in {window_id}"),
    })
}

pub fn scroll_screen(x: i32, y: i32, amount: i32) -> Result<SimpleResult> {
    x11_control::scroll_screen(x, y, amount)?;
    Ok(SimpleResult {
        session_id: None,
        window_id: x11_control::active_window()?,
        message: format!("scrolled {amount} tick(s) at screen coordinate {x},{y}"),
    })
}

pub fn close_session(session_id: &str) -> Result<SimpleResult> {
    let session = session::load(session_id)?;
    let mut window_closed = false;
    if let Some(window_id) = session.window_id {
        let _ = window::close_window(window_id);
        window_closed = window::wait_until_gone(window_id, Duration::from_secs(2)).unwrap_or(false);
    }
    if !window_closed {
        if let Some(pid) = session.pid {
            let _ = Command::new("kill").arg(pid.to_string()).status();
            if let Some(window_id) = session.window_id {
                window_closed =
                    window::wait_until_gone(window_id, Duration::from_secs(1)).unwrap_or(false);
            }
        }
    }
    if let Some(pid) = session.isolated_wm_pid {
        let _ = Command::new("kill").arg(pid.to_string()).status();
    }
    if let Some(pid) = session.isolated_display_pid {
        let _ = Command::new("kill").arg(pid.to_string()).status();
    }
    session::remove(session_id)?;
    Ok(SimpleResult {
        session_id: Some(session_id.to_string()),
        window_id: session.window_id,
        message: if session.window_id.is_some() && !window_closed {
            "closed session; window may still be present".to_string()
        } else {
            "closed session".to_string()
        },
    })
}

fn launch_real(request: LaunchRequest) -> Result<LaunchResult> {
    let before = window::list_windows().unwrap_or_default();
    let mut command = Command::new(&request.command[0]);
    command.args(&request.command[1..]);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    detach_command(&mut command);
    let child = command
        .spawn()
        .with_context(|| format!("launch {}", request.command.join(" ")))?;
    let pid = child.id();
    let mut session = Session::new(
        Mode::Real,
        env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string()),
        request.command,
        Some(pid),
        request.title_hint.clone(),
    );
    let wait = Duration::from_millis(request.wait_ms.unwrap_or(5_000));
    let found = window::wait_for_window(&before, Some(pid), request.title_hint.as_deref(), wait)?;
    if let Some(window) = &found {
        session.window_id = Some(window.id);
        let _ = x11_control::focus(window.id);
    } else {
        let _ = Command::new("kill").arg(pid.to_string()).status();
        bail!(
            "launched process {pid}, but no matching window appeared before timeout; no session was saved"
        );
    }
    session.save()?;
    Ok(LaunchResult {
        session,
        window: found,
    })
}

fn launch_isolated(request: LaunchRequest) -> Result<LaunchResult> {
    let check = env_check::check();
    if !check.isolated_mode_ready {
        bail!("isolated mode is not ready; install Xvfb or Xephyr plus a window manager");
    }

    let display = find_free_display();
    let display_arg = format!(":{display}");
    let x_server = if env_check::command_path("Xvfb").is_some() {
        let mut command = Command::new("Xvfb");
        command
            .args([&display_arg, "-screen", "0", "1280x800x24"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        detach_command(&mut command);
        command.spawn().context("launch Xvfb")?
    } else {
        let mut command = Command::new("Xephyr");
        command
            .args([&display_arg, "-screen", "1280x800"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        detach_command(&mut command);
        command.spawn().context("launch Xephyr")?
    };
    thread::sleep(Duration::from_millis(500));

    let wm = ["openbox", "fluxbox", "i3", "matchbox-window-manager"]
        .into_iter()
        .find(|cmd| env_check::command_path(cmd).is_some())
        .ok_or_else(|| anyhow!("no window manager found for isolated mode"))?;
    let mut wm_command = Command::new(wm);
    wm_command
        .env("DISPLAY", &display_arg)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    detach_command(&mut wm_command);
    let wm_child = wm_command.spawn().with_context(|| format!("launch {wm}"))?;
    thread::sleep(Duration::from_millis(500));

    let before = window::list_windows().unwrap_or_default();
    let mut command = Command::new(&request.command[0]);
    command.args(&request.command[1..]);
    command.env("DISPLAY", &display_arg);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    detach_command(&mut command);
    let child = command
        .spawn()
        .with_context(|| format!("launch {}", request.command.join(" ")))?;
    let pid = child.id();
    let mut session = Session::new(
        Mode::Isolated,
        display_arg,
        request.command,
        Some(pid),
        request.title_hint.clone(),
    );
    session.isolated_display_pid = Some(x_server.id());
    session.isolated_wm_pid = Some(wm_child.id());
    let wait = Duration::from_millis(request.wait_ms.unwrap_or(5_000));
    let found = window::wait_for_window(&before, Some(pid), request.title_hint.as_deref(), wait)?;
    if let Some(window) = &found {
        session.window_id = Some(window.id);
        let _ = x11_control::focus(window.id);
    } else {
        let _ = Command::new("kill").arg(pid.to_string()).status();
        let _ = Command::new("kill").arg(wm_child.id().to_string()).status();
        let _ = Command::new("kill").arg(x_server.id().to_string()).status();
        bail!(
            "launched process {pid}, but no matching window appeared before timeout; no session was saved"
        );
    }
    session.save()?;
    Ok(LaunchResult {
        session,
        window: found,
    })
}

fn resolve_target(target: &TargetRequest) -> Result<(WindowId, Option<Session>)> {
    if let Some(window_id) = target.window_id {
        return Ok((window_id, None));
    }
    let session_id = target
        .session_id
        .as_deref()
        .ok_or_else(|| anyhow!("session_id or window_id is required"))?;
    let session = session::load(session_id)?;
    let window_id = session
        .window_id
        .ok_or_else(|| anyhow!("session {session_id} does not have a tracked window"))?;
    Ok((window_id, Some(session)))
}

fn find_free_display() -> u16 {
    for display in 90..150 {
        let socket = format!("/tmp/.X11-unix/X{display}");
        if !std::path::Path::new(&socket).exists() {
            return display;
        }
    }
    199
}

fn detach_command(command: &mut Command) {
    #[cfg(unix)]
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
}
