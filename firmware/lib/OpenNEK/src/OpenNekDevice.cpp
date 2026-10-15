#include "OpenNekDevice.h"

#include <Preferences.h>
#include <esp_random.h>

namespace onek {

static Preferences prefs;
static constexpr uint32_t REPEAT_GAP_MS = 30;

bool Device::begin() {
  prefs.begin("opennek", false);
  uint8_t mac[6];
  esp_read_mac(mac, ESP_MAC_WIFI_STA);
  id_ = ((uint32_t)mac[2] << 24) | ((uint32_t)mac[3] << 16) | ((uint32_t)mac[4] << 8) | mac[5];
  if (prefs.getBytes("key", key_, KEY_LEN) != KEY_LEN) {
    esp_fill_random(key_, KEY_LEN);  // Wi-Fi 起動前でも RNG はブートローダのエントロピーで初期化済み
    prefs.putBytes("key", key_, KEY_LEN);
  }
  seq_ = prefs.getUShort("seq", 0) % SEQ_MOD;
  dlWaitMs_ = prefs.getUInt("dlwait", 8000);
  return radio.begin(prefs.getUChar("ch", 1));
}

bool Device::setChannel(uint8_t ch) {
  if (!radio.setChannel(ch)) return false;
  prefs.putUChar("ch", ch);
  return true;
}

void Device::setKey(const uint8_t key[KEY_LEN]) {
  memcpy(key_, key, KEY_LEN);
  prefs.putBytes("key", key_, KEY_LEN);
}

void Device::setDownlinkWaitMs(uint32_t ms) {
  dlWaitMs_ = ms;
  prefs.putUInt("dlwait", ms);
}

SendResult Device::send(const uint8_t* data, size_t len, uint8_t* downlink, bool oob) {
  if (len > MAX_UPLINK_PAYLOAD) return SendResult::BadPayload;
  Frame f;
  f.type = TYPE_UPLINK;
  f.flags = (downlink ? FLAG_ACK : 0) | (oob ? FLAG_OOB : 0);
  f.deviceId = id_;
  f.seq = seq_;
  f.len = len;
  memcpy(f.payload, data, len);
  seq_ = (seq_ + 1) % SEQ_MOD;
  prefs.putUShort("seq", seq_);

  radio.flush();
  uint8_t buf[MAX_FRAME];
  bool anySent = false;
  for (uint8_t repeat = 0; repeat < 3; repeat++) {
    size_t n = encode(f, key_, repeat, buf);
    anySent |= radio.send(Radio::BROADCAST, buf, n);
    if (repeat < 2) delay(REPEAT_GAP_MS);
  }
  if (!anySent) return SendResult::RadioError;
  if (!downlink) return SendResult::Ok;

  uint32_t start = millis();
  Packet p;
  while (millis() - start < dlWaitMs_) {
    if (!radio.receive(p, pdMS_TO_TICKS(50))) continue;
    Frame d;
    if (!decode(p.data, p.len, d) || d.type != TYPE_DOWNLINK || d.deviceId != id_ || d.seq != f.seq) continue;
    if (!verify(p.data, p.len, key_)) continue;
    memcpy(downlink, d.payload, DOWNLINK_PAYLOAD);
    return SendResult::Ok;
  }
  return SendResult::DownlinkTimeout;
}

size_t parseHex(const char* s, uint8_t* out, size_t max) {
  size_t n = strlen(s);
  if (n % 2 || n / 2 > max) return SIZE_MAX;
  for (size_t i = 0; i < n / 2; i++) {
    int v = 0;
    for (int k = 0; k < 2; k++) {
      char c = s[i * 2 + k];
      int d = c >= '0' && c <= '9' ? c - '0' : c >= 'a' && c <= 'f' ? c - 'a' + 10 : c >= 'A' && c <= 'F' ? c - 'A' + 10 : -1;
      if (d < 0) return SIZE_MAX;
      v = v * 16 + d;
    }
    out[i] = v;
  }
  return n / 2;
}

void formatHex(const uint8_t* data, size_t len, char* out, bool upper, char sep) {
  const char* digits = upper ? "0123456789ABCDEF" : "0123456789abcdef";
  for (size_t i = 0; i < len; i++) {
    if (sep && i) *out++ = sep;
    *out++ = digits[data[i] >> 4];
    *out++ = digits[data[i] & 0x0F];
  }
  *out = 0;
}

}  // namespace onek
