// OpenNEK デバイス側 API。ID は MAC から、鍵とシーケンス番号は NVS に保存する
#pragma once
#include <Arduino.h>

#include "OpenNekFrame.h"
#include "OpenNekRadio.h"

namespace onek {

enum class SendResult { Ok, BadPayload, RadioError, DownlinkTimeout };

class Device {
 public:
  bool begin();

  uint32_t id() const { return id_; }
  const uint8_t* key() const { return key_; }
  uint16_t nextSeq() const { return seq_; }
  uint8_t channel() const { return radio.channel(); }
  uint32_t downlinkWaitMs() const { return dlWaitMs_; }

  bool setChannel(uint8_t ch);
  void setKey(const uint8_t key[KEY_LEN]);
  void setDownlinkWaitMs(uint32_t ms);

  // 0〜12 バイトを送る。downlink が非 null ならダウンリンクを要求し、8 バイトを受け取るまで待つ
  SendResult send(const uint8_t* data, size_t len, uint8_t* downlink = nullptr, bool oob = false);

 private:
  uint32_t id_ = 0;
  uint8_t key_[KEY_LEN] = {};
  uint16_t seq_ = 0;
  uint32_t dlWaitMs_ = 8000;
};

// 16 進文字列 ⇔ バイト列
size_t parseHex(const char* s, uint8_t* out, size_t max);  // 不正なら SIZE_MAX
void formatHex(const uint8_t* data, size_t len, char* out, bool upper = true, char sep = 0);

}  // namespace onek
