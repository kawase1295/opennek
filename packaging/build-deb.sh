#!/usr/bin/env bash
# OpenNEK の .deb を作る: ホスト (単一バイナリ)
#   ./packaging/build-deb.sh            → dist/opennek_<ver>_amd64.deb
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
VERSION=$(sed -n "s/.*VERSION = '\(.*\)'.*/\1/p" "$ROOT/host/src/version.ts")
OUT="$ROOT/dist"
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT

echo "==> host"
(cd "$ROOT/host" && bun install --frozen-lockfile >/dev/null && bun test && bun run build)
install -Dm755 "$ROOT/host/dist/opennek" "$STAGE/usr/bin/opennek"

install -Dm644 /dev/stdin "$STAGE/usr/share/applications/opennek.desktop" <<DESKTOP
[Desktop Entry]
Name=OpenNEK
Comment=Local network for real Sigfox devices
Comment[ja]=本物の Sigfox デバイスのためのローカルネットワーク
Exec=opennek --open
Terminal=true
Type=Application
Categories=Network;Development;
DESKTOP

install -Dm644 /dev/stdin "$STAGE/usr/lib/systemd/user/opennek.service" <<SERVICE
[Unit]
Description=OpenNEK

[Service]
ExecStart=/usr/bin/opennek
Restart=on-failure

[Install]
WantedBy=default.target
SERVICE

install -Dm644 "$ROOT/LICENSE" "$STAGE/usr/share/doc/opennek/copyright"
install -Dm644 "$ROOT/docs/protocol.md" "$STAGE/usr/share/doc/opennek/protocol.md"
install -Dm644 "$ROOT/docs/protocol.ja.md" "$STAGE/usr/share/doc/opennek/protocol.ja.md"

mkdir -p "$STAGE/DEBIAN"
cat > "$STAGE/DEBIAN/control" <<CONTROL
Package: opennek
Version: $VERSION
Architecture: amd64
Maintainer: OpenNEK contributors
Installed-Size: $(du -sk "$STAGE/usr" | cut -f1)
Recommends: xdg-utils
Section: net
Priority: optional
Description: Local network for real Sigfox devices
 Receives uplinks from real Sigfox (RC3) devices through a USB receiver,
 shows them in a web UI, forwards them to HTTP callbacks compatible with
 Sigfox custom callbacks and sends downlinks back.
CONTROL

mkdir -p "$OUT"
DEB="$OUT/opennek_${VERSION}_amd64.deb"
dpkg-deb --root-owner-group -Zxz --build "$STAGE" "$DEB" >/dev/null
echo "==> $DEB ($(du -h "$DEB" | cut -f1))"
