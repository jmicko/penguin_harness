use crate::types::{Geometry, WindowId, WindowInfo};
use anyhow::{Context, Result, anyhow};
use std::collections::HashSet;
use std::thread;
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ClientMessageEvent, ConfigureWindowAux, ConnectionExt as XprotoConnectionExt,
    EventMask, GetPropertyReply, StackMode, Window,
};

const EWMH_SOURCE_APPLICATION: u32 = 1;

pub fn list_windows() -> Result<Vec<WindowInfo>> {
    let (conn, screen_num) = x11rb::connect(None).context("connect to X11 display")?;
    let atoms = Atoms::load(&conn)?;
    let root = conn.setup().roots[screen_num].root;
    let ids = client_list(&conn, &atoms, root)?;

    Ok(ids
        .into_iter()
        .filter_map(|id| window_info_from_connection(&conn, &atoms, WindowId(id)).ok())
        .collect())
}

pub fn focus_window(window_id: WindowId) -> Result<()> {
    let (conn, screen_num) = x11rb::connect(None).context("connect to X11 display")?;
    let atoms = Atoms::load(&conn)?;
    let root = conn.setup().roots[screen_num].root;
    let event = ClientMessageEvent::new(
        32,
        window_id.0,
        atoms.net_active_window,
        [EWMH_SOURCE_APPLICATION, x11rb::CURRENT_TIME, 0, 0, 0],
    );
    conn.send_event(
        false,
        root,
        EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
        event,
    )
    .with_context(|| format!("send _NET_ACTIVE_WINDOW for {window_id}"))?
    .check()
    .with_context(|| format!("check _NET_ACTIVE_WINDOW for {window_id}"))?;
    if let Ok(cookie) = conn.configure_window(
        window_id.0,
        &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
    ) {
        let _ = cookie.check();
    }
    conn.flush()?;
    Ok(())
}

pub fn close_window(window_id: WindowId) -> Result<()> {
    let (conn, screen_num) = x11rb::connect(None).context("connect to X11 display")?;
    let atoms = Atoms::load(&conn)?;
    let root = conn.setup().roots[screen_num].root;
    let event = ClientMessageEvent::new(
        32,
        window_id.0,
        atoms.net_close_window,
        [x11rb::CURRENT_TIME, EWMH_SOURCE_APPLICATION, 0, 0, 0],
    );
    conn.send_event(
        false,
        root,
        EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
        event,
    )
    .with_context(|| format!("send _NET_CLOSE_WINDOW for {window_id}"))?
    .check()
    .with_context(|| format!("check _NET_CLOSE_WINDOW for {window_id}"))?;
    conn.flush()?;
    Ok(())
}

pub fn get_geometry(window_id: WindowId) -> Result<Geometry> {
    let (conn, screen_num) = x11rb::connect(None).context("connect to X11 display")?;
    let geometry = conn
        .get_geometry(window_id.0)
        .with_context(|| format!("request X11 geometry for {window_id}"))?
        .reply()
        .with_context(|| format!("read X11 geometry for {window_id}"))?;
    let root = conn.setup().roots[screen_num].root;
    let translated = conn
        .translate_coordinates(window_id.0, root, 0, 0)
        .with_context(|| format!("translate {window_id} coordinates to root"))?
        .reply()
        .with_context(|| format!("read translated coordinates for {window_id}"))?;
    if !translated.same_screen {
        return Err(anyhow!("{window_id} is not on the active X11 screen"));
    }

    Ok(Geometry {
        x: i32::from(translated.dst_x),
        y: i32::from(translated.dst_y),
        width: u32::from(geometry.width),
        height: u32::from(geometry.height),
    })
}

pub fn ensure_window_exists(window_id: WindowId) -> Result<()> {
    get_geometry(window_id).map(|_| ())
}

pub fn wait_until_gone(window_id: WindowId, timeout: Duration) -> Result<bool> {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if get_geometry(window_id).is_err() {
            return Ok(true);
        }
        thread::sleep(Duration::from_millis(100));
    }
    Ok(get_geometry(window_id).is_err())
}

pub fn get_window_info(window_id: WindowId) -> Result<WindowInfo> {
    let (conn, _) = x11rb::connect(None).context("connect to X11 display")?;
    let atoms = Atoms::load(&conn)?;
    window_info_from_connection(&conn, &atoms, window_id)
}

pub fn wait_for_window(
    prior: &[WindowInfo],
    pid: Option<u32>,
    title_hint: Option<&str>,
    timeout: Duration,
) -> Result<Option<WindowInfo>> {
    let prior_ids: HashSet<u32> = prior.iter().map(|w| w.id.0).collect();
    let started = Instant::now();
    while started.elapsed() < timeout {
        let windows = list_windows().unwrap_or_default();
        if let Some(pid) = pid {
            if let Some(window) = windows.iter().find(|w| w.pid == Some(pid)) {
                return Ok(Some(window.clone()));
            }
        }
        if let Some(hint) = title_hint {
            let hint = hint.to_lowercase();
            if let Some(window) = windows
                .iter()
                .find(|w| w.title.to_lowercase().contains(&hint))
            {
                return Ok(Some(window.clone()));
            }
        }
        if let Some(window) = windows.iter().find(|w| !prior_ids.contains(&w.id.0)) {
            return Ok(Some(window.clone()));
        }
        thread::sleep(Duration::from_millis(150));
    }
    Ok(None)
}

struct Atoms {
    net_client_list_stacking: Atom,
    net_client_list: Atom,
    net_active_window: Atom,
    net_close_window: Atom,
    net_wm_name: Atom,
    utf8_string: Atom,
    wm_name: Atom,
    net_wm_pid: Atom,
    net_wm_desktop: Atom,
    wm_client_machine: Atom,
}

impl Atoms {
    fn load(conn: &impl Connection) -> Result<Self> {
        Ok(Self {
            net_client_list_stacking: intern(conn, b"_NET_CLIENT_LIST_STACKING")?,
            net_client_list: intern(conn, b"_NET_CLIENT_LIST")?,
            net_active_window: intern(conn, b"_NET_ACTIVE_WINDOW")?,
            net_close_window: intern(conn, b"_NET_CLOSE_WINDOW")?,
            net_wm_name: intern(conn, b"_NET_WM_NAME")?,
            utf8_string: intern(conn, b"UTF8_STRING")?,
            wm_name: AtomEnum::WM_NAME.into(),
            net_wm_pid: intern(conn, b"_NET_WM_PID")?,
            net_wm_desktop: intern(conn, b"_NET_WM_DESKTOP")?,
            wm_client_machine: AtomEnum::WM_CLIENT_MACHINE.into(),
        })
    }
}

fn intern(conn: &impl Connection, name: &[u8]) -> Result<Atom> {
    conn.intern_atom(false, name)?
        .reply()
        .with_context(|| format!("intern atom {}", String::from_utf8_lossy(name)))
        .map(|reply| reply.atom)
}

fn client_list(conn: &impl Connection, atoms: &Atoms, root: Window) -> Result<Vec<Window>> {
    read_window_list(conn, root, atoms.net_client_list_stacking)?
        .or_else(|| {
            read_window_list(conn, root, atoms.net_client_list)
                .ok()
                .flatten()
        })
        .or_else(|| {
            conn.query_tree(root)
                .ok()?
                .reply()
                .ok()
                .map(|reply| reply.children)
        })
        .ok_or_else(|| anyhow!("could not read X11 client window list"))
}

fn read_window_list(
    conn: &impl Connection,
    window: Window,
    property: Atom,
) -> Result<Option<Vec<Window>>> {
    let reply = get_property(conn, window, property, AtomEnum::WINDOW.into(), u32::MAX)?;
    if reply.type_ == u32::from(AtomEnum::NONE) {
        return Ok(None);
    }
    Ok(reply
        .value32()
        .map(|values| values.filter(|id| *id != 0).collect::<Vec<_>>()))
}

fn window_info_from_connection(
    conn: &impl Connection,
    atoms: &Atoms,
    window_id: WindowId,
) -> Result<WindowInfo> {
    Ok(WindowInfo {
        id: window_id,
        desktop: desktop_property(conn, atoms, window_id.0)?,
        pid: u32_property(conn, window_id.0, atoms.net_wm_pid)?,
        geometry: get_geometry(window_id)?,
        host: text_property(
            conn,
            window_id.0,
            atoms.wm_client_machine,
            AtomEnum::STRING.into(),
        )?,
        title: window_title(conn, atoms, window_id.0)?.unwrap_or_default(),
    })
}

fn window_title(conn: &impl Connection, atoms: &Atoms, window: Window) -> Result<Option<String>> {
    text_property(conn, window, atoms.net_wm_name, atoms.utf8_string).and_then(|title| {
        if title.as_deref().is_some_and(|value| !value.is_empty()) {
            Ok(title)
        } else {
            text_property(conn, window, atoms.wm_name, AtomEnum::STRING.into())
        }
    })
}

fn text_property(
    conn: &impl Connection,
    window: Window,
    property: Atom,
    type_: Atom,
) -> Result<Option<String>> {
    let reply = get_property(conn, window, property, type_, 1024)?;
    if reply.type_ == u32::from(AtomEnum::NONE) || reply.value.is_empty() {
        return Ok(None);
    }
    let value = String::from_utf8_lossy(&reply.value)
        .trim_end_matches('\0')
        .to_string();
    Ok(Some(value))
}

fn u32_property(conn: &impl Connection, window: Window, property: Atom) -> Result<Option<u32>> {
    let reply = get_property(conn, window, property, AtomEnum::CARDINAL.into(), 1)?;
    if reply.type_ == u32::from(AtomEnum::NONE) {
        return Ok(None);
    }
    Ok(reply.value32().and_then(|mut values| values.next()))
}

fn desktop_property(conn: &impl Connection, atoms: &Atoms, window: Window) -> Result<Option<i32>> {
    Ok(
        u32_property(conn, window, atoms.net_wm_desktop)?.and_then(|desktop| {
            if desktop == u32::MAX {
                Some(-1)
            } else {
                i32::try_from(desktop).ok()
            }
        }),
    )
}

fn get_property(
    conn: &impl Connection,
    window: Window,
    property: Atom,
    type_: Atom,
    long_length: u32,
) -> Result<GetPropertyReply> {
    conn.get_property(false, window, property, type_, 0, long_length)?
        .reply()
        .with_context(|| format!("read X11 property 0x{property:x} from 0x{window:x}"))
}
