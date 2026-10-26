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
| `host/` | Host server, a single Rust executable (axum, SQLite). Sigfox decoder, web UI (`host/ui`, embedded), callbacks, downlinks; the desktop app (`host/crates/opennek-desktop`, Tauri) shows the same UI in a window |
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
3. Download OpenNEK from [GitHub Releases](https://github.com/kawase1295/opennek/releases/latest).
   On **Linux and Windows, the desktop app** is the simplest: it shows the web UI in its own window, with no terminal
   and no browser.
   - **Linux** (Ubuntu 24.04 or later, or a distro as recent): `opennek-desktop-<version>-linux-x64.deb`.
     ```bash
     sudo apt install ./opennek-desktop-*-linux-x64.deb
     ```
     OpenNEK is then in the app menu. The package also lets the logged-in user open the XIAO kit's serial port
     (a udev rule), so there is no group to join and no new login. `sudo apt remove io-github-kawase1295-opennek`
     removes it; the data stays. Older distros use the executable below.
   - **Windows**: `opennek-desktop-<version>-windows-x64-setup.exe`. It installs for your user only, without
     administrator rights, and adds OpenNEK to the Start menu. It needs WebView2, which Windows 10 and 11 normally
     have; when it is missing the installer downloads it, so it needs internet access then.

   The app listens on <http://127.0.0.1:8085/> (another free port if other software has 8085; the **Log** tab says
   which) and only on this PC. Closing its window stops OpenNEK.

   **The executable** is for macOS, older Linux distros, running without a desktop, and options such as `--sigfox` or
   `--host` (access from the LAN): `opennek-<version>-linux-x64`, `opennek-<version>-windows-x64.exe`,
   `opennek-<version>-macos-arm64` (Apple silicon) or `opennek-<version>-macos-x64` (Intel Mac), a single file with
   nothing else to install. `SHA256SUMS` lists the checksums of all downloads.
4. Start the executable (the app needs no step here). It finds the XIAO kit by itself; `--sigfox` gives the serial port
   of another receiver, or picks one of several kits:
   ```bash
   chmod +x opennek-*-linux-x64 && ./opennek-*-linux-x64 --sigfox /dev/ttyACM0     # Linux
   chmod +x opennek-*-macos-* && ./opennek-*-macos-arm64 --sigfox /dev/cu.usbmodem1101   # macOS
   .\opennek-<version>-windows-x64.exe --sigfox COM3       # Windows (PowerShell)
   ```
   The web UI opens in your browser (`--no-open` to skip that). `--sigfox off` starts it without a receiver.
   `--help` lists all options. On Windows, double-clicking the `.exe` starts it too.

   On Linux and macOS, start it from a terminal opened in the download folder (on Linux, right-click the folder →
   **Open in Terminal**). Double-clicking it in the file manager does not start it: on Linux, Files says "There is no
   app installed for executable files".

   On Linux, run `./opennek-*-linux-x64 install` once to add it to the app menu as **OpenNEK (Terminal)**; from then on,
   start it with a click there (a terminal window shows its log, and closing it stops OpenNEK). `opennek uninstall`
   takes it out again.

   The downloads are not signed. On Windows, SmartScreen may say "Windows protected your PC" for the installer or the
   `.exe`: choose **More info → Run anyway**. On macOS, Gatekeeper blocks the downloaded file: run
   `xattr -d com.apple.quarantine opennek-*-macos-*` once, or right-click it in Finder and choose **Open**.

Open <http://127.0.0.1:8085/>. The kit listens on 923.2 MHz (`--sigfox-freq <hz>` to change it). OpenNEK finds the XIAO kit by itself
(the **Log** tab says `sigfox: found the XIAO kit on <port>`) and remembers its port. Pick the port under
**Settings → Sigfox receiver (RC3)** only for another receiver or when several kits are plugged in; **Don't use** there
is remembered too. Send a message from a Sigfox device (`AT$SF=0102`) and it shows up
in the message list. The web UI is in English; a Japanese translation can be selected in the top right.

On Linux, if you get a permission error on the serial port, run `sudo usermod -aG dialout $USER` and log in again.
This applies to the app too when the receiver is not the XIAO kit its udev rule is for.

Data is stored in `~/.local/share/opennek/` (Windows: `%APPDATA%\opennek\`, macOS: `~/Library/Application Support/opennek/`).
Only one OpenNEK runs per data dir, since it holds the database and the receiver's serial port; the app and the
executable share it. A second one says where the first one is running and exits. Another `--data <dir>` is a separate instance.

### Receiving with an RTL-SDR (receive only)

An RTL-SDR (tested with the RTL-SDR Blog V3) can receive uplinks instead of the XIAO kit. It cannot send downlinks.

- Pick `RTL-SDR … (rtlsdr:0)` in **Settings → Sigfox receiver (RC3)**, or start with `--sigfox rtlsdr:0`. Without a XIAO kit
  plugged in, a single RTL-SDR is picked by itself (the **Log** tab shows `sigfox: found an RTL-SDR on rtlsdr:0`).
- It receives the whole uplink band (923.2 MHz ± 100 kHz) at once, so it gets all three repetitions although each frame is on
  another frequency. The gain is fixed at 29.7 dB, and the RSSI is approximate (not calibrated).
- On Linux it takes the dongle from the DVB-T driver (dvb_usb_rtl28xxu) by itself. On a permission error, add this udev rule,
  run `sudo udevadm control --reload-rules && sudo udevadm trigger` and plug the dongle in again (the desktop app's .deb has it).
  ```
  # /etc/udev/rules.d/70-rtl-sdr.rules
  SUBSYSTEM=="usb", ATTRS{idVendor}=="0bda", ATTRS{idProduct}=="2838", TAG+="uaccess"
  ```
- On Windows, install the WinUSB driver for the RTL-SDR with [Zadig](https://zadig.akeo.ie/).

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

Callbacks have the types, channels and fields of a device type's callbacks in the Sigfox Backend: **Type**
(DATA UPLINK / BIDIR, SERVICE STATUS / ACKNOWLEDGE / REPEATER / DATA_ADVANCED, ERROR), **Channel** (URL, BATCH_URL,
EMAIL), Send duplicate, Custom payload config, Url pattern, Use HTTP Method, Send SNI, Headers, Content type and Body.
OpenNEK has no device types, so a callback names its devices instead (empty for all). OpenNEK sends DATA callbacks to
a URL; the other types and channels are kept as in the Backend but not sent yet, which the **Callbacks** tab shows.
The url pattern, headers and body support the same variables as Sigfox custom callbacks (`{device}` `{time}`
`{data}` `{seqNumber}` `{ack}` `{rssi}` `{snr}` `{station}` `{customData#name}`, … — full list in
[docs/protocol.md](docs/protocol.md)).

```json
{"device":"{device}","time":{time},"data":"{data}","seqNumber":{seqNumber},"rssi":{rssi}}
```

- **Custom payload config** (per callback, as in the Backend): with something like
  `temp::int:16:little-endian hum::uint:8 door:0:bool:7`, the values are available as `{customData#temp}`. A callback
  without one uses the device's **payload format**, which is also what the UI shows decoded.
- **Downlink data**: set the device's downlink mode to "Static data" (e.g. `{time}0102{rssi}`) or to
  "Callback response". For the latter, make a DATA BIDIR callback and choose it in the Downlink column of the list
  (a new BIDIR callback is not chosen, as in the Backend); if the response is
  `{"0A1B2C3D":{"downlinkData":"0102030405060708"}}`, those 8 bytes are sent to the device.
- **Send SNI** is on for new callbacks (the Backend leaves it off), since most HTTPS servers need it.
- The **Send test** button fires the callback with the latest message (not recorded in the history). Each callback's
  last answer is in the list, and its history behind it.

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

Run the host from source (needs [rustup](https://rustup.rs/), where `rust-toolchain.toml` picks the Rust version,
and Node 22 or later). The web UI in `host/ui` ([Svelte](https://svelte.dev/), TypeScript, Vite) is built first:
the executable embeds its build (`host/ui/dist`), and a Rust build without it stops with a hint.

```bash
(cd host/ui && npm ci && npm run build)
cd host
cargo run --release -- --sigfox /dev/ttyACM0
```

While working on the UI, run `npm run dev` in `host/ui` next to a running host: it serves the UI with hot reload
on <http://localhost:5173/> and passes `/api` to the host on port 8085. The UI's types for the API
(`host/ui/src/lib/api.gen.ts`) are generated from the host's Rust types; after changing those, regenerate them with
`UPDATE_API_TYPES=1 cargo test -p opennek-core api_types` (a test fails while they differ).

The desktop app (`host/crates/opennek-desktop`, [Tauri](https://tauri.app/)) shows the same web UI in its own window.
Plain `cargo` commands leave it out, so the `opennek` executable stays a static binary; build it by name:

```bash
cd host
cargo run -p opennek-desktop
```

On Linux it needs the WebKitGTK development files
(`sudo apt-get install pkg-config libwebkit2gtk-4.1-dev libxdo-dev libssl-dev librsvg2-dev` on Ubuntu / Debian);
Windows has WebView2 already. The app listens on 127.0.0.1:8085, or on a free port when other software has 8085.
The LSM100A tools in `tools/lsm` run on Node (`cd tools/lsm && npm install`); only firmware work needs them.

Pushing a version tag (`v0.2.0`, equal to the version in `host/Cargo.toml`) on main builds the executables and
the desktop app's installers and publishes them on GitHub Releases (`.github/workflows/release.yml`). To build an
installer locally, run `npx @tauri-apps/cli@2.12.4 build --bundles deb` (or `nsis` on Windows) in
`host/crates/opennek-desktop`.

```bash
scripts/check     # what CI runs: type check, test and build the UI, format, lint and test the host and the desktop app, test the LSM100A tools and the firmware natively
(cd host && cargo test)
(cd firmware && pio run)
(cd firmware/test && uvx --from ziglang python -m ziglang c++ -std=c++17 -O2 -I ../lib/SigfoxRx/src \
  sigfox_native_test.cpp ../lib/SigfoxRx/src/*.cpp -o sigfox_test && ./sigfox_test ../../fixtures)
(cd firmware/test && uvx --from ziglang python -m ziglang c++ -std=c++17 -O2 -I ../lib/RadioStreamer/src \
  streamer_native_test.cpp ../lib/RadioStreamer/src/*.cpp -o streamer_test && ./streamer_test)
```

The host tests decode recorded captures of real Sigfox devices end to end (uplink → callback → downlink) through
a fake receiver that replays the streamer's lines. The C++ decoder in the firmware is checked against the same
captures.

## License

MIT
