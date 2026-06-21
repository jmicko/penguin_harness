#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
install_root="${1:-"${PENGUIN_HARNESS_INSTALL_ROOT:-"$HOME/.local"}"}"
dest="$install_root/bin/penguin-harness"

cargo install --path "$repo_dir" --locked --force --root "$install_root"

echo "Installed penguin-harness to $dest"
echo
echo "Run this inside a project to opt that project into the MCP server:"
echo "  $dest init-project ."
