use crate::{env_check, screenshot as x11_screenshot, session};
use anyhow::{Context, Result, anyhow, bail};
use ashpd::desktop::screenshot::Screenshot;
use image::GenericImageView;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio::runtime::Builder;
use url::Url;

#[derive(Debug, Serialize)]
pub struct NativeScreenshotCheck {
    pub backends: Vec<ScreenshotBackendCheck>,
}

#[derive(Debug, Serialize)]
pub struct ScreenshotBackendCheck {
    pub name: String,
    pub available: bool,
    pub detail: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct NativeScreenshotResult {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub backend: String,
}

pub fn check() -> NativeScreenshotCheck {
    NativeScreenshotCheck {
        backends: vec![
            ScreenshotBackendCheck {
                name: "gnome_shell_screenshot".to_string(),
                available: gnome_shell_screenshot_available(),
                detail: Some("org.gnome.Shell.Screenshot DBus interface".to_string()),
            },
            ScreenshotBackendCheck {
                name: "gnome_screenshot".to_string(),
                available: env_check::command_path("gnome-screenshot").is_some(),
                detail: env_check::command_path("gnome-screenshot"),
            },
            ScreenshotBackendCheck {
                name: "xdg_desktop_portal_screenshot".to_string(),
                available: crate::env_check::check_portal().screenshot.available,
                detail: Some("org.freedesktop.portal.Screenshot".to_string()),
            },
            ScreenshotBackendCheck {
                name: "grim".to_string(),
                available: env_check::command_path("grim").is_some(),
                detail: env_check::command_path("grim"),
            },
            ScreenshotBackendCheck {
                name: "x11_root".to_string(),
                available: crate::x11_control::screen_size().is_ok(),
                detail: Some("X11 root window capture".to_string()),
            },
        ],
    }
}

pub fn capture(include_cursor: bool) -> Result<NativeScreenshotResult> {
    let mut failures = Vec::new();

    let path = session::timestamped_png_path("native-screen")?;
    match gnome_shell_screenshot(&path, include_cursor) {
        Ok(()) => return result(path, "gnome_shell_screenshot"),
        Err(error) => failures.push(backend_failure("gnome_shell_screenshot", error)),
    }

    let path = session::timestamped_png_path("native-screen")?;
    match command_screenshot(
        "gnome-screenshot",
        gnome_screenshot_args(&path, include_cursor),
    ) {
        Ok(()) => return result(path, "gnome_screenshot"),
        Err(error) => failures.push(backend_failure("gnome_screenshot", error)),
    }

    let path = session::timestamped_png_path("native-screen")?;
    match portal_screenshot(&path) {
        Ok(()) => return result(path, "xdg_desktop_portal_screenshot"),
        Err(error) => failures.push(backend_failure("xdg_desktop_portal_screenshot", error)),
    }

    let path = session::timestamped_png_path("native-screen")?;
    match command_screenshot("grim", vec![path.display().to_string()]) {
        Ok(()) => return result(path, "grim"),
        Err(error) => failures.push(backend_failure("grim", error)),
    }

    let path = session::timestamped_png_path("native-screen")?;
    match x11_screenshot::capture_screen(&path) {
        Ok(_) => return result(path, "x11_root"),
        Err(error) => failures.push(backend_failure("x11_root", error)),
    }

    bail!(
        "no native screenshot backend succeeded:\n{}",
        failures.join("\n")
    );
}

fn backend_failure(name: &str, error: anyhow::Error) -> String {
    format!("{name}: {error:#}")
}

fn result(path: PathBuf, backend: &str) -> Result<NativeScreenshotResult> {
    let image =
        image::open(&path).with_context(|| format!("read native screenshot {}", path.display()))?;
    let (width, height) = image.dimensions();
    Ok(NativeScreenshotResult {
        path,
        width,
        height,
        backend: backend.to_string(),
    })
}

fn gnome_shell_screenshot_available() -> bool {
    let Some(gdbus) = env_check::command_path("gdbus") else {
        return false;
    };
    Command::new(gdbus)
        .args([
            "introspect",
            "--session",
            "--dest",
            "org.gnome.Shell.Screenshot",
            "--object-path",
            "/org/gnome/Shell/Screenshot",
        ])
        .output()
        .is_ok_and(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).contains("org.gnome.Shell.Screenshot")
        })
}

fn gnome_shell_screenshot(path: &Path, include_cursor: bool) -> Result<()> {
    let gdbus = env_check::command_path("gdbus").ok_or_else(|| anyhow!("gdbus is unavailable"))?;
    let output = Command::new(gdbus)
        .args([
            "call",
            "--session",
            "--dest",
            "org.gnome.Shell.Screenshot",
            "--object-path",
            "/org/gnome/Shell/Screenshot",
            "--method",
            "org.gnome.Shell.Screenshot.Screenshot",
            if include_cursor { "true" } else { "false" },
            "false",
            path.to_str()
                .ok_or_else(|| anyhow!("screenshot path is not valid UTF-8"))?,
        ])
        .output()
        .context("run gnome shell screenshot DBus call")?;
    if !output.status.success() {
        bail!(
            "gnome shell screenshot failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.contains("true") || !path.exists() {
        bail!("gnome shell screenshot did not produce {}", path.display());
    }
    Ok(())
}

fn gnome_screenshot_args(path: &Path, include_cursor: bool) -> Vec<String> {
    let mut args = Vec::new();
    if include_cursor {
        args.push("--include-pointer".to_string());
    }
    args.push("-f".to_string());
    args.push(path.display().to_string());
    args
}

fn command_screenshot(command: &str, args: Vec<String>) -> Result<()> {
    let command_path =
        env_check::command_path(command).ok_or_else(|| anyhow!("{command} is unavailable"))?;
    let output = Command::new(command_path)
        .args(args)
        .output()
        .with_context(|| format!("run {command} screenshot backend"))?;
    if !output.status.success() {
        bail!(
            "{command} screenshot failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn portal_screenshot(path: &Path) -> Result<()> {
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .context("create screenshot portal runtime")?;
    let response = runtime.block_on(async {
        let request = Screenshot::request()
            .interactive(false)
            .modal(false)
            .send()
            .await?;
        let response = request.response()?;
        Ok::<_, anyhow::Error>(response)
    })?;
    let source = file_uri_to_path(response.uri().as_str())?;
    fs::copy(&source, path).with_context(|| {
        format!(
            "copy portal screenshot {} to {}",
            source.display(),
            path.display()
        )
    })?;
    Ok(())
}

fn file_uri_to_path(uri: &str) -> Result<PathBuf> {
    let url = Url::parse(uri).with_context(|| format!("parse screenshot portal URI {uri}"))?;
    if url.scheme() != "file" {
        bail!(
            "screenshot portal returned unsupported URI scheme {}",
            url.scheme()
        );
    }
    url.to_file_path()
        .map_err(|_| anyhow!("screenshot portal URI is not a local path: {uri}"))
}
