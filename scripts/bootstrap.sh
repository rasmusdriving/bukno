#!/bin/sh
# Prepare this checkout for building on a Mac.
#
# Writes .cargo/config.local.toml (ignored by Git) so compiler output goes to the
# internal drive instead of the repository, then reports every location it uses.
# Run it again at any time; it only rewrites that one file.
set -eu

repo="$(cd "$(dirname "$0")/.." && pwd)"
local_config="$repo/.cargo/config.local.toml"
target_dir="${BUKNO_CARGO_TARGET_DIR:-$HOME/Library/Caches/bukno/cargo-target}"

# Finder and fresh shells may not have ~/.cargo/bin on PATH yet.
PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"

if ! command -v rustup >/dev/null 2>&1; then
    echo "rustup is not installed. Install it from https://rustup.rs (it installs to ~/.rustup and ~/.cargo)." >&2
    exit 1
fi

mkdir -p "$target_dir"

developer_dir=""
# The Xcode linker refuses to run until its license is accepted. The Command Line
# Tools work without that step, so use them for Cargo only when Xcode is blocked.
if ! xcrun clang --version >/dev/null 2>&1; then
    if [ -x /Library/Developer/CommandLineTools/usr/bin/clang ]; then
        developer_dir="/Library/Developer/CommandLineTools"
    else
        echo "No working C linker. Accept the Xcode license (sudo xcodebuild -license) or install the Command Line Tools (xcode-select --install)." >&2
        exit 1
    fi
fi

{
    echo "# Written by scripts/bootstrap.sh for this machine. Not committed."
    echo "[build]"
    echo "target-dir = \"$target_dir\""
    if [ -n "$developer_dir" ]; then
        echo
        echo "[env]"
        echo "# Xcode license not accepted on this machine; link with the Command Line Tools."
        echo "DEVELOPER_DIR = \"$developer_dir\""
    fi
} > "$local_config"

echo "Bukno bootstrap"
echo "  repository:      $repo"
echo "  rustup home:     ${RUSTUP_HOME:-$HOME/.rustup}"
echo "  cargo home:      ${CARGO_HOME:-$HOME/.cargo}"
echo "  toolchain:       $(cd "$repo" && rustup show active-toolchain | head -n 1)"
echo "  cargo target:    $target_dir"
if [ -n "$developer_dir" ]; then
    echo "  linker:          $developer_dir (Xcode license not accepted)"
else
    echo "  linker:          $(xcode-select -p)"
fi
echo "  local config:    $local_config"
