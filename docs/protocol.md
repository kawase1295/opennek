# OpenNEK Protocol Specification v1

**English** | [日本語](protocol.ja.md)

OpenNEK consists of three layers.

```
[Device ESP32] --(1) radio frame / ESP-NOW--> [Gateway ESP32] --(2) JSON Lines / USB serial--> [Host (Bun)] --(3) HTTP callback--> [App]
       ^                                                                                                  |
       +------------------------------------------ downlink (8 bytes) -----------------------------------+
```

The gateway is a simple radio ⇔ serial bridge. Frame parsing, authentication, de-duplication,
callbacks and downlink decisions are all done by the host.

---

## (1) Radio frame

- Physical layer: ESP-NOW (Wi-Fi 2.4 GHz). Channel 1–13 (default 1). All nodes must use the same channel.
- Uplinks are broadcast (`FF:FF:FF:FF:FF:FF`) and **repeated 3 times** (repeat 0, 1, 2).
- Downlinks are unicast from the gateway to the device's MAC address.
- All integers are **little-endian**.

### Layout (shared by uplink and downlink)

| Offset | Size | Field | Contents |
|---|---|---|---|
| 0 | 2 | magic | `0x4E 0x4B` ("NK") |
| 2 | 1 | version | `1` |
| 3 | 1 | type | `0x01` = uplink, `0x02` = downlink |
| 4 | 1 | flags | see below |
| 5 | 4 | deviceId | uint32, displayed as 8 upper-case hex digits (e.g. `0A1B2C3D`) |
| 9 | 2 | seq | sequence number 0–4095 (12 bits, wraps from 4095 to 0) |
| 11 | 1 | len | payload length: 0–12 for uplinks, always 8 for downlinks |
| 12 | len | payload | |
| 12+len | 4 | mac | first 4 bytes of `HMAC-SHA256(key, frame[0 .. 12+len))`, computed with the repeat bits of flags set to 0 |

flags:

| Bit | Meaning |
|---|---|
| 0 | downlink requested (uplink only) |
| 1-2 | repeat number 0–2 |
| 3 | OOB (keep-alive) frame |
| 4-7 | reserved (0) |

- `key` is a 16-byte per-device key. The device generates it randomly on first boot, stores it in NVS,
  and reports it as 32 hex digits via `AT$PAC`. Register this key on the host together with the device ID.
- A downlink carries the `seq` of the uplink it answers. The device accepts only downlinks whose
  deviceId, seq and mac all match.

### Timing

- Interval between repeats: 30 ms
- When a downlink is requested, the device listens for up to `dlWaitMs` (default 8000 ms) after the last repeat.
- The host waits `dedupWindowMs` (default 400 ms) after the first frame to merge the repeats, then runs
  callbacks and sends the downlink within `downlinkTimeoutMs` (default 5000 ms).

---

## (2) Gateway ⇔ host (USB serial)

115200 bps 8N1, one JSON object per line (`\n` separated).

### Gateway → host

```jsonc
{"t":"hello","fw":"0.1.0","radio":"espnow","ch":1,"mac":"24:6F:28:AA:BB:CC"}  // on boot and in reply to ping
{"t":"up","src":"24:6F:28:11:22:33","rssi":-48,"frame":"4E4B0101..."}           // received radio frame (raw hex)
{"t":"sent","dst":"24:6F:28:11:22:33","ok":true}                               // downlink send result
{"t":"log","msg":"..."}                                                        // debug
```

### Host → gateway

```jsonc
{"t":"ping"}
{"t":"ch","ch":6}                                             // change channel (saved to NVS, replies with hello)
{"t":"down","dst":"24:6F:28:11:22:33","frame":"4E4B0102..."}  // send a downlink
```

---

## (3) HTTP callbacks

Variables compatible with Sigfox backend custom callbacks are available.

| Variable | Contents |
|---|---|
| `{device}` | device ID (8 hex digits) |
| `{deviceName}` | name registered on the host |
| `{time}` | reception time (UNIX seconds) |
| `{data}` | payload (lower-case hex) |
| `{seqNumber}` | sequence number |
| `{ack}` | downlink requested, `true` / `false` |
| `{rssi}` | best RSSI (dBm) |
| `{snr}` | always `0` (not available with ESP-NOW) |
| `{station}` | lower 4 bytes of the gateway MAC (hex) |
| `{nbFrames}` | number of repeats received |
| `{oob}` | `true` for OOB frames |
| `{customData#name}` | value decoded with the custom payload format |

### Custom payload format

Space-separated `name:offset:type:args` fields. Leaving `offset` empty (`name::type`) reads right after the previous field.

| Type | Syntax | Example |
|---|---|---|
| uint / int | `uint:<bits>[:little-endian]` | `temp::int:16:little-endian` |
| float | `float:32` or `float:64` (optionally `:little-endian`) | `v::float:32` |
| bool | `bool:<bit>` (0 = LSB) — reads the byte at offset, does not advance | `flag:0:bool:7` |
| char | `char:<bytes>` | `name::char:4` |

### Downlinks

Each device selects `none` / `static` (fixed 8 bytes) / `callback`.

The `static` data may embed these variables between hex bytes (Sigfox-style emulator compatible). Hex is
case-insensitive and must be written in whole bytes; variable names are lower-case. The template must
render to exactly 8 bytes, which is checked when the device is saved (e.g. `{time}0102{rssi}`).

| Variable | Bytes | Contents |
|---|---|---|
| `{time}` | 4 | UNIX time of the uplink in seconds, big-endian |
| `{tapid}` | 4 | `{station}` parsed as hex and left-padded to 8 hex digits (`0000` for real Sigfox → `00000000`); `00000000` if it is not hex |
| `{rssi}` | 2 | best RSSI of the uplink (dBm) as int16 two's complement, big-endian (`-100` → `ff9c`) |

With `callback`, the 8 bytes are
taken from the response of a callback marked for downlinks, in either of these formats:

```json
{"0A1B2C3D": {"downlinkData": "0102030405060708"}}
{"downlinkData": "0102030405060708"}
```

A `204 No Content` response means no downlink is sent.
