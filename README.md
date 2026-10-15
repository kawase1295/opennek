# OpenNEK — Open Network Emulator Kit

**English** | [日本語](README.ja.md)

An **LPWAN (Sigfox-style) network emulator** that runs on two ESP32 boards and a PC.
Devices send 12-byte uplinks, which you can watch in a web UI on your PC, forward to your own
application through HTTP callbacks, and answer with 8-byte downlinks.
Develop and test IoT applications locally — no cloud subscription or radio license required.

```
┌────────────┐  ESP-NOW (2.4GHz)  ┌──────────────┐   USB serial   ┌───────────────────────┐  HTTP   ┌──────────┐
│ Device      │ ─ uplink ×3 ────▶ │ Gateway      │ ─ JSON Lines ─▶│ opennek (Bun + Hono)  │ ──────▶ │ Your     │
│ ESP32       │ ◀──── downlink ── │ ESP32 (USB)  │ ◀───────────── │ Web UI / SQLite       │ ◀────── │ app      │
│ AT modem    │                    └──────────────┘                └───────────────────────┘         └──────────┘
└────────────┘
```

| Part | Contents |
|---|---|
| `host/` | Host server. Bun + Hono + bun:sqlite. Web UI, callbacks, downlinks, simulator |
| `firmware/` | PlatformIO (Arduino). `gateway` = USB bridge, `device` = Sigfox-style AT modem emulator |
| `docs/protocol.md` | Radio frame, serial and callback specification |
| `packaging/build-deb.sh` | Builds a `.deb` for Ubuntu |

> **Relation to Sigfox**: the radio is ESP-NOW, so **OpenNEK cannot receive real Sigfox modules**.
> Instead, the AT commands and the callback format follow Sigfox conventions, so most Sigfox
> applications and tools can be tried as-is.

---

## Quick start (no hardware)

```bash
cd host
bun install
bun run sim
```

Open <http://127.0.0.1:8085/>. The messages page shows a simulator form for sending uplinks.
The web UI is currently in Japanese.

## Running with real hardware

### 1. What you need

- Two ESP32 development boards (ESP32 DevKit, ESP32-S3, … — anything that supports ESP-NOW)
  - Board 1: **gateway** (stays plugged into the PC over USB)
  - Board 2: **device** (powered from a USB battery or similar)

### 2. Flash the firmware

```bash
cd firmware
pio run -e gateway -t upload --upload-port /dev/ttyUSB0
pio run -e device  -t upload --upload-port /dev/ttyUSB1
```

For ESP32-S3 boards use `gateway-s3` / `device-s3`. To flash without PlatformIO (e.g. from a browser),
see [docs/flashing.md](docs/flashing.md).

### 3. Start the host

```bash
cd host && bun run start          # or, with the .deb installed: opennek --open
```

The gateway is detected automatically (the badge in the top right shows "接続中 · ch 1" when connected).
If you get a permission error on the serial port, run `sudo usermod -aG dialout $USER` and log in again.

### 4. Register the device

On the device's USB serial port (115200 bps):

```
AT$ID      → 0A1B2C3D                          (device ID)
AT$PAC     → 3F2A... (32 hex digits)           (device key)
AT$SF=0102030405
OK
```

After the first uplink, the device appears in the device list as unregistered. Click **編集** (Edit) and
enter the `AT$PAC` value as the key; authentication then shows OK and downlinks are enabled.

---

## Device (AT modem emulator)

Commands are accepted on both USB serial (115200 bps) and a UART (**GPIO16 = RX, GPIO17 = TX, 9600 bps**).
An existing Sigfox AT terminal can be rewired to this ESP32's UART and keep working.
Pressing the **BOOT button** sends the uptime in seconds (4 bytes) as a demo uplink.

| Command | Response | Description |
|---|---|---|
| `AT` / `ATZ` | `OK` | |
| `AT$SF=<hex>` | `OK` | Send 0–12 bytes |
| `AT$SF=<hex>,1` | `OK` `RX=01 02 03 04 05 06 07 08` | Request a downlink (`ERROR: downlink timeout` if none arrives) |
| `AT$SB=<0\|1>[,1]` | `OK` | Send a single bit (as one byte, `00` / `01`) |
| `AT$SO` | `OK` | OOB (keep-alive) frame |
| `AT$ID` / `AT$I=10` | `0A1B2C3D` | Device ID (lower 4 bytes of the MAC address) |
| `AT$PAC` / `AT$I=11` | 32 hex digits | Device key (register it on the host) |
| `AT$CH?` / `AT$CH=<1-13>` | | ESP-NOW channel (must match the gateway) |
| `AT$KEY=<32 hex>` | `OK` | Replace the key |
| `AT$DLW?` / `AT$DLW=<ms>` | | Downlink wait time (default 8000 ms) |
| `AT$SEQ?` | | Next sequence number |
| `AT$T?` / `AT$V?` | | Temperature (0.1 °C units) / voltage (fixed 3300) |
| `AT$RC…` `AT$P=…` `AT+MODE…` `ATS3…` | `OK` | Settings commands of real modems; accepted and ignored |

To use it from your own sketch, use the `firmware/lib/OpenNEK` library directly:

```cpp
#include <OpenNEK.h>
onek::Device dev;
void setup() { dev.begin(); }
void loop() {
  uint8_t data[] = {0x12, 0x34}, dl[8];
  if (dev.send(data, sizeof(data), dl) == onek::SendResult::Ok) { /* 8 bytes in dl */ }
  delay(60000);
}
```

## Callbacks

Configure the URL, headers and body in the **コールバック** (Callbacks) tab. The body supports the same
variables as Sigfox custom callbacks (`{device}` `{time}` `{data}` `{seqNumber}` `{ack}` `{rssi}` `{snr}`
`{station}` `{customData#name}`, … — full list in [docs/protocol.md](docs/protocol.md)).

```json
{"device":"{device}","time":{time},"data":"{data}","seqNumber":{seqNumber},"rssi":{rssi}}
```

- **Payload format** (per device): with something like `temp::int:16:little-endian hum::uint:8 door:0:bool:7`,
  decoded values are shown in the UI and are available in callbacks as `{customData#temp}`.
- **Downlinks**: set the device's downlink mode to "callback response" and enable
  "extract downlink data from the response" on the callback. If the response is
  `{"0A1B2C3D":{"downlinkData":"0102030405060708"}}`, those 8 bytes are sent to the device.
- The **テスト送信** (Test) button fires the callback with the latest message (not recorded in the logs).

## Using it on WSL2

WSL cannot see USB devices by default. Use [usbipd-win](https://github.com/dorssel/usbipd-win) on Windows
to attach the gateway to WSL (run in an elevated PowerShell):

```powershell
usbipd list
usbipd bind --busid <BUSID>
usbipd attach --wsl --busid <BUSID>
```

The web UI can be opened from a Windows browser at <http://localhost:8085/>.

## Building the .deb

```bash
./packaging/build-deb.sh     # → dist/opennek_0.1.0_amd64.deb
sudo apt install ./dist/opennek_0.1.0_amd64.deb
opennek --open               # or: systemctl --user enable --now opennek
```

The host is compiled into a single binary with `bun build --compile`, so neither Bun nor Node is needed to run it.
Merged firmware images are installed to `/usr/share/opennek/firmware/`. Data is stored in `~/.local/share/opennek/`.

## Development

```bash
(cd host && bun test && bun run typecheck)
(cd firmware && pio run)
(cd firmware/test && uvx --from ziglang python -m ziglang c++ -std=c++17 -I ../lib/OpenNEK/src \
  frame_native_test.cpp ../lib/OpenNEK/src/OpenNekFrame.cpp -o frame_test && ./frame_test)
```

The host tests include a simulator end-to-end test (uplink → callback → downlink) and a gateway
serial test over a socat virtual serial port. The firmware and host frame implementations are checked
against the same golden vector.

## License

MIT
