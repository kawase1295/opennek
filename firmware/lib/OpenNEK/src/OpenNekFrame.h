// OpenNEK 無線フレーム (docs/protocol.md の (1))
#pragma once
#include <stddef.h>
#include <stdint.h>

namespace onek {

constexpr uint8_t MAGIC0 = 0x4E;
constexpr uint8_t MAGIC1 = 0x4B;
constexpr uint8_t VERSION = 1;
constexpr uint8_t TYPE_UPLINK = 0x01;
constexpr uint8_t TYPE_DOWNLINK = 0x02;
constexpr uint8_t FLAG_ACK = 0x01;
constexpr uint8_t FLAG_OOB = 0x08;
constexpr uint8_t REPEAT_MASK = 0x06;
constexpr size_t HEADER_LEN = 12;
constexpr size_t MAC_LEN = 4;
constexpr size_t KEY_LEN = 16;
constexpr size_t MAX_UPLINK_PAYLOAD = 12;
constexpr size_t DOWNLINK_PAYLOAD = 8;
constexpr size_t MAX_FRAME = HEADER_LEN + MAX_UPLINK_PAYLOAD + MAC_LEN;
constexpr uint16_t SEQ_MOD = 4096;

struct Frame {
  uint8_t type = TYPE_UPLINK;
  uint8_t flags = 0;  // FLAG_ACK | FLAG_OOB (repeat ビットは encode の引数で渡す)
  uint32_t deviceId = 0;
  uint16_t seq = 0;
  uint8_t len = 0;
  uint8_t payload[MAX_UPLINK_PAYLOAD] = {};
};

// HMAC-SHA256 の実装は環境ごとに差し替える (ESP32 は mbedtls、ネイティブテストは自前実装)
void hmacSha256(const uint8_t* key, size_t keyLen, const uint8_t* data, size_t len, uint8_t out[32]);

// out には MAX_FRAME バイト以上を用意する。戻り値はフレーム長 (ペイロード長が不正なら 0)
size_t encode(const Frame& f, const uint8_t key[KEY_LEN], uint8_t repeat, uint8_t* out);

// 構造だけを検査してデコードする (鍵の検証は verify)
bool decode(const uint8_t* buf, size_t len, Frame& out);

// MAC を検証する。decode が成功したバッファに対して呼ぶ
bool verify(const uint8_t* buf, size_t len, const uint8_t key[KEY_LEN]);

}  // namespace onek
