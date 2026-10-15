// Transparent USB <-> UART bridge for an ESP32-S3 (e.g. M5Stack Cardputer + LSM100A on Grove).
//
// Uses the S3's hardware USB serial (USB-JTAG) rather than TinyUSB: it survives usbipd/WSL, and esptool
// can still reset it into download mode. The hardware CDC does not report the host's baud rate, so the
// UART baud rate is set in-band: after at least GUARD_MS of silence, a line starting with "~~~OPENNEK:"
// is taken as a command instead of being forwarded.
//   ~~~OPENNEK:BAUD=9600     -> ~~~OK baud=9600
//   ~~~OPENNEK:DIAG          -> ~~~OK boots=.. uptime_ms=.. usb_to_uart=.. uart_to_usb=.. baud=..
//   ~~~OPENNEK:IAP           -> resets an LSM100A (stock AT firmware or OpenNEK streamer) and catches its
//                               IAP bootloader window; the bootloader output is forwarded, then ~~~OK iap
//   ~~~OPENNEK:KNOCK=<ms>    -> sends '1' every 10 ms for up to <ms> (e.g. while the module is reset by
//                               hand) and stops at the bootloader prompt; then ~~~OK knock=seen|timeout
// LSM100A: 9600 for the AT firmware, 115200 for its IAP bootloader and the OpenNEK RSSI streamer.
#include <Arduino.h>
#include <esp_system.h>

#ifndef BRIDGE_RX
#define BRIDGE_RX 1  // Cardputer Grove G1 <- LSM100A UART TX
#endif
#ifndef BRIDGE_TX
#define BRIDGE_TX 2  // Cardputer Grove G2 -> LSM100A UART RX
#endif

#define GUARD_MS 150
#define CMD_PREFIX "~~~OPENNEK:"
#define CMD_TIMEOUT_MS 200

static HardwareSerial uart(1);
static uint32_t baud = 115200;
// Survives software resets and brownouts, so DIAG can tell whether the board rebooted
RTC_NOINIT_ATTR static uint32_t bootMagic;
RTC_NOINIT_ATTR static uint32_t bootCount;
static uint32_t toUart = 0;
static uint32_t toUsb = 0;
static uint32_t lastUsbRx = 0;

static void reply(const char* fmt, ...) {
  char line[160];
  va_list ap;
  va_start(ap, fmt);
  int n = vsnprintf(line, sizeof(line), fmt, ap);
  va_end(ap);
  Serial.write((const uint8_t*)line, n);
}

// Incremental substring matcher (good enough for these patterns: no repeated prefix besides "CCC")
struct Matcher {
  const char* text;
  size_t matched;
  explicit Matcher(const char* t) : text(t), matched(0) {}
  bool feed(uint8_t c) {
    matched = c == text[matched] ? matched + 1 : (c == text[0] ? 1 : 0);
    if (text[matched]) return false;
    matched = 0;
    return true;
  }
};

// Forwards pending UART bytes; returns true once the IAP bootloader is waiting for a transfer
// (its prompt, or repeated Ymodem CRC requests when the prompt went by earlier)
static bool forwardUart(bool watchIap = false) {
  static Matcher prompt("Waiting");
  static Matcher crc("CCC");
  bool seen = false;
  uint8_t buf[256];
  size_t n;
  while ((n = uart.available()) > 0) {
    n = uart.readBytes(buf, min(n, sizeof(buf)));
    Serial.write(buf, n);
    toUsb += n;
    for (size_t i = 0; watchIap && i < n; i++) seen |= prompt.feed(buf[i]) | crc.feed(buf[i]);
  }
  return seen;
}

// Sends '1' every 10 ms until the IAP prompt appears or `ms` elapse
static bool knock(uint32_t ms) {
  for (uint32_t start = millis(); millis() - start < ms;) {
    uart.write('1');
    delay(10);
    if (forwardUart(true)) {
      delay(50);
      forwardUart();
      return true;
    }
  }
  return false;
}

// The LSM100A bootloader only listens for '1' during the first 0.5 s after reset, which is too tight to
// hit from the PC through USB, so the bridge does the reset and the knocking itself.
static void enterIap() {
  uart.updateBaudRate(115200);
  uart.write((const uint8_t*)"X\n", 2);  // OpenNEK RSSI streamer: reset
  uart.flush();
  delay(20);
  uart.updateBaudRate(9600);
  // A bare CR first: the "X" above arrives as junk at 9600 and would otherwise prefix the ATZ line
  uart.write((const uint8_t*)"\r", 1);
  uart.flush();
  delay(100);
  uart.write((const uint8_t*)"ATZ\r", 4);  // stock AT firmware: reset
  uart.flush();
  uart.updateBaudRate(115200);
  baud = 115200;
  // The AT firmware takes a while between ATZ and the actual reset, so keep knocking well past it,
  // but stop at the prompt: further bytes would land in the Ymodem receiver
  bool seen = knock(4000);
  reply("\r\n~~~OK iap=%s\r\n", seen ? "seen" : "timeout");
}

static void runCommand(const char* cmd) {
  if (!strncmp(cmd, "BAUD=", 5)) {
    long b = atol(cmd + 5);
    if (b >= 300 && b <= 2000000) {
      baud = b;
      uart.updateBaudRate(baud);
      reply("~~~OK baud=%lu\r\n", (unsigned long)baud);
      return;
    }
  } else if (!strncmp(cmd, "KNOCK=", 6)) {
    bool seen = knock(atol(cmd + 6));
    reply("\r\n~~~OK knock=%s\r\n", seen ? "seen" : "timeout");
    return;
  } else if (!strcmp(cmd, "IAP")) {
    enterIap();
    return;
  } else if (!strcmp(cmd, "DIAG")) {
    reply("~~~OK boots=%lu reset_reason=%d uptime_ms=%lu usb_to_uart=%lu uart_to_usb=%lu baud=%lu\r\n", (unsigned long)bootCount,
          (int)esp_reset_reason(), (unsigned long)millis(), (unsigned long)toUart, (unsigned long)toUsb, (unsigned long)baud);
    return;
  }
  reply("~~~ERR %s\r\n", cmd);
}

// Called with the first byte after a quiet period that could start a command. Runs the command when the
// bytes form one; otherwise everything read so far is forwarded to the UART unchanged.
static void tryCommand(uint8_t first) {
  char buf[48];
  size_t len = 0;
  buf[len++] = first;
  const size_t prefixLen = strlen(CMD_PREFIX);
  uint32_t start = millis();
  while (millis() - start < CMD_TIMEOUT_MS && len < sizeof(buf) - 1) {
    if (!Serial.available()) {
      delay(1);
      continue;
    }
    char c = Serial.read();
    if (c == '\n' || c == '\r') {
      buf[len] = 0;
      if (len > prefixLen && !strncmp(buf, CMD_PREFIX, prefixLen)) {
        runCommand(buf + prefixLen);
        return;
      }
      buf[len++] = c;
      break;
    }
    buf[len++] = c;
    // Stop holding bytes back as soon as they can no longer be a command
    if (len <= prefixLen && strncmp(buf, CMD_PREFIX, len)) break;
  }
  uart.write((const uint8_t*)buf, len);
  toUart += len;
}

void setup() {
  if (bootMagic != 0x0BE11E5E) {
    bootMagic = 0x0BE11E5E;
    bootCount = 0;
  }
  bootCount++;
  Serial.setRxBufferSize(4096);
  Serial.setTxBufferSize(4096);
  Serial.begin();
  uart.setRxBufferSize(4096);
  uart.setTxBufferSize(4096);
  uart.begin(baud, SERIAL_8N1, BRIDGE_RX, BRIDGE_TX);
}

void loop() {
  uint8_t buf[256];
  size_t n;
  forwardUart();
  while (Serial.available() > 0) {
    bool quiet = millis() - lastUsbRx >= GUARD_MS;
    lastUsbRx = millis();
    if (quiet && Serial.peek() == '~') {
      tryCommand(Serial.read());
      lastUsbRx = millis();
      continue;
    }
    n = Serial.read(buf, min((size_t)Serial.available(), sizeof(buf)));
    uart.write(buf, n);
    toUart += n;
  }
  delay(1);
}
