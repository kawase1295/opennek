# OpenNEK Callback Specification

**English** | [日本語](protocol.ja.md)

The host receives uplinks from real Sigfox devices through the receiver, authenticates them, runs HTTP callbacks
and sends downlinks back through the receiver.

```
[Sigfox device] --uplink (RC3)--> [Receiver] --USB serial--> [Host (Bun)] --HTTP callback--> [App]
       ^                                                          |
       +----------------------- downlink (8 bytes) ---------------+
```

---

## HTTP callbacks

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
| `{snr}` | always `0` (not measured) |
| `{station}` | always `0000` (there is no base station) |
| `{nbFrames}` | number of repeats received |
| `{oob}` | `true` for control frames (keep-alive, downlink confirmation) |
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
| `{tapid}` | 4 | `{station}` parsed as hex and left-padded to 8 hex digits (`0000` → `00000000`); `00000000` if it is not hex |
| `{rssi}` | 2 | best RSSI of the uplink (dBm) as int16 two's complement, big-endian (`-100` → `ff9c`) |

With `callback`, the 8 bytes are
taken from the response of a callback marked for downlinks, in either of these formats:

```json
{"0A1B2C3D": {"downlinkData": "0102030405060708"}}
{"downlinkData": "0102030405060708"}
```

A `204 No Content` response means no downlink is sent.
