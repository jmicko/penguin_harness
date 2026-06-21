use crate::env_check;
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const MCP_HEADER: &str = "[mcp_servers.penguin_harness]";

#[derive(Debug, Serialize)]
pub struct InstallResult {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ProjectConfigResult {
    pub project_dir: PathBuf,
    pub config_path: PathBuf,
    pub command: String,
    pub updated_existing_block: bool,
    pub message: String,
}

pub fn default_installed_command() -> String {
    env_check::command_path("penguin-harness")
        .or_else(|| env::current_exe().ok().map(|p| p.display().to_string()))
        .unwrap_or_else(|| "penguin-harness".to_string())
}

pub fn mcp_config(command: &str) -> String {
    format!(
        r#"{MCP_HEADER}
command = "{}"
args = ["mcp"]
env_vars = ["DISPLAY", "WAYLAND_DISPLAY", "XAUTHORITY", "XDG_SESSION_TYPE", "XDG_CURRENT_DESKTOP", "XDG_RUNTIME_DIR", "DBUS_SESSION_BUS_ADDRESS", "PATH", "SHELL", "HOME"]
startup_timeout_sec = 20
tool_timeout_sec = 120
default_tools_approval_mode = "prompt"
"#,
        toml_string(command)
    )
}

pub fn init_project(
    project_dir: impl AsRef<Path>,
    command: Option<String>,
    force: bool,
) -> Result<ProjectConfigResult> {
    let project_dir = project_dir.as_ref().canonicalize().with_context(|| {
        format!(
            "canonicalize project directory {}",
            project_dir.as_ref().display()
        )
    })?;
    let codex_dir = project_dir.join(".codex");
    let config_path = codex_dir.join("config.toml");
    let command = command.unwrap_or_else(default_installed_command);
    let block = mcp_config(&command);

    fs::create_dir_all(&codex_dir).with_context(|| format!("create {}", codex_dir.display()))?;
    let old = fs::read_to_string(&config_path).unwrap_or_default();
    let (without_existing, replaced) = remove_existing_block(&old);
    if replaced && !force {
        bail!(
            "{} already contains {MCP_HEADER}; rerun with --force to replace it",
            config_path.display()
        );
    }

    let mut new_config = without_existing.trim_end().to_string();
    if !new_config.is_empty() {
        new_config.push_str("\n\n");
    }
    new_config.push_str(block.trim_end());
    new_config.push('\n');
    fs::write(&config_path, new_config)
        .with_context(|| format!("write {}", config_path.display()))?;

    Ok(ProjectConfigResult {
        project_dir,
        config_path,
        command,
        updated_existing_block: replaced,
        message: "project-scoped Codex MCP config is ready".to_string(),
    })
}

pub fn install_current_binary(destination: Option<PathBuf>, force: bool) -> Result<InstallResult> {
    let source = env::current_exe().context("locate current executable")?;
    let destination = destination.unwrap_or_else(default_install_path);
    if destination.exists() && !force {
        bail!(
            "{} already exists; rerun with --force to replace it",
            destination.display()
        );
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::copy(&source, &destination)
        .with_context(|| format!("copy {} to {}", source.display(), destination.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&destination, fs::Permissions::from_mode(0o755))
            .with_context(|| format!("chmod +x {}", destination.display()))?;
    }

    Ok(InstallResult {
        source,
        destination,
        message: "installed current executable".to_string(),
    })
}

fn default_install_path() -> PathBuf {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".local/bin/penguin-harness")
}

fn remove_existing_block(input: &str) -> (String, bool) {
    let mut output = Vec::new();
    let mut skipping = false;
    let mut replaced = false;

    for line in input.lines() {
        let trimmed = line.trim();
        if trimmed == MCP_HEADER {
            skipping = true;
            replaced = true;
            continue;
        }
        if skipping && trimmed.starts_with('[') {
            skipping = false;
        }
        if !skipping {
            output.push(line);
        }
    }

    (output.join("\n"), replaced)
}

fn toml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::{MCP_HEADER, remove_existing_block};

    #[test]
    fn removes_existing_mcp_block_only() {
        let input = format!(
            r#"[foo]
a = 1

{MCP_HEADER}
command = "old"
args = ["mcp"]

[bar]
b = 2
"#
        );

        let (output, replaced) = remove_existing_block(&input);

        assert!(replaced);
        assert!(output.contains("[foo]"));
        assert!(output.contains("[bar]"));
        assert!(!output.contains("command = \"old\""));
    }
}
