use crate::actions::{self, LaunchRequest, MoveResizeRequest, TargetRequest, WindowQuery};
use crate::terminal::TerminalRequest;
use crate::types::{MouseButton, WindowId};
use anyhow::{Context, Result, anyhow};
use base64::Engine;
use serde::Deserialize;
use serde_json::{Value, json};
use std::io::{self, BufRead, Write};

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ToolCall {
    name: String,
    arguments: Option<Value>,
}

struct McpState {
    portal: crate::portal::PortalController,
}

impl McpState {
    fn new() -> Result<Self> {
        Ok(Self {
            portal: crate::portal::PortalController::new()?,
        })
    }
}

pub fn serve() -> Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut state = McpState::new()?;
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(error) => {
                write_response(
                    &mut stdout,
                    None,
                    Err(json!({"code": -32700, "message": error.to_string()})),
                )?;
                continue;
            }
        };

        if request.id.is_none() {
            continue;
        }
        let id = request.id.clone();
        let result = match request.method.as_str() {
            "initialize" => initialize_result(request.params.as_ref()),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tool_definitions() })),
            "tools/call" => call_tool(request.params, &mut state),
            method => Err(json!({
                "code": -32601,
                "message": format!("unknown method {method}")
            })),
        };
        write_response(&mut stdout, id, result)?;
    }
    Ok(())
}

fn initialize_result(params: Option<&Value>) -> Result<Value, Value> {
    let protocol = params
        .and_then(|p| p.get("protocolVersion"))
        .and_then(Value::as_str)
        .unwrap_or("2024-11-05");
    Ok(json!({
        "protocolVersion": protocol,
        "capabilities": {
            "tools": { "listChanged": false }
        },
        "serverInfo": {
            "name": "penguin_harness",
            "version": env!("CARGO_PKG_VERSION")
        },
        "instructions": "Linux desktop control. X11 tools control X11/Xwayland windows through XTEST/EWMH. Portal tools control Wayland desktops through xdg-desktop-portal RemoteDesktop/ScreenCast after the user approves the system permission prompt. portal_start requests persistent portal permission and reuses a saved restore token when the compositor supports it. Take screenshots before and after GUI actions that change visible state. X11 window coordinates are relative to the target window screenshot; portal screen coordinates are absolute screen pixels."
    }))
}

fn call_tool(params: Option<Value>, state: &mut McpState) -> Result<Value, Value> {
    let call: ToolCall = parse_params(params)?;
    let args = call.arguments.unwrap_or_else(|| json!({}));
    let outcome =
        match call.name.as_str() {
            "check_environment" => ok_text(actions::check_environment()),
            "list_sessions" => ok_text_result(crate::session::list()),
            "launch_app" => typed::<LaunchRequest>(args)
                .and_then(|request| ok_text(actions::launch_app(request)?)),
            "launch_terminal" => typed::<TerminalRequest>(args)
                .and_then(|request| ok_text(crate::terminal::launch_terminal(request)?)),
            "list_windows" => ok_text_result(actions::list_windows()),
            "find_windows" => {
                typed::<WindowQuery>(args).and_then(|query| ok_text(actions::find_windows(query)?))
            }
            "window_info" => typed::<TargetRequest>(args)
                .and_then(|target| ok_text(actions::window_info(target)?)),
            "active_window" => ok_text_result(actions::active_window()),
            "focus_window" => {
                typed::<TargetRequest>(args).and_then(|target| ok_text(actions::focus(target)?))
            }
            "close_window" => typed::<TargetRequest>(args)
                .and_then(|target| ok_text(actions::close_window(target)?)),
            "move_resize_window" => typed::<MoveResizeRequest>(args)
                .and_then(|request| ok_text(actions::move_resize_window(request)?)),
            "portal_start" => ok_text_result(state.portal.start()),
            "portal_status" => ok_text_result(state.portal.status()),
            "portal_screenshot" => portal_screenshot_tool(args, &state.portal),
            "portal_click_screen" => portal_click_screen_tool(args, &state.portal, false),
            "portal_double_click_screen" => portal_click_screen_tool(args, &state.portal, true),
            "portal_drag_screen" => portal_drag_screen_tool(args, &state.portal),
            "portal_scroll_screen" => portal_scroll_screen_tool(args, &state.portal),
            "portal_type_text" => portal_type_text_tool(args, &state.portal),
            "portal_press_key" => portal_press_key_tool(args, &state.portal),
            "screenshot" => screenshot_tool(args),
            "screenshot_screen" => screenshot_screen_tool(args),
            "type_text" => type_text_tool(args),
            "type_active" => type_active_tool(args),
            "press_key" => press_key_tool(args),
            "press_key_active" => press_key_active_tool(args),
            "click" => click_tool(args, false),
            "click_screen" => click_screen_tool(args, false),
            "double_click" => click_tool(args, true),
            "double_click_screen" => click_screen_tool(args, true),
            "drag" => drag_tool(args),
            "drag_screen" => drag_screen_tool(args),
            "scroll" => scroll_tool(args),
            "scroll_screen" => scroll_screen_tool(args),
            "close_session" => close_tool(args),
            name => Err(anyhow!("unknown tool {name}")),
        };

    match outcome {
        Ok(value) => Ok(value),
        Err(error) => Ok(json!({
            "content": [{ "type": "text", "text": error.to_string() }],
            "isError": true
        })),
    }
}

#[derive(Debug, Deserialize)]
struct ScreenshotArgs {
    session_id: Option<String>,
    window_id: Option<WindowId>,
    #[serde(default)]
    include_image: bool,
}

#[derive(Debug, Deserialize)]
struct ScreenScreenshotArgs {
    #[serde(default)]
    include_image: bool,
}

#[derive(Debug, Deserialize)]
struct TextArgs {
    session_id: Option<String>,
    window_id: Option<WindowId>,
    text: String,
}

#[derive(Debug, Deserialize)]
struct ActiveTextArgs {
    text: String,
}

#[derive(Debug, Deserialize)]
struct KeyArgs {
    session_id: Option<String>,
    window_id: Option<WindowId>,
    key: String,
}

#[derive(Debug, Deserialize)]
struct ActiveKeyArgs {
    key: String,
}

#[derive(Debug, Deserialize)]
struct ClickArgs {
    session_id: Option<String>,
    window_id: Option<WindowId>,
    x: i32,
    y: i32,
    #[serde(default)]
    button: MouseButton,
}

#[derive(Debug, Deserialize)]
struct ScreenClickArgs {
    x: i32,
    y: i32,
    #[serde(default)]
    button: MouseButton,
}

#[derive(Debug, Deserialize)]
struct DragArgs {
    session_id: Option<String>,
    window_id: Option<WindowId>,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    #[serde(default)]
    button: MouseButton,
}

#[derive(Debug, Deserialize)]
struct ScreenDragArgs {
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    #[serde(default)]
    button: MouseButton,
}

#[derive(Debug, Deserialize)]
struct ScrollArgs {
    session_id: Option<String>,
    window_id: Option<WindowId>,
    x: i32,
    y: i32,
    amount: i32,
}

#[derive(Debug, Deserialize)]
struct ScreenScrollArgs {
    x: i32,
    y: i32,
    amount: i32,
}

#[derive(Debug, Deserialize)]
struct CloseArgs {
    session_id: String,
}

fn screenshot_tool(args: Value) -> Result<Value> {
    let args: ScreenshotArgs = typed(args)?;
    let include_image = args.include_image;
    let result = actions::screenshot(TargetRequest {
        session_id: args.session_id,
        window_id: args.window_id,
    })?;
    let text = serde_json::to_string_pretty(&result)?;
    let mut content = vec![json!({ "type": "text", "text": text })];
    if include_image {
        let bytes = std::fs::read(&result.path)
            .with_context(|| format!("read screenshot {}", result.path.display()))?;
        content.push(json!({
            "type": "image",
            "mimeType": "image/png",
            "data": base64::engine::general_purpose::STANDARD.encode(bytes)
        }));
    }
    Ok(json!({ "content": content, "isError": false }))
}

fn screenshot_screen_tool(args: Value) -> Result<Value> {
    let args: ScreenScreenshotArgs = typed(args)?;
    let result = actions::screenshot_screen()?;
    let text = serde_json::to_string_pretty(&result)?;
    let mut content = vec![json!({ "type": "text", "text": text })];
    if args.include_image {
        let bytes = std::fs::read(&result.path)
            .with_context(|| format!("read screenshot {}", result.path.display()))?;
        content.push(json!({
            "type": "image",
            "mimeType": "image/png",
            "data": base64::engine::general_purpose::STANDARD.encode(bytes)
        }));
    }
    Ok(json!({ "content": content, "isError": false }))
}

fn portal_screenshot_tool(args: Value, portal: &crate::portal::PortalController) -> Result<Value> {
    let args: ScreenScreenshotArgs = typed(args)?;
    let result = portal.screenshot()?;
    let text = serde_json::to_string_pretty(&result)?;
    let mut content = vec![json!({ "type": "text", "text": text })];
    if args.include_image {
        let bytes = std::fs::read(&result.path)
            .with_context(|| format!("read screenshot {}", result.path.display()))?;
        content.push(json!({
            "type": "image",
            "mimeType": "image/png",
            "data": base64::engine::general_purpose::STANDARD.encode(bytes)
        }));
    }
    Ok(json!({ "content": content, "isError": false }))
}

fn type_text_tool(args: Value) -> Result<Value> {
    let args: TextArgs = typed(args)?;
    ok_text(actions::type_text(
        TargetRequest {
            session_id: args.session_id,
            window_id: args.window_id,
        },
        &args.text,
    )?)
}

fn type_active_tool(args: Value) -> Result<Value> {
    let args: ActiveTextArgs = typed(args)?;
    ok_text(actions::type_text_active(&args.text)?)
}

fn press_key_tool(args: Value) -> Result<Value> {
    let args: KeyArgs = typed(args)?;
    ok_text(actions::press_key(
        TargetRequest {
            session_id: args.session_id,
            window_id: args.window_id,
        },
        &args.key,
    )?)
}

fn press_key_active_tool(args: Value) -> Result<Value> {
    let args: ActiveKeyArgs = typed(args)?;
    ok_text(actions::press_key_active(&args.key)?)
}

fn click_tool(args: Value, double: bool) -> Result<Value> {
    let args: ClickArgs = typed(args)?;
    let target = TargetRequest {
        session_id: args.session_id,
        window_id: args.window_id,
    };
    if double {
        ok_text(actions::double_click(target, args.x, args.y, args.button)?)
    } else {
        ok_text(actions::click(target, args.x, args.y, args.button)?)
    }
}

fn click_screen_tool(args: Value, double: bool) -> Result<Value> {
    let args: ScreenClickArgs = typed(args)?;
    if double {
        ok_text(actions::double_click_screen(args.x, args.y, args.button)?)
    } else {
        ok_text(actions::click_screen(args.x, args.y, args.button)?)
    }
}

fn portal_click_screen_tool(
    args: Value,
    portal: &crate::portal::PortalController,
    double: bool,
) -> Result<Value> {
    let args: ScreenClickArgs = typed(args)?;
    if double {
        ok_text(portal.double_click_screen(args.x, args.y, args.button)?)
    } else {
        ok_text(portal.click_screen(args.x, args.y, args.button)?)
    }
}

fn drag_tool(args: Value) -> Result<Value> {
    let args: DragArgs = typed(args)?;
    ok_text(actions::drag(
        TargetRequest {
            session_id: args.session_id,
            window_id: args.window_id,
        },
        args.x1,
        args.y1,
        args.x2,
        args.y2,
        args.button,
    )?)
}

fn drag_screen_tool(args: Value) -> Result<Value> {
    let args: ScreenDragArgs = typed(args)?;
    ok_text(actions::drag_screen(
        args.x1,
        args.y1,
        args.x2,
        args.y2,
        args.button,
    )?)
}

fn portal_drag_screen_tool(args: Value, portal: &crate::portal::PortalController) -> Result<Value> {
    let args: ScreenDragArgs = typed(args)?;
    ok_text(portal.drag_screen(args.x1, args.y1, args.x2, args.y2, args.button)?)
}

fn scroll_tool(args: Value) -> Result<Value> {
    let args: ScrollArgs = typed(args)?;
    ok_text(actions::scroll(
        TargetRequest {
            session_id: args.session_id,
            window_id: args.window_id,
        },
        args.x,
        args.y,
        args.amount,
    )?)
}

fn scroll_screen_tool(args: Value) -> Result<Value> {
    let args: ScreenScrollArgs = typed(args)?;
    ok_text(actions::scroll_screen(args.x, args.y, args.amount)?)
}

fn portal_scroll_screen_tool(
    args: Value,
    portal: &crate::portal::PortalController,
) -> Result<Value> {
    let args: ScreenScrollArgs = typed(args)?;
    ok_text(portal.scroll_screen(args.x, args.y, args.amount)?)
}

fn portal_type_text_tool(args: Value, portal: &crate::portal::PortalController) -> Result<Value> {
    let args: ActiveTextArgs = typed(args)?;
    ok_text(portal.type_text(&args.text)?)
}

fn portal_press_key_tool(args: Value, portal: &crate::portal::PortalController) -> Result<Value> {
    let args: ActiveKeyArgs = typed(args)?;
    ok_text(portal.press_key(&args.key)?)
}

fn close_tool(args: Value) -> Result<Value> {
    let args: CloseArgs = typed(args)?;
    ok_text(actions::close_session(&args.session_id)?)
}

fn ok_text<T: serde::Serialize>(value: T) -> Result<Value> {
    Ok(json!({
        "content": [{ "type": "text", "text": serde_json::to_string_pretty(&value)? }],
        "isError": false
    }))
}

fn ok_text_result<T: serde::Serialize>(value: Result<T>) -> Result<Value> {
    ok_text(value?)
}

fn parse_params<T: for<'de> Deserialize<'de>>(params: Option<Value>) -> Result<T, Value> {
    serde_json::from_value(params.unwrap_or_else(|| json!({}))).map_err(|error| {
        json!({
            "code": -32602,
            "message": error.to_string()
        })
    })
}

fn typed<T: for<'de> Deserialize<'de>>(args: Value) -> Result<T> {
    serde_json::from_value(args).map_err(Into::into)
}

fn write_response<W: Write>(
    writer: &mut W,
    id: Option<Value>,
    result: Result<Value, Value>,
) -> Result<()> {
    let response = match result {
        Ok(result) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        }),
        Err(error) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": error
        }),
    };
    serde_json::to_writer(&mut *writer, &response)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn tool_definitions() -> Vec<Value> {
    vec![
        tool(
            "check_environment",
            "Report X11, XTEST, screenshot, and isolation dependencies.",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),
        tool(
            "list_sessions",
            "List sessions launched and tracked by Penguin Harness.",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),
        tool(
            "launch_app",
            "Launch an application command and track the best matching window.",
            json!({
                "type": "object",
                "required": ["command"],
                "properties": {
                    "command": { "type": "array", "items": { "type": "string" }, "minItems": 1 },
                    "mode": { "type": "string", "enum": ["real", "isolated"], "default": "real" },
                    "title_hint": { "type": "string" },
                    "wait_ms": { "type": "integer", "minimum": 0 }
                }
            }),
        ),
        tool(
            "launch_terminal",
            "Launch a controllable terminal emulator window. On GNOME Wayland this prefers Ptyxis/Xwayland when available.",
            json!({
                "type": "object",
                "properties": {
                    "mode": { "type": "string", "enum": ["real", "isolated"], "default": "real" },
                    "terminal": { "type": "string", "description": "Optional terminal command override." },
                    "title_hint": { "type": "string" },
                    "cwd": { "type": "string" },
                    "command": { "type": "array", "items": { "type": "string" } },
                    "wait_ms": { "type": "integer", "minimum": 0 }
                }
            }),
        ),
        tool(
            "list_windows",
            "List visible X11 windows.",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),
        tool(
            "find_windows",
            "Find X11 windows by case-insensitive title substring and/or process id.",
            json!({
                "type": "object",
                "properties": {
                    "title_contains": { "type": "string" },
                    "pid": { "type": "integer", "minimum": 1 },
                    "limit": { "type": "integer", "minimum": 1 }
                }
            }),
        ),
        tool(
            "window_info",
            "Return current metadata for a tracked session window or explicit window id.",
            target_schema(),
        ),
        tool(
            "active_window",
            "Return the currently active X11 window id and metadata when available.",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),
        tool(
            "focus_window",
            "Focus a tracked session window or explicit window id.",
            target_schema(),
        ),
        tool(
            "close_window",
            "Ask the window manager to close a tracked session window or explicit window id without killing unrelated processes.",
            target_schema(),
        ),
        tool(
            "move_resize_window",
            "Move and resize a tracked session window or explicit window id.",
            json!({
                "type": "object",
                "required": ["width", "height"],
                "properties": target_properties(json!({
                    "x": { "type": "integer" },
                    "y": { "type": "integer" },
                    "width": { "type": "integer", "minimum": 1 },
                    "height": { "type": "integer", "minimum": 1 }
                }))
            }),
        ),
        tool(
            "screenshot",
            "Capture a target window. Coordinates for later actions are relative to this screenshot.",
            json!({
                "type": "object",
                "properties": target_properties(json!({
                    "include_image": { "type": "boolean", "default": false }
                }))
            }),
        ),
        tool(
            "screenshot_screen",
            "Capture the whole X11 screen. Coordinates for screen-level actions are absolute screen pixels.",
            json!({
                "type": "object",
                "properties": {
                    "include_image": { "type": "boolean", "default": false }
                }
            }),
        ),
        tool(
            "portal_start",
            "Start a Wayland RemoteDesktop/ScreenCast portal session. The first run normally shows a system permission prompt; later runs can reuse the saved restore token when supported.",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),
        tool(
            "portal_status",
            "Return the active Wayland portal session and selected screen stream metadata.",
            json!({
                "type": "object",
                "properties": {}
            }),
        ),
        tool(
            "portal_screenshot",
            "Capture the Wayland desktop through the screenshot portal. Coordinates for portal screen actions are absolute screen pixels.",
            json!({
                "type": "object",
                "properties": {
                    "include_image": { "type": "boolean", "default": false }
                }
            }),
        ),
        tool(
            "portal_click_screen",
            "Click an absolute Wayland screen coordinate through the RemoteDesktop portal.",
            screen_point_schema(),
        ),
        tool(
            "portal_double_click_screen",
            "Double-click an absolute Wayland screen coordinate through the RemoteDesktop portal.",
            screen_point_schema(),
        ),
        tool(
            "portal_drag_screen",
            "Drag between two absolute Wayland screen coordinates through the RemoteDesktop portal.",
            json!({
                "type": "object",
                "required": ["x1", "y1", "x2", "y2"],
                "properties": {
                    "x1": { "type": "integer" },
                    "y1": { "type": "integer" },
                    "x2": { "type": "integer" },
                    "y2": { "type": "integer" },
                    "button": { "type": "string", "enum": ["left", "middle", "right"], "default": "left" }
                }
            }),
        ),
        tool(
            "portal_scroll_screen",
            "Scroll at an absolute Wayland screen coordinate through the RemoteDesktop portal. Positive amount scrolls up; negative scrolls down.",
            json!({
                "type": "object",
                "required": ["x", "y", "amount"],
                "properties": {
                    "x": { "type": "integer" },
                    "y": { "type": "integer" },
                    "amount": { "type": "integer" }
                }
            }),
        ),
        tool(
            "portal_type_text",
            "Type text through the Wayland RemoteDesktop portal into the currently focused surface.",
            json!({
                "type": "object",
                "required": ["text"],
                "properties": {
                    "text": { "type": "string" }
                }
            }),
        ),
        tool(
            "portal_press_key",
            "Press one key through the Wayland RemoteDesktop portal, such as Enter, Escape, Tab, Ctrl-C, Left, Right, F5.",
            json!({
                "type": "object",
                "required": ["key"],
                "properties": {
                    "key": { "type": "string" }
                }
            }),
        ),
        tool(
            "type_text",
            "Type text into the focused target window.",
            json!({
                "type": "object",
                "required": ["text"],
                "properties": target_properties(json!({
                    "text": { "type": "string" }
                }))
            }),
        ),
        tool(
            "type_active",
            "Type text into the currently active X11 window. Use only after intentionally focusing/clicking the target.",
            json!({
                "type": "object",
                "required": ["text"],
                "properties": {
                    "text": { "type": "string" }
                }
            }),
        ),
        tool(
            "press_key",
            "Press one key such as Enter, Escape, Tab, Ctrl-C, Left, Right, F5.",
            json!({
                "type": "object",
                "required": ["key"],
                "properties": target_properties(json!({
                    "key": { "type": "string" }
                }))
            }),
        ),
        tool(
            "press_key_active",
            "Press a key in the currently active X11 window. Use only after intentionally focusing/clicking the target.",
            json!({
                "type": "object",
                "required": ["key"],
                "properties": {
                    "key": { "type": "string" }
                }
            }),
        ),
        tool(
            "click",
            "Click target window coordinates with left, middle, or right button.",
            point_schema(),
        ),
        tool(
            "click_screen",
            "Click an absolute screen coordinate with left, middle, or right button.",
            screen_point_schema(),
        ),
        tool(
            "double_click",
            "Double-click target window coordinates.",
            point_schema(),
        ),
        tool(
            "double_click_screen",
            "Double-click an absolute screen coordinate.",
            screen_point_schema(),
        ),
        tool(
            "drag",
            "Drag between two target-window coordinates.",
            json!({
                "type": "object",
                "required": ["x1", "y1", "x2", "y2"],
                "properties": target_properties(json!({
                    "x1": { "type": "integer" },
                    "y1": { "type": "integer" },
                    "x2": { "type": "integer" },
                    "y2": { "type": "integer" },
                    "button": { "type": "string", "enum": ["left", "middle", "right"], "default": "left" }
                }))
            }),
        ),
        tool(
            "drag_screen",
            "Drag between two absolute screen coordinates.",
            json!({
                "type": "object",
                "required": ["x1", "y1", "x2", "y2"],
                "properties": {
                    "x1": { "type": "integer" },
                    "y1": { "type": "integer" },
                    "x2": { "type": "integer" },
                    "y2": { "type": "integer" },
                    "button": { "type": "string", "enum": ["left", "middle", "right"], "default": "left" }
                }
            }),
        ),
        tool(
            "scroll",
            "Scroll at target-window coordinates. Positive amount scrolls up; negative scrolls down.",
            json!({
                "type": "object",
                "required": ["x", "y", "amount"],
                "properties": target_properties(json!({
                    "x": { "type": "integer" },
                    "y": { "type": "integer" },
                    "amount": { "type": "integer" }
                }))
            }),
        ),
        tool(
            "scroll_screen",
            "Scroll at an absolute screen coordinate. Positive amount scrolls up; negative scrolls down.",
            json!({
                "type": "object",
                "required": ["x", "y", "amount"],
                "properties": {
                    "x": { "type": "integer" },
                    "y": { "type": "integer" },
                    "amount": { "type": "integer" }
                }
            }),
        ),
        tool(
            "close_session",
            "Terminate a launched session and remove its state.",
            json!({
                "type": "object",
                "required": ["session_id"],
                "properties": {
                    "session_id": { "type": "string" }
                }
            }),
        ),
    ]
}

fn tool(name: &str, description: &str, input_schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema
    })
}

fn target_schema() -> Value {
    json!({
        "type": "object",
        "properties": target_properties(json!({}))
    })
}

fn point_schema() -> Value {
    json!({
        "type": "object",
        "required": ["x", "y"],
        "properties": target_properties(json!({
            "x": { "type": "integer" },
            "y": { "type": "integer" },
            "button": { "type": "string", "enum": ["left", "middle", "right"], "default": "left" }
        }))
    })
}

fn screen_point_schema() -> Value {
    json!({
        "type": "object",
        "required": ["x", "y"],
        "properties": {
            "x": { "type": "integer" },
            "y": { "type": "integer" },
            "button": { "type": "string", "enum": ["left", "middle", "right"], "default": "left" }
        }
    })
}

fn target_properties(extra: Value) -> Value {
    let mut properties = json!({
        "session_id": { "type": "string" },
        "window_id": {
            "description": "X11 window id as a decimal number or 0x-prefixed string",
            "oneOf": [{ "type": "integer" }, { "type": "string" }]
        }
    });
    if let (Some(base), Some(extra)) = (properties.as_object_mut(), extra.as_object()) {
        for (key, value) in extra {
            base.insert(key.clone(), value.clone());
        }
    }
    properties
}
