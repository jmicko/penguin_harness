# Penguin Harness

Linux/X11 desktop automation harness for Codex.

Penguin Harness exposes a small CLI and an MCP stdio server that can launch GUI apps, discover windows, take screenshots, focus windows, type, click, drag, scroll, and close windows. The main target is project-scoped Codex desktop use without installing an MCP server globally for every Codex session.

## Current Scope

Penguin Harness is intentionally X11-first:

- Works best on X11 desktop sessions, including Linux Mint Cinnamon on X11.
- Requires XTEST for keyboard and mouse synthesis.
- Uses native X11 screenshot capture and writes PNG files directly.
- Uses EWMH/NetWM X11 window-manager properties directly for desktop window listing, focusing, and close requests.
- Wayland-only sessions are not a primary target yet because most Wayland compositors intentionally block global input injection.

Run this first on a new machine:

```bash
penguin-harness check
```

The check output reports `DISPLAY`, `XDG_SESSION_TYPE`, X11 connectivity, XTEST, screen size, required commands, and isolated-mode readiness.

## Install

From this checkout, the recommended install is:

```bash
./scripts/install-user.sh
```

Source installs need Rust plus a working system linker. On a fresh Ubuntu install:

```bash
sudo apt install build-essential
```

That script runs:

```bash
cargo install --path . --locked --force --root "$HOME/.local"
```

and installs:

```bash
~/.local/bin/penguin-harness
```

Make sure `~/.local/bin` is on your `PATH`.

You can choose a different Cargo install root:

```bash
./scripts/install-user.sh "$HOME/.cargo"
```

which installs to:

```bash
~/.cargo/bin/penguin-harness
```

If you already have a built release binary and only want to copy that exact executable somewhere, use:

```bash
penguin-harness install --dest ~/.local/bin/penguin-harness --force
```

That copy-based command is useful for release artifacts. For a source checkout, `cargo install --path .` is the cleaner default because Cargo builds the package, installs the declared bin target, respects `Cargo.lock` with `--locked`, and can use the same flow later for `--git` installs.

## Add To One Codex Project

Run this from a project you want to opt in:

```bash
penguin-harness init-project .
```

That creates or updates only that repo's `.codex/config.toml`:

```toml
[mcp_servers.penguin_harness]
command = "/home/YOU/.local/bin/penguin-harness"
args = ["mcp"]
env_vars = ["DISPLAY", "WAYLAND_DISPLAY", "XAUTHORITY", "XDG_SESSION_TYPE", "XDG_CURRENT_DESKTOP", "XDG_RUNTIME_DIR", "DBUS_SESSION_BUS_ADDRESS", "PATH", "SHELL", "HOME"]
startup_timeout_sec = 20
tool_timeout_sec = 120
default_tools_approval_mode = "prompt"
```

This does not modify `~/.codex/config.toml`. Codex only sees Penguin Harness in projects that contain this project-local config.

To preview the config without writing:

```bash
penguin-harness print-config
```

After installing a new binary version, restart the Codex session for that project so the MCP server starts from the updated executable and advertises any new tools.

## Runtime Dependencies

Required for real desktop control:

- X11 session or a usable XWayland display exposed through `DISPLAY`
- X11 auth through `XAUTHORITY` when the display requires it
- XTEST extension
- An EWMH-compatible X11 window manager for high-level window listing, focus, and close requests

Optional for isolated mode:

```bash
sudo apt install xvfb openbox
```

`Xephyr`, `fluxbox`, `i3`, or `matchbox-window-manager` can also satisfy the isolated-mode pieces.

## CLI Examples

```bash
penguin-harness check
penguin-harness windows
penguin-harness find-windows --title-contains signal
penguin-harness active-window
penguin-harness window-info --window-id 0x5600004
penguin-harness screenshot --window-id 0x5600004
penguin-harness screenshot-screen
penguin-harness focus --window-id 0x5600004
penguin-harness type --window-id 0x5600004 "hello"
penguin-harness key --window-id 0x5600004 Enter
penguin-harness click --window-id 0x5600004 --x 100 --y 80
penguin-harness close-window --window-id 0x5600004
```

Launch and track a new terminal:

```bash
penguin-harness terminal --cwd "$PWD"
penguin-harness terminal --title-hint penguin-demo -- bash -lc 'printf "hello\n"; exec bash'
```

Coordinates for `click`, `double-click`, `drag`, and `scroll` are relative to the target window screenshot. Screen-level commands such as `screenshot-screen` and `click-screen` use absolute screen coordinates.

## MCP Tools

The server exposes:

- `check_environment`
- `list_sessions`
- `launch_app`
- `launch_terminal`
- `list_windows`
- `find_windows`
- `window_info`
- `active_window`
- `focus_window`
- `close_window`
- `screenshot`
- `screenshot_screen`
- `type_text`
- `type_active`
- `press_key`
- `press_key_active`
- `click`
- `click_screen`
- `double_click`
- `double_click_screen`
- `drag`
- `drag_screen`
- `scroll`
- `scroll_screen`
- `close_session`

## Practical Codex Flow

1. Start with `check_environment`.
2. Use `list_sessions`, `list_windows`, `find_windows`, or `launch_app` to identify the target.
3. Take a `screenshot` with `include_image: true`.
4. Use screenshot-relative coordinates for clicks, drags, and scrolls.
5. Verify visible state with another screenshot after each meaningful UI action.
6. Use `close_window` for existing user apps and `close_session` for apps launched by the harness.

## Notes For Desktop Switching

On a new computer or desktop environment, the first branch point is the session type:

- `XDG_SESSION_TYPE=x11`: expected path; verify XTEST and EWMH behavior with `penguin-harness check` and `penguin-harness windows`.
- `XDG_SESSION_TYPE=wayland` with `DISPLAY` and `XAUTHORITY` set: some XWayland windows may be controllable, but global desktop control is compositor-dependent.
- `XDG_SESSION_TYPE=wayland` without `DISPLAY`: this harness will not control the desktop in its current form.

GNOME on Wayland commonly runs Xwayland with an auth file like `/run/user/1000/.mutter-Xwaylandauth.*`. If `penguin-harness check` reports that `XAUTHORITY` is unset but lists an Xwayland auth candidate, launch Codex from the graphical terminal or export that file as `XAUTHORITY` for SSH-based testing.

Rootless Xwayland can expose individual X11/Xwayland app windows while rejecting root-screen screenshots. In that setup, use `screenshot` on a specific Xwayland window rather than `screenshot_screen`.

Terminal launch support includes `x-terminal-emulator`, Ptyxis, MATE Terminal, GNOME Terminal, XFCE Terminal, xterm, kitty, Alacritty, Konsole, and WezTerm. On GNOME Wayland, Penguin Harness prefers Ptyxis when present and launches it as a standalone GTK/X11 app so XTEST can control it. Unknown terminals can still be launched, but Penguin Harness only passes `cwd` or command arguments to terminals whose flags it knows.
