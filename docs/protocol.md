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

## Simulated uplinks

`POST /api/uplinks` makes up an uplink and handles it like one from the receiver, with or without a receiver.
The host signs the frame with the device's registered key, or with the public key when no key is registered.
It then stores the message, shows it in the web UI and runs the DATA callbacks with the variables above.

| Field | Required | Contents |
|---|---|---|
| `device` | yes | device ID (8 hex digits). An unknown device is added, as on reception |
| `data` | yes | payload (hex, 0 to 12 bytes) |
| `ack` | no | downlink requested, `true` / `false` (default `false`) |
| `seq` | no | sequence number, 0 to 4095. Default: the device's last plus one, 0 after 4095 and for a new device |
| `rssi` | no | RSSI in dBm, -200 to 0 (default `-100`) |
| `key` | no | key to sign with instead (32 hex digits). For a registered device, another key shows a key mismatch and no callbacks run. For a device without a key, a key other than the public key shows unverified |

```bash
curl -X POST http://127.0.0.1:8085/api/uplinks \
  -H 'Content-Type: application/json' \
  -d '{"device":"0A1B2C3D","data":"0102","ack":true}'
```

The host answers `201 Created` with the stored message after the callbacks have answered. The message has
`"simulated": true`, and so has its row in the CSV.

With `ack: true`, the host resolves the downlink data the device would get (its static data or the `downlinkData` of
a DATA BIDIR callback) and puts it on the message. It never transmits the downlink, even with downlinks turned on.
`downlinkStatus` is `simulated`, or `nodata` when there is no data or the uplink failed authentication.

Send the body as JSON with `Content-Type: application/json`. A field with an invalid value gets `400 Bad Request`
with `{"error": "<field>: <what is expected>"}`. Give `seq` when you send uplinks of one device at the same time,
because two uplinks without `seq` can get the same number.

## CSV export

The **CSV** button on the **Messages** tab downloads `GET /api/messages.csv`. The file holds every stored message,
oldest first. The host keeps the newest 10,000 messages. When you choose a device in the device list on the same tab,
the file holds only that device's messages.
