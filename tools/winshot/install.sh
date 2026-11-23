#!/usr/bin/env bash
# Registers target/release/winshot with KDE as allowed to use KWin's screenshot interface
# (org.kde.KWin.ScreenShot2), so it can capture the obsidilight window during UI checks.
# Only this one binary is trusted; it captures a single window chosen by process id.
# Undo: rm ~/.local/share/applications/dev.obsidilight.winshot.desktop && kbuildsycoca6
set -euo pipefail
cd "$(dirname "$0")/../.."
BIN="$(pwd)/target/release/winshot"
[ -x "$BIN" ] || cargo build --release -p winshot
DEST="$HOME/.local/share/applications/dev.obsidilight.winshot.desktop"
mkdir -p "$(dirname "$DEST")"
cat > "$DEST" <<DESKTOP
[Desktop Entry]
Type=Application
Name=obsidilight winshot
Comment=Captures the obsidilight window for its UI checks
Exec=$BIN
NoDisplay=true
X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2
DESKTOP
kbuildsycoca6 --noincremental >/dev/null 2>&1 || true
echo "installed $DEST"
