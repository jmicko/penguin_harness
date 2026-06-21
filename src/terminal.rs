use crate::actions::{self, LaunchRequest, LaunchResult};
use crate::env_check;
use crate::session;
use crate::types::Mode;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct TerminalRequest {
    #[serde(default)]
    pub mode: Mode,
    pub terminal: Option<String>,
    pub title_hint: Option<String>,
    pub cwd: Option<PathBuf>,
    pub command: Option<Vec<String>>,
    pub wait_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct TerminalResult {
    pub terminal_command: Vec<String>,
    pub launch: LaunchResult,
}

pub fn launch_terminal(request: TerminalRequest) -> Result<TerminalResult> {
    let terminal = request.terminal.unwrap_or_else(default_terminal);
    let title = request
        .title_hint
        .unwrap_or_else(|| format!("penguin-terminal-{}", &Uuid::new_v4().to_string()[..8]));
    let terminal_command = terminal_command(
        &terminal,
        &title,
        request.cwd.as_deref(),
        request.command.as_deref(),
    )?;
    let launch = actions::launch_app(LaunchRequest {
        command: terminal_command.clone(),
        mode: request.mode,
        title_hint: Some(title),
        wait_ms: request.wait_ms,
    })?;
    Ok(TerminalResult {
        terminal_command,
        launch,
    })
}

pub fn default_terminal() -> String {
    [
        "x-terminal-emulator",
        "mate-terminal",
        "gnome-terminal",
        "xfce4-terminal",
        "konsole",
        "kitty",
        "alacritty",
        "wezterm",
        "xterm",
    ]
    .into_iter()
    .find(|name| env_check::command_path(name).is_some())
    .unwrap_or("xterm")
    .to_string()
}

fn terminal_command(
    terminal: &str,
    title: &str,
    cwd: Option<&Path>,
    command: Option<&[String]>,
) -> Result<Vec<String>> {
    let path = env_check::command_path(terminal).unwrap_or_else(|| terminal.to_string());
    let resolved_name = resolved_terminal_name(&path);
    let script = if let Some(command) = command {
        Some(write_command_script(command)?)
    } else {
        None
    };

    let mut args = vec![path.clone()];
    if resolved_name.contains("mate-terminal") {
        args.push("--disable-factory".to_string());
        args.push("--title".to_string());
        args.push(title.to_string());
        if let Some(cwd) = cwd {
            args.push("--working-directory".to_string());
            args.push(cwd.display().to_string());
        }
        if let Some(script) = script {
            args.push("-e".to_string());
            args.push(script.display().to_string());
        }
    } else if resolved_name.contains("gnome-terminal") {
        args.push("--title".to_string());
        args.push(title.to_string());
        if let Some(cwd) = cwd {
            args.push("--working-directory".to_string());
            args.push(cwd.display().to_string());
        }
        if let Some(script) = script {
            args.push("--".to_string());
            args.push(script.display().to_string());
        }
    } else if resolved_name.contains("xfce4-terminal") {
        args.push("--disable-server".to_string());
        args.push("--title".to_string());
        args.push(title.to_string());
        if let Some(cwd) = cwd {
            args.push("--working-directory".to_string());
            args.push(cwd.display().to_string());
        }
        if let Some(script) = script {
            args.push("--command".to_string());
            args.push(script.display().to_string());
        }
    } else if resolved_name.contains("xterm") {
        args.push("-T".to_string());
        args.push(title.to_string());
        if let Some(script) = script {
            args.push("-e".to_string());
            args.push(script.display().to_string());
        } else if let Some(cwd) = cwd {
            args.push("-e".to_string());
            args.push("/bin/sh".to_string());
            args.push("-lc".to_string());
            args.push(format!(
                "cd {} && exec \"${{SHELL:-/bin/sh}}\"",
                shell_quote(cwd)
            ));
        }
    } else if resolved_name.contains("kitty") {
        args.push("--title".to_string());
        args.push(title.to_string());
        if let Some(cwd) = cwd {
            args.push("--directory".to_string());
            args.push(cwd.display().to_string());
        }
        if let Some(script) = script {
            args.push(script.display().to_string());
        }
    } else if resolved_name.contains("alacritty") {
        args.push("--title".to_string());
        args.push(title.to_string());
        if let Some(cwd) = cwd {
            args.push("--working-directory".to_string());
            args.push(cwd.display().to_string());
        }
        if let Some(script) = script {
            args.push("-e".to_string());
            args.push(script.display().to_string());
        }
    } else if resolved_name.contains("konsole") {
        args.push("--new-tab".to_string());
        args.push("-p".to_string());
        args.push(format!("tabtitle={title}"));
        if let Some(cwd) = cwd {
            args.push("--workdir".to_string());
            args.push(cwd.display().to_string());
        }
        if let Some(script) = script {
            args.push("-e".to_string());
            args.push(script.display().to_string());
        }
    } else if resolved_name.contains("wezterm") {
        args.push("start".to_string());
        args.push("--class".to_string());
        args.push(title.to_string());
        if let Some(cwd) = cwd {
            args.push("--cwd".to_string());
            args.push(cwd.display().to_string());
        }
        if let Some(script) = script {
            args.push("--".to_string());
            args.push(script.display().to_string());
        }
    } else {
        if script.is_some() || cwd.is_some() {
            bail!("terminal {terminal:?} is not recognized enough to pass cwd/command safely");
        }
    }
    Ok(args)
}

fn write_command_script(command: &[String]) -> Result<PathBuf> {
    if command.is_empty() {
        bail!("terminal command cannot be empty");
    }
    fs::create_dir_all(session::root_dir().join("terminal-scripts"))?;
    let path = session::root_dir()
        .join("terminal-scripts")
        .join(format!("{}.sh", Uuid::new_v4()));
    let command = command
        .iter()
        .map(|part| shell_quote_str(part))
        .collect::<Vec<_>>()
        .join(" ");
    let content = format!("#!/usr/bin/env bash\nexec {command}\n");
    fs::write(&path, content).with_context(|| format!("write {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .with_context(|| format!("chmod +x {}", path.display()))?;
    }
    Ok(path)
}

fn resolved_terminal_name(path: &str) -> String {
    let raw = Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_ascii_lowercase();
    let resolved = fs::canonicalize(path)
        .ok()
        .and_then(|p| p.file_name().map(|s| s.to_string_lossy().to_string()))
        .unwrap_or_default()
        .to_ascii_lowercase();
    format!("{raw} {resolved}")
}

fn shell_quote(path: &Path) -> String {
    shell_quote_str(&path.display().to_string())
}

fn shell_quote_str(value: &str) -> String {
    if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_./:=+".contains(c))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
}

#[cfg(test)]
mod tests {
    use super::{shell_quote_str, terminal_command};

    #[test]
    fn shell_quote_handles_spaces_and_quotes() {
        assert_eq!(shell_quote_str("abc/def"), "abc/def");
        assert_eq!(shell_quote_str("a b"), "'a b'");
        assert_eq!(shell_quote_str("a'b"), "'a'\"'\"'b'");
    }

    #[test]
    fn unknown_terminal_is_not_given_a_fake_title_argument() {
        let command = terminal_command("definitely-not-a-real-terminal", "title", None, None)
            .expect("terminal command");

        assert_eq!(command, ["definitely-not-a-real-terminal"]);
    }
}
