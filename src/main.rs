mod actions;
mod cli;
mod env_check;
mod mcp;
mod portal;
mod project;
mod screenshot;
mod session;
mod terminal;
mod types;
mod window;
mod x11_control;

use anyhow::Result;

fn main() -> Result<()> {
    cli::run()
}
