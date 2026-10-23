# OpenNEK — Open Network Emulator Kit

**English** | [日本語](README.ja.md)

A **local network for real Sigfox devices** (RC3, Japan) that runs on a PC. A Seeed Studio
**XIAO ESP32S3 & Wio-SX1262 kit** works as the radio, in place of an SDR dongle.
Uplinks from your devices show up in a web UI, are forwarded to your own application through
Sigfox-compatible HTTP callbacks, and can be answered with 8-byte downlinks.
Develop and test Sigfox applications on your desk, without a backend subscription.

```
┌──────────────┐  Sigfox RC3 (923.2 MHz)   ┌──────────────┐  USB serial  ┌───────────────────────┐  HTTP   ┌──────────┐
│ Sigfox device │ ─ uplink ×3 ───────────▶ │ XIAO ESP32S3 │ ─ RSSI/CFO ─▶ │ opennek (Rust)        │ ──────▶ │ Your     │
│               │ ◀──── downlink (922 MHz) │ + Wio-SX1262 │ ◀──── TX ──── │ Web UI / SQLite       │ ◀────── │ app      │
└──────────────┘                            └──────────────┘               └───────────────────────┘         └──────────┘
```

| Part | Contents |
|---|---|
| `host/` | Host server, a single Rust executable (axum, SQLite). Sigfox decoder, web UI (`host/ui`, embedded), callbacks, downlinks |
| `tools/lsm/` | Node tools for the LSM100A receiver: flashing its firmware through the bridge, RSSI/CFO captures |
| `firmware/` | PlatformIO (Arduino) firmware. `radio-xiao-sx1262` = the radio: streams RSSI from the Wio-SX1262 to the host and sends downlinks |
| `docs/protocol.md` | Callback and downlink specification |

---

## Getting started

1. Plug the XIAO ESP32S3 onto the Wio-SX1262 and attach the antenna that comes with the kit.
2. Flash the radio firmware:
   ```bash
   cd firmware && pio run -e radio-xiao-sx1262 -t upload
   ```
3. Download the host for your OS from [GitHub Releases](https://github.com/kawase1295/opennek/releases/latest).
   It is a single executable with nothing else to install:
   `opennek-<version>-linux-x64`, `opennek-<version>-windows-x64.exe`, `opennek-<version>-macos-arm64` (Apple silicon)
   or `opennek-<version>-macos-x64` (Intel Mac). `SHA256SUMS` lists their checksums.
4. Start it with the kit's serial port:
   ```bash
   chmod +x opennek-*-linux-x64 && ./opennek-*-linux-x64 --sigfox /dev/ttyACM0     # Linux
   chmod +x opennek-*-macos-* && ./opennek-*-macos-arm64 --sigfox /dev/cu.usbmodem1101   # macOS
   .\opennek-<version>-windows-x64.exe --sigfox COM3       # Windows (PowerShell)
   ```
   The web UI opens in your browser (`--no-open` to skip that). Without `--sigfox`, pick the port in the web UI.
   `--help` lists all options. On Windows, double-clicking the `.exe` starts it too.

   On Linux and macOS, start it from a terminal opened in the download folder (on Linux, right-click the folder →
   **Open in Terminal**). Double-clicking it in the file manager does not start it: on Linux, Files says "There is no
   app installed for executable files".

   On Linux, run `./opennek-*-linux-x64 install` once to add OpenNEK to the app menu; from then on, start it with a
   click there (a terminal window shows its log, and closing it stops OpenNEK). `opennek uninstall` takes it out again.

   The binaries are not signed. On Windows, SmartScreen may say "Windows protected your PC": choose
   **More info → Run anyway**. On macOS, Gatekeeper blocks the downloaded file: run
   `xattr -d com.apple.quarantine opennek-*-macos-*` once, or right-click it in Finder and choose **Open**.

Open <http://127.0.0.1:8085/>. The kit listens on 923.2 MHz (`--sigfox-freq <hz>` to change it). The port is remembered; you can also pick it under
**Settings → Sigfox receiver (RC3)**. Send a message from a Sigfox device (`AT$SF=0102`) and it shows up
in the message list. The web UI is in English; a Japanese translation can be selected in the top right.

On Linux, if you get a permission error on the serial port, run `sudo usermod -aG dialout $USER` and log in again.

Data is stored in `~/.local/share/opennek/` (Windows: `%APPDATA%\opennek\`, macOS: `~/Library/Application Support/opennek/`).

### Uplink authentication

An uplink can only be verified with the key the device signed it with. Sigfox-provisioned private keys
normally cannot be read out of a module, so verify a real device by switching it to the public key
(`00112233445566778899AABBCCDDEEFF`, defined by sigfox-ep-lib). On AX-SFJK and similar modules:

```
ATS410=1    → public key: OpenNEK can verify the messages; not for use on the real network
ATS410=0    → back to the private key; do this before using the real network again
```

| Device | Shown as | Callbacks |
|---|---|---|
| Key registered on the device page | OK, or a key mismatch | only when OK |
| Public key mode, no key registered | Public key | while "Run callbacks for devices without a registered key" is on |
| Private key, not registered | Unverified | while "Run callbacks for devices without a registered key" is on |

Anyone can sign with the public key, so it only shows that the frame is consistent, not who sent it.

### Downlinks

Turn on **Settings → Send downlinks from the receiver**. The kit then answers every authenticated uplink
that requests a downlink (`AT$SF=…,1`) with the device's static data or the callback's `downlinkData`, and the
device prints `RX=…`.

- The kit measures the frequency of uplink frame 1 and transmits on frame 1 − 1 MHz (922.1–922.3 MHz),
  21 s after the end of frame 1, inside the device's RC3 receive window.
- The frame is GFSK 600 bps. Its authentication value is computed with the registered key or the public key.
- The confirmation frame the device sends after `RX=` marks the downlink as acknowledged.

The firmware refuses anything outside these limits, whatever the host asks:

| Limit | Value |
|---|---|
| Transmitting | only while the host allows it; off after every reset |
| Frequency | 922.1–922.3 MHz (the 922.2 MHz unit channel) |
| Power | 10 dBm (10 mW) |
| Carrier sense | −80 dBm for 5 ms before every frame |
| Pause / hourly budget | 100 ms between transmissions, 360 s on air per hour |

> **Radio regulations**: downlinks transmit on the 920 MHz band. Turn this on only when the kit meets
> your local regulations (in Japan: 技適 certification, used with its certified antenna and within its
> certified conditions). Check the limits above against them.

## Callbacks

Configure the URL, headers and body in the **Callbacks** tab. The body supports the same
variables as Sigfox custom callbacks (`{device}` `{time}` `{data}` `{seqNumber}` `{ack}` `{rssi}` `{snr}`
`{station}` `{customData#name}`, … — full list in [docs/protocol.md](docs/protocol.md)).

```json
{"device":"{device}","time":{time},"data":"{data}","seqNumber":{seqNumber},"rssi":{rssi}}
```

- **Payload format** (per device): with something like `temp::int:16:little-endian hum::uint:8 door:0:bool:7`,
  decoded values are shown in the UI and are available in callbacks as `{customData#temp}`.
- **Downlink data**: set the device's downlink mode to "Static data" (e.g. `{time}0102{rssi}`) or to
  "Callback response". For the latter, enable "Take the downlink data from the response" on the callback;
  if the response is `{"0A1B2C3D":{"downlinkData":"0102030405060708"}}`, those 8 bytes are sent to the device.
- The **Send test** button fires the callback with the latest message (not recorded in the logs).

## Using it on WSL2

On Windows, running OpenNEK natively (above) is the simplest. To run it inside WSL instead: WSL cannot see USB devices by default. Use [usbipd-win](https://github.com/dorssel/usbipd-win) on Windows
to attach the kit to WSL (run in an elevated PowerShell):

```powershell
usbipd list
usbipd bind --busid <BUSID>
usbipd attach --wsl --busid <BUSID>
```

The web UI can be opened from a Windows browser at <http://localhost:8085/>.

## Development

Run the host from source (needs [rustup](https://rustup.rs/); `rust-toolchain.toml` picks the Rust version):

```bash
cd host
cargo run --release -- --sigfox /dev/ttyACM0
```

The web UI in `host/ui` is plain JS without a build step and is embedded in the executable.
The LSM100A tools in `tools/lsm` run on Node (`cd tools/lsm && npm install`); only firmware work needs them.

Pushing a version tag (`v0.2.0`, equal to the version in `host/Cargo.toml`) on main builds the executables and
publishes them on GitHub Releases (`.github/workflows/release.yml`).

```bash
scripts/check     # what CI runs: format, lint and test the host, test the UI strings and the LSM100A tools
(cd host && cargo test)
(cd firmware && pio run)
(cd firmware/test && uvx --from ziglang python -m ziglang c++ -std=c++17 -O2 -I ../lib/SigfoxRx/src \
  sigfox_native_test.cpp ../lib/SigfoxRx/src/*.cpp -o sigfox_test && ./sigfox_test ../../fixtures)
```

The host tests decode recorded captures of real Sigfox devices end to end (uplink → callback → downlink) through
a fake receiver that replays the streamer's lines. The C++ decoder in the firmware is checked against the same
captures.

## License

MIT
