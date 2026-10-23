# OpenNEK — Open Network Emulator Kit

**English** | [日本語](README.ja.md)

A **local network for real Sigfox devices** (RC3, Japan) that runs on a PC with a USB radio receiver.
Uplinks from your devices show up in a web UI, are forwarded to your own application through
Sigfox-compatible HTTP callbacks, and can be answered with 8-byte downlinks.
Develop and test Sigfox applications on your desk, without a backend subscription.

```
┌──────────────┐  Sigfox RC3 (923.2 MHz)   ┌──────────┐  USB serial  ┌───────────────────────┐  HTTP   ┌──────────┐
│ Sigfox device │ ─ uplink ×3 ───────────▶ │ Receiver │ ───────────▶ │ opennek (Bun + Hono)  │ ──────▶ │ Your     │
│               │ ◀──── downlink (922 MHz) │          │ ◀─────────── │ Web UI / SQLite       │ ◀────── │ app      │
└──────────────┘                            └──────────┘              └───────────────────────┘         └──────────┘
```

| Part | Contents |
|---|---|
| `host/` | Host server. Bun + Hono + bun:sqlite. Sigfox decoder, web UI, callbacks, downlinks |
| `firmware/` | PlatformIO (Arduino) firmware for the ESP32-S3 side of the receiver: USB bridge and standalone tester |
| `docs/protocol.md` | Callback and downlink specification |
| `packaging/build-deb.sh` | Builds a `.deb` for Ubuntu |

---

## Getting started

```bash
cd host
bun install
bun run start -- --sigfox /dev/ttyACM0    # the receiver's serial port
```

Open <http://127.0.0.1:8085/>. The port is remembered; you can also pick it under
**Settings → Sigfox receiver (RC3)**. Send a message from a Sigfox device (`AT$SF=0102`) and it shows up
in the message list. The web UI is in English; a Japanese translation can be selected in the top right.

If you get a permission error on the serial port, run `sudo usermod -aG dialout $USER` and log in again.

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

Turn on **Settings → Send downlinks from the receiver**. The receiver then answers every authenticated
uplink that requests a downlink (`AT$SF=…,1`) inside the device's receive window, and the device prints `RX=…`.
The confirmation frame the device sends afterwards marks the downlink as acknowledged.

> **Radio regulations**: downlinks transmit on the 920 MHz band. Turn this on only when the receiver meets
> your local regulations (in Japan: 技適 certification, used within its certified conditions).

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

WSL cannot see USB devices by default. Use [usbipd-win](https://github.com/dorssel/usbipd-win) on Windows
to attach the receiver to WSL (run in an elevated PowerShell):

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
opennek --open --sigfox /dev/ttyACM0     # or: systemctl --user enable --now opennek
```

The host is compiled into a single binary with `bun build --compile`, so neither Bun nor Node is needed to run it.
Data is stored in `~/.local/share/opennek/`.

## Development

```bash
(cd host && bun test && bun run typecheck)
(cd firmware && pio run)
(cd firmware/test && uvx --from ziglang python -m ziglang c++ -std=c++17 -O2 -I ../lib/SigfoxRx/src \
  sigfox_native_test.cpp ../lib/SigfoxRx/src/*.cpp -o sigfox_test && ./sigfox_test ../../host/test/fixtures)
```

The host tests decode recorded captures of real Sigfox devices end to end (uplink → callback → downlink) and
talk to the receiver over a socat virtual serial port. The C++ decoder in the firmware is checked against the
same captures.

## License

MIT
