use crate::types::{Mode, WindowId};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Session {
    pub id: String,
    pub mode: Mode,
    pub display: String,
    pub command: Vec<String>,
    pub pid: Option<u32>,
    pub window_id: Option<WindowId>,
    pub title_hint: Option<String>,
    pub created_at_ms: u128,
    pub screenshots: Vec<PathBuf>,
    pub isolated_display_pid: Option<u32>,
    pub isolated_wm_pid: Option<u32>,
}

impl Session {
    pub fn new(
        mode: Mode,
        display: String,
        command: Vec<String>,
        pid: Option<u32>,
        title_hint: Option<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            mode,
            display,
            command,
            pid,
            window_id: None,
            title_hint,
            created_at_ms: now_ms(),
            screenshots: Vec::new(),
            isolated_display_pid: None,
            isolated_wm_pid: None,
        }
    }

    pub fn save(&self) -> Result<()> {
        fs::create_dir_all(sessions_dir())?;
        let path = session_path(&self.id);
        let json = serde_json::to_vec_pretty(self)?;
        fs::write(&path, json).with_context(|| format!("write {}", path.display()))
    }
}

pub fn root_dir() -> PathBuf {
    PathBuf::from("/tmp/penguin-harness")
}

pub fn sessions_dir() -> PathBuf {
    root_dir().join("sessions")
}

pub fn screenshots_dir() -> PathBuf {
    root_dir().join("screenshots")
}

pub fn session_path(id: &str) -> PathBuf {
    sessions_dir().join(format!("{id}.json"))
}

pub fn load(id: &str) -> Result<Session> {
    let path = session_path(id);
    let data = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_slice(&data).with_context(|| format!("parse {}", path.display()))
}

pub fn list() -> Result<Vec<Session>> {
    let dir = sessions_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut sessions: Vec<Session> = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        if entry.path().extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let data = fs::read(entry.path())?;
        if let Ok(session) = serde_json::from_slice(&data) {
            sessions.push(session);
        }
    }
    sessions.sort_by_key(|s| s.created_at_ms);
    Ok(sessions)
}

pub fn remove(id: &str) -> Result<()> {
    let path = session_path(id);
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
    }
    Ok(())
}

pub fn ensure_dirs() -> Result<()> {
    fs::create_dir_all(sessions_dir())?;
    fs::create_dir_all(screenshots_dir())?;
    Ok(())
}

pub fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub fn timestamped_png_path(session_id: &str) -> Result<PathBuf> {
    fs::create_dir_all(screenshots_dir())?;
    Ok(screenshots_dir().join(format!("{session_id}-{}.png", now_ms())))
}
