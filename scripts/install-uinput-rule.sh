#!/usr/bin/env bash
set -euo pipefail

add_user_to_input=false
target_user="${SUDO_USER:-${USER:-}}"
original_args=("$@")

while [[ $# -gt 0 ]]; do
  arg="$1"
  case "$arg" in
    --add-user-to-input)
      add_user_to_input=true
      ;;
    --user=*)
      target_user="${arg#--user=}"
      ;;
    --user)
      shift
      if [[ $# -eq 0 ]]; then
        echo "--user requires a username" >&2
        exit 2
      fi
      target_user="$1"
      ;;
    -h|--help)
      cat <<'USAGE'
Usage: install-uinput-rule.sh [--add-user-to-input] [--user USERNAME|--user=USERNAME]

Installs /etc/udev/rules.d/70-penguin-harness-uinput.rules so the active
graphical seat user can open /dev/uinput.

Options:
  --add-user-to-input  Also add USERNAME to the input group. This is useful for
                       dedicated/headless machines, but it is a broad trust
                       grant: input group members can read/inject input.
  --user USERNAME      User to add when --add-user-to-input is supplied.
USAGE
      exit 0
      ;;
    *)
      echo "unknown argument: $arg" >&2
      exit 2
      ;;
  esac
  shift
done

if [[ "${EUID:-$(id -u)}" -ne 0 ]]; then
  exec sudo "$0" "${original_args[@]}"
fi

rule_path="/etc/udev/rules.d/70-penguin-harness-uinput.rules"

cat > "$rule_path" <<'RULE'
# Penguin Harness native unattended input backend.
# TAG+="uaccess" grants the active local seat user access through logind.
# GROUP="input" supports dedicated service users when explicitly added.
KERNEL=="uinput", SUBSYSTEM=="misc", MODE="0660", GROUP="input", TAG+="uaccess"
RULE

modprobe uinput || true
udevadm control --reload-rules
udevadm trigger --sysname-match=uinput || true

if [[ "$add_user_to_input" == true ]]; then
  if [[ -z "$target_user" ]]; then
    echo "could not determine target user for --add-user-to-input" >&2
    exit 1
  fi
  usermod -aG input "$target_user"
  echo "Added $target_user to input group. Log out and back in for group membership to apply."
fi

echo "Installed $rule_path"
echo "Current /dev/uinput permissions:"
ls -l /dev/uinput || true
