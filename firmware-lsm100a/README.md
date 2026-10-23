# LSM100A RSSI streamer

**English** | [日本語](README.ja.md)

Experimental firmware that turns a SEONG JI **LSM100A** (STM32WLE5, Sigfox + LoRa module) into a
wide-band envelope receiver for real Sigfox uplinks (RC3, 923.2 MHz).

Sigfox uplinks are pulse-shaped DBPSK, so every phase reversal makes the amplitude dip towards zero.
The firmware keeps the sub-GHz radio in continuous RX with a 234 kHz filter (wider than the 192 kHz
uplink band) and streams the instantaneous RSSI at 2 kHz; the host turns the dips back into bits.

> Flashing replaces the AT firmware. The Sigfox credentials (ID / PAC / key) of the module may be lost.
> Use a module you can dedicate as a receiver.

## Wiring

```
LSM100A (receiver)            M5Stack Cardputer (or any ESP32-S3)          PC
UART2_TX ──────────────────▶ G1 (GPIO1)
UART2_RX ◀────────────────── G2 (GPIO2)       USB ─────────────────▶ /dev/ttyACM0
VCC 3.3 V / GND ──────────── 3.3 V LDO / GND
```

Any 3.3 V USB-UART adapter works as well. With the Cardputer, flash the bridge first; it follows the
baud rate the PC opens the port with:

```bash
cd firmware && pio run -e bridge-s3 -t upload
```

## Build and flash

```bash
cd firmware-lsm100a && pio run                       # .pio/build/lsm100a/firmware.bin
cd tools/lsm && npm install && npx tsx lsm-flash.ts /dev/ttyACM0 ../../firmware-lsm100a/.pio/build/lsm100a/firmware.bin
```

`lsm-flash.ts` resets the module (ATZ for the stock firmware, `X` for this firmware) and talks to the
IAP bootloader over Ymodem. If the module does not reset by itself, pull RST low briefly (or power-cycle
it) while "Waiting for the bootloader" is shown.

To go back to the stock AT firmware, flash `FW/LSM100A_SDK_V104_240129.bin` from the
[LSM100A SDK](https://github.com/Support-SJI/LSM100A) the same way, then run `AT$RFS`.

## Capture

```bash
cd tools/lsm && npx tsx rssi-capture.ts /dev/ttyACM0 --out capture
```

Send a few frames from another Sigfox device (e.g. `AT$SF=0102030405` on a second LSM100A) and stop with
Ctrl+C. `capture.html` plots every burst; `capture.json` keeps all samples for later analysis.

Options: `--freq <hz>` (default 923200000), `--bw <code>` (SX126x RX bandwidth code, 10 = 234.3 kHz,
25 = 312 kHz, 9 = 467 kHz), `--rate <hz>` (100-4000), `--boost 0|1`.

## Decode Sigfox uplinks live

```bash
cd tools/lsm && npx tsx sigfox-listen.ts /dev/ttyACM0 [--save session.json]
```

Prints every frame (repetition, CRC, RSSI, receiver clock error) and one line per message once its
repetitions are merged. Verified with a real RC3 device: 1- and 5-byte payloads and the downlink request
flag decode with a valid CRC; the LSM100A's RC clock runs about 1.3 % slow and is tracked automatically.

## Receive in the OpenNEK host

The host runs the same decoder. Real Sigfox messages then appear in the Web UI and the device list, and
trigger the Sigfox-compatible callbacks:

```bash
opennek --sigfox /dev/ttyACM0 [--sigfox-freq 923200000]
```

The port is remembered; pick it (or "Don't use" to turn the receiver off) under Settings → Sigfox receiver (RC3).

### Downlinks (firmware 0.3.0, transmits)

With "受信機からダウンリンクを送信する" (Settings → Sigfox 受信機) turned on, the receiver answers uplinks
that request a downlink. The request must be authenticated: registered key, or public key mode (below). The
payload is the device's static data or the callback's `downlinkData`.

1. While the setting is on, the receiver tunes to frame 1 of each message once its frequency offset (CFO)
   has settled. Frame 1 is then the midpoint of frames 2 and 3 (frame 1 ± 6 kHz), measured to ~100 Hz.
2. The downlink frequency is frame 1 − 1 MHz. Measuring and transmitting use the same oscillator, so no
   calibration is needed.
3. The downlink is sent 21 s after the end of frame 1, inside the device's RC3 window (19 s after frame 1
   for 33.5 s). The frame is GFSK 600 bps, 800 Hz deviation, preamble `AAAA`, sync word `B227`, as SJI's
   `RF_Tx_Local_loop`.

After `RX=`, the device sends a downlink confirmation: one control frame with the next sequence number. The
host stores it as an OOB message with the device's voltages, temperature and the RSSI at which it received the
downlink, and marks the downlink as "受信確認" (acked).

The firmware refuses anything outside these limits, whatever the host asks:

| Limit | Value |
|---|---|
| Transmitting | only between `E1` and `E0` (the host sends them around each frame); off after every reset |
| Frequency | 922.1–922.3 MHz (the 922.2 MHz unit channel) |
| Power | 10 dBm (10 mW), low-power PA |
| Carrier sense | −80 dBm for 5 ms in ~200 kHz (sigfox-ep-lib RC3 LBT); if busy, the host retries every 200 ms until the window closes |
| Pause / hourly budget | 100 ms between transmissions, 360 s on air per hour |

Turn it on only when the transmitting equipment meets its radio certification (技適) or licence. Check
these values against your certification conditions.

### Uplink authentication

The authentication field of an uplink can only be checked with the key the device signed it with:

| Device | Shown as | Callbacks |
|---|---|---|
| Private key registered on the host (device page) | OK, or 鍵不一致 when it does not match | run only when OK |
| Public key mode (see below), no key registered | 公開鍵 (public key) | run while "鍵が未登録のデバイスもコールバックを実行する" is on |
| Private key, not registered (the usual case) | 未検証 (unverified) | run while "鍵が未登録のデバイスもコールバックを実行する" is on |

Like the official SNEK, verify a real device by switching it to the public key
(`00112233445566778899AABBCCDDEEFF`, defined by sigfox-ep-lib). On LSM100A / AX modules:

```
ATS410=1    → public key: OpenNEK can verify the messages; not for use on the real network
ATS410=0    → back to the private key; do this before using the real network again
```

Anyone can sign with the public key, so it only shows that the frame is consistent, not who sent it.
It is treated like an unregistered device, and a registered key always takes precedence.

## Standalone tester (M5Stack Cardputer)

The Cardputer can decode on its own, without a PC, and check a device under test:

```bash
cd firmware && pio run -e tester-cardputer -t upload    # replaces the bridge; flash bridge-s3 to go back
```

Keep the LSM100A on Grove G1/G2 as above. Press `L` and send one message from the device to register it
(or `I` to type its ID). Every message of a registered device is then checked and shown as PASS / WARN / FAIL:

| Check | Result |
|---|---|
| All three repetitions received with a valid CRC | WARN when one or two are missing |
| Sequence number is the previous one + 1 | WARN on a gap, a repeat or a step back |
| Payload equals the expected value (`P`, optional) | FAIL on a mismatch |
| Signal at least 25 dB above the noise floor | WARN when weaker |

Other keys: `D` registered devices (`1`-`8` removes one), `H` history, `M` mute, `R` reset counters,
`` ` `` back. With a microSD card, results are appended to `/opennek/tester.csv` and the last 30 s of RSSI
samples are saved next to every WARN/FAIL result (`/opennek/raw-*.rssi`, the same format as
`fixtures/`). USB serial prints one JSON line per result and a health line every 10 s.

The tester only receives: the LSM100A firmware never transmits and the Cardputer firmware never starts
Wi-Fi or Bluetooth. Messages from devices that are not registered are neither shown nor logged (learn mode
only offers the ID of the next device it hears).

The decoder is a C++ port of `tools/lsm/sigfox` (`firmware/lib/SigfoxRx`); `firmware/test/sigfox_native_test.cpp`
checks that it gives the same results on the recorded captures.

## Serial protocol (115200 bps)

| Direction | Line | Meaning |
|---|---|---|
| out | `I fw=0.3.0 f=923200000 bw=10 sr=2000 boost=1 cfo=1 pd=4 tx=0` | status |
| out | `R<seq>:<64 x 2 hex>` | 64 RSSI samples, dBm = -value / 2 |
| out | `C<seq>:<8 x 3 hex>` | CFO register (12-bit signed), one per 8 samples of the R line with the same seq (0.2.0, with `C1`) |
| out | `D ok ms=<n>` / `D busy rssi=<dBm>` / `D off` / `D range` / `D limit` / `D err` | result of `T` (0.3.0) |
| in | `F<hz>` `W<code>` `S<hz>` `B<0\|1>` | frequency, bandwidth, sample rate, LNA boost |
| in | `C<0\|1>` `P<code>` | CFO lines, preamble detector (0.2.0) |
| in | `E<0\|1>` `T<hz>,<dBm>,<30 hex>` | allow transmitting, send one downlink frame (0.3.0) |
| in | `?` / `X` | status / reset (into the IAP bootloader) |
