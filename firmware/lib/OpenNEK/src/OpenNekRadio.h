// ESP-NOW の薄いラッパー。受信はコールバックから FreeRTOS キューへ積み、loop 側で取り出す
#pragma once
#include <Arduino.h>

namespace onek {

struct Packet {
  uint8_t src[6];
  int8_t rssi;
  uint8_t len;
  uint8_t data[64];
};

class Radio {
 public:
  bool begin(uint8_t channel);
  bool setChannel(uint8_t channel);
  uint8_t channel() const { return channel_; }
  // 送信して ESP-NOW の送信完了を待つ。ブロードキャストは常に成功扱い
  bool send(const uint8_t dst[6], const uint8_t* data, size_t len);
  bool receive(Packet& out, TickType_t wait = 0);
  void flush();
  void macAddress(uint8_t out[6]) const;

  static const uint8_t BROADCAST[6];

 private:
  uint8_t channel_ = 1;
};

extern Radio radio;

// "AA:BB:CC:DD:EE:FF" との相互変換
void formatMac(const uint8_t mac[6], char out[18]);
bool parseMac(const char* s, uint8_t out[6]);

}  // namespace onek
