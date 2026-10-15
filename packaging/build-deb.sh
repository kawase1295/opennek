#!/usr/bin/env bash
# OpenNEK の .deb を作る: ホスト (単一バイナリ) + ファームウェア (書き込み用の統合 .bin)
#   ./packaging/build-deb.sh            → dist/opennek_<ver>_amd64.deb
#   SKIP_FIRMWARE=1 ./packaging/build-deb.sh  ファームを含めない
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
VERSION=$(sed -n "s/.*VERSION = '\(.*\)'.*/\1/p" "$ROOT/host/src/version.ts")
OUT="$ROOT/dist"
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
PIO=${PIO:-$(command -v pio || echo "$HOME/.platformio/penv/bin/pio")}
ESPTOOL="$HOME/.platformio/packages/tool-esptoolpy/esptool.py"
PIO_PY="$HOME/.platformio/penv/bin/python"

echo "==> host"
(cd "$ROOT/host" && bun install --frozen-lockfile >/dev/null && bun test && bun run build)
install -Dm755 "$ROOT/host/dist/opennek" "$STAGE/usr/bin/opennek"

if [[ -z "${SKIP_FIRMWARE:-}" ]]; then
  echo "==> firmware"
  (cd "$ROOT/firmware" && "$PIO" run -e gateway -e device -e gateway-s3 -e device-s3)
  FWDIR="$STAGE/usr/share/opennek/firmware"
  mkdir -p "$FWDIR"
  for env in gateway device gateway-s3 device-s3; do
    B="$ROOT/firmware/.pio/build/$env"
    if [[ $env == *-s3 ]]; then chip=esp32s3 boot=0x0; else chip=esp32 boot=0x1000; fi
    BOOT_APP0="$HOME/.platformio/packages/framework-arduinoespressif32/tools/partitions/boot_app0.bin"
    "$PIO_PY" "$ESPTOOL" --chip $chip merge_bin -o "$FWDIR/opennek-$env-$VERSION.bin" \
      $boot "$B/bootloader.bin" 0x8000 "$B/partitions.bin" 0xe000 "$BOOT_APP0" 0x10000 "$B/firmware.bin" >/dev/null
  done
  cp "$ROOT/docs/flashing.md" "$ROOT/docs/flashing.ja.md" "$FWDIR/"
fi

install -Dm644 /dev/stdin "$STAGE/usr/share/applications/opennek.desktop" <<DESKTOP
[Desktop Entry]
Name=OpenNEK
Comment=ESP32 で作るオープンな LPWAN ネットワークエミュレータ
Exec=opennek --open
Terminal=true
Type=Application
Categories=Network;Development;
DESKTOP

install -Dm644 /dev/stdin "$STAGE/usr/lib/systemd/user/opennek.service" <<SERVICE
[Unit]
Description=OpenNEK network emulator

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
Description: Open LPWAN network emulator kit for ESP32
 Receives uplinks from ESP32 devices through an ESP32 USB gateway (ESP-NOW),
 shows them in a web UI, forwards them to HTTP callbacks compatible with
 Sigfox custom callbacks and sends downlinks back.
CONTROL

mkdir -p "$OUT"
DEB="$OUT/opennek_${VERSION}_amd64.deb"
dpkg-deb --root-owner-group -Zxz --build "$STAGE" "$DEB" >/dev/null
echo "==> $DEB ($(du -h "$DEB" | cut -f1))"
