#!/bin/sh
# Install wake on macOS (Apple Silicon) and keep it updated daily.
#
#   curl -fsSL https://raw.githubusercontent.com/dantebarbieri/wake/main/install/install.sh | sudo sh
#   sudo sh install.sh --uninstall
#
# Installs /usr/local/bin/wake, removes the old /usr/local/bin/wake-pc, and
# loads a root LaunchDaemon that runs `wake self-update --quiet` every day at
# 04:15. Linux machines with Nix get wake from the flake instead.
set -eu

BIN=/usr/local/bin/wake
LABEL=com.danteb.wake-update
PLIST=/Library/LaunchDaemons/$LABEL.plist
LOG=/var/log/wake-update.log
URL=https://github.com/dantebarbieri/wake/releases/latest/download/wake-aarch64-apple-darwin.tar.gz

die() { printf 'install.sh: %s\n' "$*" >&2; exit 1; }

[ "$(uname -s)" = Darwin ] || die "this installer is for macOS; use the Nix flake on Linux"
[ "$(uname -m)" = arm64 ] || die "only Apple Silicon builds are published"
[ "$(id -u)" -eq 0 ] || die "run with sudo"

if [ "${1:-}" = --uninstall ]; then
  launchctl bootout "system/$LABEL" 2>/dev/null || true
  rm -f "$PLIST" "$BIN"
  echo "wake removed (log left at $LOG)"
  exit 0
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl -fsSL "$URL" -o "$tmp/wake.tar.gz"
tar -xzf "$tmp/wake.tar.gz" -C "$tmp"
mkdir -p /usr/local/bin
install -m 0755 -o root -g wheel "$tmp/wake" "$BIN"
rm -f /usr/local/bin/wake-pc

cat >"$PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>$LABEL</string>
  <key>ProgramArguments</key>
  <array>
    <string>$BIN</string>
    <string>self-update</string>
    <string>--quiet</string>
  </array>
  <key>StartCalendarInterval</key>
  <dict>
    <key>Hour</key><integer>4</integer>
    <key>Minute</key><integer>15</integer>
  </dict>
  <key>StandardOutPath</key><string>$LOG</string>
  <key>StandardErrorPath</key><string>$LOG</string>
</dict>
</plist>
EOF
chown root:wheel "$PLIST"
chmod 0644 "$PLIST"
launchctl bootout "system/$LABEL" 2>/dev/null || true
launchctl bootstrap system "$PLIST"

"$BIN" --version
echo "daily update: $PLIST (log: $LOG)"
