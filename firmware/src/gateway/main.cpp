// OpenNEK ゲートウェイ: ESP-NOW で受けたフレームを USB シリアルの JSON Lines でホストへ渡し、
// ホストからのダウンリンクを ESP-NOW で送る単純なブリッジ (docs/protocol.md の (2))
#include <Arduino.h>
#include <ArduinoJson.h>
#include <OpenNEK.h>
#include <Preferences.h>

#ifndef OPENNEK_FW_VERSION
#define OPENNEK_FW_VERSION "dev"
#endif

using namespace onek;

static Preferences prefs;
static char line[256];
static size_t lineLen = 0;

static void printHello() {
  uint8_t mac[6];
  char macStr[18];
  radio.macAddress(mac);
  formatMac(mac, macStr);
  Serial.printf("{\"t\":\"hello\",\"fw\":\"%s\",\"radio\":\"espnow\",\"ch\":%u,\"mac\":\"%s\"}\n", OPENNEK_FW_VERSION, radio.channel(), macStr);
}

static void printLog(const char* msg) { Serial.printf("{\"t\":\"log\",\"msg\":\"%s\"}\n", msg); }

static void forwardUplinks() {
  Packet p;
  while (radio.receive(p)) {
    // OpenNEK 以外の ESP-NOW 通信は無視する
    if (p.len < HEADER_LEN + MAC_LEN || p.data[0] != MAGIC0 || p.data[1] != MAGIC1 || p.data[3] != TYPE_UPLINK) continue;
    char src[18];
    char hex[sizeof(p.data) * 2 + 1];
    formatMac(p.src, src);
    formatHex(p.data, p.len, hex);
    Serial.printf("{\"t\":\"up\",\"src\":\"%s\",\"rssi\":%d,\"frame\":\"%s\"}\n", src, p.rssi, hex);
  }
}

static void handleCommand(const char* text) {
  JsonDocument doc;
  if (deserializeJson(doc, text)) return printLog("bad json");
  const char* t = doc["t"] | "";
  if (!strcmp(t, "ping")) return printHello();
  if (!strcmp(t, "ch")) {
    uint8_t ch = doc["ch"] | 0;
    if (!radio.setChannel(ch)) return printLog("bad channel");
    prefs.putUChar("ch", ch);
    return printHello();
  }
  if (!strcmp(t, "down")) {
    const char* dstStr = doc["dst"] | "";
    const char* frameHex = doc["frame"] | "";
    uint8_t dst[6];
    uint8_t frame[MAX_FRAME];
    size_t n = parseHex(frameHex, frame, sizeof(frame));
    bool ok = parseMac(dstStr, dst) && n != SIZE_MAX && radio.send(dst, frame, n);
    Serial.printf("{\"t\":\"sent\",\"dst\":\"%s\",\"ok\":%s}\n", dstStr, ok ? "true" : "false");
    return;
  }
  printLog("unknown command");
}

static void readSerial() {
  while (Serial.available()) {
    char c = Serial.read();
    if (c == '\n' || c == '\r') {
      if (lineLen) {
        line[lineLen] = 0;
        handleCommand(line);
        lineLen = 0;
      }
    } else if (lineLen < sizeof(line) - 1) {
      line[lineLen++] = c;
    } else {
      lineLen = 0;  // 長すぎる行は捨てる
    }
  }
}

void setup() {
  Serial.setRxBufferSize(1024);
  Serial.begin(115200);
  prefs.begin("opennek-gw", false);
  if (!radio.begin(prefs.getUChar("ch", 1))) printLog("radio init failed");
  printHello();
}

void loop() {
  forwardUplinks();
  readSerial();
  delay(1);
}
