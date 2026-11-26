# OpenNEK Callback Specification

**English** | [日本語](protocol.ja.md)

The host receives uplinks from real Sigfox devices through the receiver, authenticates them, runs HTTP callbacks
and sends downlinks back through the receiver.

```
[Sigfox device] --uplink (RC3)--> [Receiver] --USB serial--> [Host] --HTTP callback--> [App]
       ^                                                           |
       +----------------------- downlink (8 bytes) ----------------+
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

### SERVICE ACKNOWLEDGE

A SERVICE ACKNOWLEDGE callback (URL channel) is sent once for each uplink that requested a downlink (`{ack}` is
`true`), after the host has settled that downlink, for the devices in its device list. It is sent like a DATA callback
(url pattern, method, headers, body, content type, SNI), after the same authentication check, and logged in the
callback's history. It has only the variables of the Sigfox Backend's ACKNOWLEDGE callback:

| Variable | Contents |
|---|---|
| `{device}` | device ID (8 hex digits) |
| `{time}` | reception time of the uplink (UNIX seconds) |
| `{infoCode}` | what became of the downlink (table below) |
| `{infoMessage}` | the same in words |
| `{downlinkAck}` | `true` when the downlink was transmitted |
| `{downlinkOverusage}` | always `false` (OpenNEK has no downlink quota) |

The host decides the outcome in this order: the downlink data, the key to sign it with, the transmitter, the timing,
then the radio. An uplink without downlink data therefore gets `1` even on a host that only receives.

| Outcome | `{infoCode}` | `{infoMessage}` | `{downlinkAck}` | Downlink status |
|---|---|---|---|---|
| Transmitted in the device's receive window | `0` | `Acked: the downlink was transmitted` | `true` | `sent` |
| No downlink data: mode `none`, empty static data, or no chosen BIDIR callback answered with `downlinkData` (a `204`, an HTTP error or a timeout included) | `1` | `No answer: no downlink data was given` | `false` | `nodata` |
| `downlinkData` that is not 8 bytes of hex | `2` | `Invalid payload: the downlink data is not 8 bytes of hex` | `false` | `nodata` |
| Mode `callback`, but no enabled DATA BIDIR callback chosen for downlink applies to the device | `8` | `Invalid configuration: the device takes its downlink from a callback, but no DATA BIDIR callback is chosen for downlink` | `false` | `nodata` |
| No key to sign it with: the uplink is not authenticated | `4` | `Network error: not transmitted, the uplink is not authenticated` | `false` | `nodata` |
| **Send downlinks from the receiver** off | `4` | `Network error: not transmitted, OpenNEK is receiving only` | `false` | `unsupported` |
| Frame 1 not received, or its frequency unknown | `4` | `Network error: not transmitted, the time or frequency of frame 1 is unknown` | `false` | `unsupported` |
| The device's receive window had closed by the time the downlink was ready | `4` | `Network error: not transmitted, the device's receive window has closed` | `false` | `failed` |
| The receiver did not transmit it (e.g. an RTL-SDR, or a channel busy until the window closed) | `4` | `Network error: not transmitted, the transmitter did not send it` | `false` | `failed` |

The codes are the Backend's: `0` ACKED, `1` NO_ANSWER, `2` INVALID_PAYLOAD, `4` NETWORK_ERROR, `8`
INVALID_CONFIGURATION. OpenNEK never sends `3` OVERRUN_ERROR (there is no daily quota), `5` PENDING, `6`
NO_DOWNLINK_CONTRACT or `7` TOO_MANY_REQUESTS. Valid downlink data is stored with the message whenever it was given,
also when it could not be sent.

As in the Backend, the callback reports what the station did, once. The device's later confirmation of the downlink
(status `acked`) sends no second ACKNOWLEDGE callback. **Send test** sends it as if the latest message's downlink had
been transmitted (`{infoCode}` `0`).
