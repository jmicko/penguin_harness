use crate::actions::{self, LaunchRequest, TargetRequest, WindowQuery};
use crate::terminal::TerminalRequest;
use crate::types::{Mode, MouseButton, WindowId};
use anyhow::{Result, bail};
use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(version, about = "Linux desktop automation harness for Codex")]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Check,
    Mcp,
    Install(InstallArgs),
    InitProject(InitProjectArgs),
    PrintConfig(PrintConfigArgs),
    Sessions,
    Windows,
    FindWindows(FindWindowsArgs),
    WindowInfo(TargetArgs),
    ActiveWindow,
    MoveResize(MoveResizeArgs),
    Launch(LaunchArgs),
    Terminal(TerminalArgs),
    Screenshot(TargetArgs),
    ScreenshotScreen,
    NativeCheck,
    NativeScreenshot(NativeScreenshotArgs),
    NativeType(ActiveTypeArgs),
    NativeKey(ActiveKeyArgs),
    NativeClickScreen(ScreenPointArgs),
    NativeDoubleClickScreen(ScreenPointArgs),
    NativeDragScreen(ScreenDragArgs),
    NativeScrollScreen(ScreenScrollArgs),
    Focus(TargetArgs),
    CloseWindow(TargetArgs),
    Type(TypeArgs),
    TypeActive(ActiveTypeArgs),
    Key(KeyArgs),
    KeyActive(ActiveKeyArgs),
    Click(PointArgs),
    ClickScreen(ScreenPointArgs),
    DoubleClick(PointArgs),
    DoubleClickScreen(ScreenPointArgs),
    Drag(DragArgs),
    DragScreen(ScreenDragArgs),
    Scroll(ScrollArgs),
    ScrollScreen(ScreenScrollArgs),
    Close { session_id: String },
}

#[derive(Debug, Args)]
struct InstallArgs {
    #[arg(long)]
    dest: Option<PathBuf>,
    #[arg(long)]
    force: bool,
}

#[derive(Debug, Args)]
struct InitProjectArgs {
    #[arg(default_value = ".")]
    path: PathBuf,
    #[arg(long)]
    command: Option<String>,
    #[arg(long)]
    force: bool,
}

#[derive(Debug, Args)]
struct PrintConfigArgs {
    #[arg(long)]
    command: Option<String>,
}

#[derive(Debug, Args)]
struct FindWindowsArgs {
    #[arg(long)]
    title_contains: Option<String>,
    #[arg(long)]
    pid: Option<u32>,
    #[arg(long)]
    limit: Option<usize>,
}

#[derive(Debug, Args)]
struct LaunchArgs {
    #[arg(long, value_enum, default_value_t = Mode::Real)]
    mode: Mode,
    #[arg(long)]
    title_hint: Option<String>,
    #[arg(long)]
    wait_ms: Option<u64>,
    #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
    command: Vec<String>,
}

#[derive(Debug, Args)]
struct TerminalArgs {
    #[arg(long, value_enum, default_value_t = Mode::Real)]
    mode: Mode,
    #[arg(long)]
    terminal: Option<String>,
    #[arg(long)]
    title_hint: Option<String>,
    #[arg(long)]
    cwd: Option<PathBuf>,
    #[arg(long)]
    wait_ms: Option<u64>,
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    command: Vec<String>,
}

#[derive(Debug, Args)]
struct TargetArgs {
    #[arg(long)]
    session_id: Option<String>,
    #[arg(long)]
    window_id: Option<WindowId>,
}

#[derive(Debug, Args)]
struct MoveResizeArgs {
    #[command(flatten)]
    target: TargetArgs,
    #[arg(long)]
    x: Option<i32>,
    #[arg(long)]
    y: Option<i32>,
    #[arg(long)]
    width: u32,
    #[arg(long)]
    height: u32,
}

#[derive(Debug, Args)]
struct TypeArgs {
    #[command(flatten)]
    target: TargetArgs,
    text: String,
}

#[derive(Debug, Args)]
struct KeyArgs {
    #[command(flatten)]
    target: TargetArgs,
    key: String,
}

#[derive(Debug, Args)]
struct ActiveTypeArgs {
    text: String,
}

#[derive(Debug, Args)]
struct ActiveKeyArgs {
    key: String,
}

#[derive(Debug, Args)]
struct PointArgs {
    #[command(flatten)]
    target: TargetArgs,
    #[arg(long)]
    x: i32,
    #[arg(long)]
    y: i32,
    #[arg(long, default_value = "left")]
    button: ButtonArg,
}

#[derive(Debug, Args)]
struct ScreenPointArgs {
    #[arg(long)]
    x: i32,
    #[arg(long)]
    y: i32,
    #[arg(long, default_value = "left")]
    button: ButtonArg,
}

#[derive(Debug, Args)]
struct DragArgs {
    #[command(flatten)]
    target: TargetArgs,
    #[arg(long)]
    x1: i32,
    #[arg(long)]
    y1: i32,
    #[arg(long)]
    x2: i32,
    #[arg(long)]
    y2: i32,
    #[arg(long, default_value = "left")]
    button: ButtonArg,
}

#[derive(Debug, Args)]
struct ScreenDragArgs {
    #[arg(long)]
    x1: i32,
    #[arg(long)]
    y1: i32,
    #[arg(long)]
    x2: i32,
    #[arg(long)]
    y2: i32,
    #[arg(long, default_value = "left")]
    button: ButtonArg,
}

#[derive(Debug, Args)]
struct ScrollArgs {
    #[command(flatten)]
    target: TargetArgs,
    #[arg(long)]
    x: i32,
    #[arg(long)]
    y: i32,
    #[arg(long, default_value_t = -3)]
    amount: i32,
}

#[derive(Debug, Args)]
struct ScreenScrollArgs {
    #[arg(long)]
    x: i32,
    #[arg(long)]
    y: i32,
    #[arg(long, default_value_t = -3)]
    amount: i32,
}

#[derive(Debug, Args)]
struct NativeScreenshotArgs {
    #[arg(long)]
    include_cursor: bool,
}

#[derive(Clone, Debug)]
struct ButtonArg(MouseButton);

impl std::str::FromStr for ButtonArg {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let button = match value.to_ascii_lowercase().as_str() {
            "left" | "1" => MouseButton::Left,
            "middle" | "2" => MouseButton::Middle,
            "right" | "3" => MouseButton::Right,
            _ => bail!("button must be left, middle, or right"),
        };
        Ok(Self(button))
    }
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Check => print_json(actions::check_environment()),
        Command::Mcp => crate::mcp::serve(),
        Command::Install(args) => print_json(crate::project::install_current_binary(
            args.dest, args.force,
        )?),
        Command::InitProject(args) => print_json(crate::project::init_project(
            args.path,
            args.command,
            args.force,
        )?),
        Command::PrintConfig(args) => {
            let command = args
                .command
                .unwrap_or_else(crate::project::default_installed_command);
            println!("{}", crate::project::mcp_config(&command));
            Ok(())
        }
        Command::Sessions => print_json(crate::session::list()?),
        Command::Windows => print_json(actions::list_windows()?),
        Command::FindWindows(args) => print_json(actions::find_windows(WindowQuery {
            title_contains: args.title_contains,
            pid: args.pid,
            limit: args.limit,
        })?),
        Command::WindowInfo(args) => print_json(actions::window_info(args.into())?),
        Command::ActiveWindow => print_json(actions::active_window()?),
        Command::MoveResize(args) => {
            print_json(actions::move_resize_window(actions::MoveResizeRequest {
                session_id: args.target.session_id,
                window_id: args.target.window_id,
                x: args.x,
                y: args.y,
                width: args.width,
                height: args.height,
            })?)
        }
        Command::Launch(args) => print_json(actions::launch_app(LaunchRequest {
            command: args.command,
            mode: args.mode,
            title_hint: args.title_hint,
            wait_ms: args.wait_ms,
        })?),
        Command::Terminal(args) => print_json(crate::terminal::launch_terminal(TerminalRequest {
            command: (!args.command.is_empty()).then_some(args.command),
            mode: args.mode,
            terminal: args.terminal,
            title_hint: args.title_hint,
            cwd: args.cwd,
            wait_ms: args.wait_ms,
        })?),
        Command::Screenshot(args) => print_json(actions::screenshot(args.into())?),
        Command::ScreenshotScreen => print_json(actions::screenshot_screen()?),
        Command::NativeCheck => print_json(crate::native::check()),
        Command::NativeScreenshot(args) => {
            print_json(crate::native_screenshot::capture(args.include_cursor)?)
        }
        Command::NativeType(args) => print_json(crate::native_input::with_device(|device| {
            device.type_text(&args.text)
        })?),
        Command::NativeKey(args) => print_json(crate::native_input::with_device(|device| {
            device.press_key(&args.key)
        })?),
        Command::NativeClickScreen(args) => {
            print_json(crate::native_input::with_device(|device| {
                device.click_screen(args.x, args.y, args.button.0)
            })?)
        }
        Command::NativeDoubleClickScreen(args) => {
            print_json(crate::native_input::with_device(|device| {
                device.double_click_screen(args.x, args.y, args.button.0)
            })?)
        }
        Command::NativeDragScreen(args) => {
            print_json(crate::native_input::with_device(|device| {
                device.drag_screen(args.x1, args.y1, args.x2, args.y2, args.button.0)
            })?)
        }
        Command::NativeScrollScreen(args) => {
            print_json(crate::native_input::with_device(|device| {
                device.scroll_screen(args.x, args.y, args.amount)
            })?)
        }
        Command::Focus(args) => print_json(actions::focus(args.into())?),
        Command::CloseWindow(args) => print_json(actions::close_window(args.into())?),
        Command::Type(args) => print_json(actions::type_text(args.target.into(), &args.text)?),
        Command::TypeActive(args) => print_json(actions::type_text_active(&args.text)?),
        Command::Key(args) => print_json(actions::press_key(args.target.into(), &args.key)?),
        Command::KeyActive(args) => print_json(actions::press_key_active(&args.key)?),
        Command::Click(args) => print_json(actions::click(
            args.target.into(),
            args.x,
            args.y,
            args.button.0,
        )?),
        Command::ClickScreen(args) => {
            print_json(actions::click_screen(args.x, args.y, args.button.0)?)
        }
        Command::DoubleClick(args) => print_json(actions::double_click(
            args.target.into(),
            args.x,
            args.y,
            args.button.0,
        )?),
        Command::DoubleClickScreen(args) => {
            print_json(actions::double_click_screen(args.x, args.y, args.button.0)?)
        }
        Command::Drag(args) => print_json(actions::drag(
            args.target.into(),
            args.x1,
            args.y1,
            args.x2,
            args.y2,
            args.button.0,
        )?),
        Command::DragScreen(args) => print_json(actions::drag_screen(
            args.x1,
            args.y1,
            args.x2,
            args.y2,
            args.button.0,
        )?),
        Command::Scroll(args) => print_json(actions::scroll(
            args.target.into(),
            args.x,
            args.y,
            args.amount,
        )?),
        Command::ScrollScreen(args) => {
            print_json(actions::scroll_screen(args.x, args.y, args.amount)?)
        }
        Command::Close { session_id } => print_json(actions::close_session(&session_id)?),
    }
}

impl From<TargetArgs> for TargetRequest {
    fn from(value: TargetArgs) -> Self {
        Self {
            session_id: value.session_id,
            window_id: value.window_id,
        }
    }
}

fn print_json<T: Serialize>(value: T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}
