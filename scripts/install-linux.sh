#!/bin/sh
# Install the current native app and an application-menu entry on this computer.
# Run from the internal working copy via devbox-local. --debug skips a release build.
set -eu

if [ "$(uname -s)" != Linux ]; then
    echo "This installer is for Linux." >&2
    exit 1
fi
profile=release
case "${1:-}" in
    "") ;;
    --debug) profile=debug ;;
    *) echo "usage: sh scripts/install-linux.sh [--debug]" >&2; exit 1 ;;
esac
repo="$(cd "$(dirname "$0")/.." && pwd)"
PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$HOME/.local/bin:$PATH"
export PATH
CARGO_TARGET_DIR="${BUKNO_CARGO_TARGET_DIR:-$HOME/.cache/bukno/cargo-target}"
export CARGO_TARGET_DIR
if [ "$profile" = release ]; then
    cargo build --locked --manifest-path "$repo/Cargo.toml" -p bukno-desktop --release
else
    cargo build --locked --manifest-path "$repo/Cargo.toml" -p bukno-desktop
fi

app_dir="$HOME/.local/share/bukno/bin"
mkdir -p "$app_dir" "$HOME/.local/bin" "$HOME/.local/share/applications"
# Replace the installed binary atomically so an open app keeps its original inode.
install -m 755 "$CARGO_TARGET_DIR/$profile/bukno" "$app_dir/bukno.new"
mv -f "$app_dir/bukno.new" "$app_dir/bukno"
cat > "$HOME/.local/bin/bukno" <<'LAUNCHER'
#!/bin/sh
PATH="$HOME/.local/bin:$PATH"
export PATH
exec "$HOME/.local/share/bukno/bin/bukno" "$@"
LAUNCHER
chmod 755 "$HOME/.local/bin/bukno"
cat > "$HOME/.local/share/applications/io.github.rasmusdriving.bukno.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Bukno
Comment=Native interface for coding agents
Exec="$HOME/.local/bin/bukno"
TryExec=$HOME/.local/bin/bukno
Terminal=false
Categories=Development;
StartupNotify=true
StartupWMClass=io.github.rasmusdriving.bukno
DESKTOP
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$HOME/.local/share/applications"
fi
echo "Installed Bukno ($profile). Open Bukno from Applications, or run $HOME/.local/bin/bukno."
