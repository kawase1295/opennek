// OpenNEK デバイス: Sigfox モデム風の AT コマンドを USB シリアルと UART の両方で受け付ける。
// 既存の Sigfox AT 端末の UART をこのボードに繋ぎ替えればそのまま送信できる。
// BOOT ボタンを押すと稼働秒数をデモ送信する。
#include <Arduino.h>
#include <OpenNEK.h>

#ifndef OPENNEK_FW_VERSION
#define OPENNEK_FW_VERSION "dev"
#endif
#ifndef AT_UART_RX
#define AT_UART_RX 16
#endif
#ifndef AT_UART_TX
#define AT_UART_TX 17
#endif
#ifndef AT_UART_BAUD
#define AT_UART_BAUD 9600
#endif
#ifndef DEMO_BUTTON
#define DEMO_BUTTON 0
#endif

using namespace onek;

static Device device;
static HardwareSerial atUart(1);

struct Port {
  Stream& io;
  char buf[96];
  size_t len;
};
static Port usbPort{Serial, {}, 0};
static Port uartPort{atUart, {}, 0};

static void reply(Port& p, const char* s) {
  p.io.print(s);
  p.io.print("\r\n");
}

static void replyf(Port& p, const char* fmt, ...) {
  char s[96];
  va_list ap;
  va_start(ap, fmt);
  vsnprintf(s, sizeof(s), fmt, ap);
  va_end(ap);
  reply(p, s);
}

// 送信結果を Sigfox モデムと同じ形 (OK のあとに RX=xx xx ...) で返す
static void transmit(Port& p, const uint8_t* data, size_t len, bool wantDownlink, bool oob = false) {
  uint8_t dl[DOWNLINK_PAYLOAD];
  SendResult r = device.send(data, len, wantDownlink ? dl : nullptr, oob);
  switch (r) {
    case SendResult::Ok:
      reply(p, "OK");
      if (wantDownlink) {
        char hex[DOWNLINK_PAYLOAD * 3];
        formatHex(dl, DOWNLINK_PAYLOAD, hex, true, ' ');
        replyf(p, "RX=%s", hex);
      }
      break;
    case SendResult::BadPayload:
      reply(p, "ERROR: payload must be 0-12 bytes");
      break;
    case SendResult::RadioError:
      reply(p, "ERROR: radio");
      break;
    case SendResult::DownlinkTimeout:
      reply(p, "ERROR: downlink timeout");
      break;
  }
}

// "xxxx" または "xxxx,1" を分解する
static bool splitAck(char* arg, bool& ack) {
  ack = false;
  char* comma = strchr(arg, ',');
  if (!comma) return true;
  *comma = 0;
  if (!strcmp(comma + 1, "1")) ack = true;
  else if (strcmp(comma + 1, "0")) return false;
  return true;
}

static void handle(Port& p, char* cmd) {
  // 大文字小文字を区別しない (16 進の引数は parseHex がどちらも受ける)
  for (char* c = cmd; *c; c++) *c = toupper(*c);

  if (!strcmp(cmd, "AT") || !strcmp(cmd, "ATZ")) return reply(p, "OK");
  if (!strcmp(cmd, "AT$ID") || !strcmp(cmd, "AT$I=10")) return replyf(p, "%08X", device.id());
  if (!strcmp(cmd, "AT$PAC") || !strcmp(cmd, "AT$I=11")) {
    char hex[KEY_LEN * 2 + 1];
    formatHex(device.key(), KEY_LEN, hex);
    return reply(p, hex);
  }
  if (!strcmp(cmd, "AT$I=0")) return reply(p, "OpenNEK");
  if (!strcmp(cmd, "AT$I=7") || !strcmp(cmd, "AT+VER=?") || !strcmp(cmd, "AT$V")) return reply(p, OPENNEK_FW_VERSION);

  if (!strncmp(cmd, "AT$SF=", 6)) {
    char* arg = cmd + 6;
    bool ack;
    uint8_t data[MAX_UPLINK_PAYLOAD];
    size_t n;
    if (!splitAck(arg, ack) || (n = parseHex(arg, data, sizeof(data))) == SIZE_MAX) return reply(p, "ERROR: AT$SF=<hex 0-12 bytes>[,1]");
    return transmit(p, data, n, ack);
  }
  if (!strncmp(cmd, "AT$SB=", 6)) {
    char* arg = cmd + 6;
    bool ack;
    if (!splitAck(arg, ack) || (strcmp(arg, "0") && strcmp(arg, "1"))) return reply(p, "ERROR: AT$SB=<0|1>[,1]");
    uint8_t bit = arg[0] == '1';
    return transmit(p, &bit, 1, ack);
  }
  if (!strcmp(cmd, "AT$SO")) return transmit(p, nullptr, 0, false, true);

  // OpenNEK 独自: チャンネル・鍵・ダウンリンク待ち時間・シーケンス番号
  if (!strcmp(cmd, "AT$CH?")) return replyf(p, "%u", device.channel());
  if (!strncmp(cmd, "AT$CH=", 6)) {
    int ch = atoi(cmd + 6);
    return reply(p, ch >= 1 && ch <= 13 && device.setChannel(ch) ? "OK" : "ERROR: channel must be 1-13");
  }
  if (!strncmp(cmd, "AT$KEY=", 7)) {
    uint8_t key[KEY_LEN];
    if (parseHex(cmd + 7, key, KEY_LEN) != KEY_LEN) return reply(p, "ERROR: AT$KEY=<32 hex>");
    device.setKey(key);
    return reply(p, "OK");
  }
  if (!strcmp(cmd, "AT$DLW?")) return replyf(p, "%u", device.downlinkWaitMs());
  if (!strncmp(cmd, "AT$DLW=", 7)) {
    long ms = atol(cmd + 7);
    if (ms < 500 || ms > 60000) return reply(p, "ERROR: 500-60000 ms");
    device.setDownlinkWaitMs(ms);
    return reply(p, "OK");
  }
  if (!strcmp(cmd, "AT$SEQ?")) return replyf(p, "%u", device.nextSeq());

  if (!strcmp(cmd, "AT$T?")) return replyf(p, "%d", (int)(temperatureRead() * 10));
  if (!strcmp(cmd, "AT$V?")) return reply(p, "3300");

  // 実モデムの設定系コマンドは受け流す (端末側の初期化シーケンスが止まらないように)
  if (!strncmp(cmd, "AT$RC", 5) || !strncmp(cmd, "AT$P=", 5) || !strncmp(cmd, "AT+MODE", 7) || !strcmp(cmd, "AT$WR") ||
      !strncmp(cmd, "AT$TR", 5) || !strncmp(cmd, "ATS3", 4)) {
    if (strchr(cmd, '?')) return reply(p, "0");
    return reply(p, "OK");
  }
  reply(p, "ERROR");
}

static void pump(Port& p) {
  while (p.io.available()) {
    char c = p.io.read();
    if (c == '\r' || c == '\n') {
      if (p.len) {
        p.buf[p.len] = 0;
        p.len = 0;
        handle(p, p.buf);
      }
    } else if (c >= 0x20 && c < 0x7F && p.len < sizeof(p.buf) - 1) {
      p.buf[p.len++] = c;
    }
  }
}

static void demoButton() {
  static bool last = true;
  static uint32_t lastChange = 0;
  bool now = digitalRead(DEMO_BUTTON);
  if (now == last || millis() - lastChange < 50) return;
  last = now;
  lastChange = millis();
  if (now) return;  // 押した瞬間だけ
  uint32_t up = millis() / 1000;
  uint8_t data[4] = {(uint8_t)(up >> 24), (uint8_t)(up >> 16), (uint8_t)(up >> 8), (uint8_t)up};
  Serial.printf("# demo uplink: uptime=%lu s\r\n", (unsigned long)up);
  transmit(usbPort, data, sizeof(data), false);
}

void setup() {
  Serial.begin(115200);
  atUart.begin(AT_UART_BAUD, SERIAL_8N1, AT_UART_RX, AT_UART_TX);
  pinMode(DEMO_BUTTON, INPUT_PULLUP);
  if (!device.begin()) Serial.println("# radio init failed");
  Serial.printf("# OpenNEK device %s  ID=%08X  ch=%u  (AT$PAC shows the key)\r\n", OPENNEK_FW_VERSION, device.id(), device.channel());
}

void loop() {
  pump(usbPort);
  pump(uartPort);
  demoButton();
  delay(1);
}
