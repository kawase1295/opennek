#include "OpenNekFrame.h"

#include <string.h>

#ifdef ARDUINO
#include <mbedtls/md.h>
#endif

namespace onek {

#ifdef ARDUINO
void hmacSha256(const uint8_t* key, size_t keyLen, const uint8_t* data, size_t len, uint8_t out[32]) {
  mbedtls_md_hmac(mbedtls_md_info_from_type(MBEDTLS_MD_SHA256), key, keyLen, data, len, out);
}
#endif

static void computeMac(const uint8_t* buf, size_t signedLen, const uint8_t key[KEY_LEN], uint8_t mac[MAC_LEN]) {
  uint8_t tmp[MAX_FRAME];
  memcpy(tmp, buf, signedLen);
  tmp[4] &= ~REPEAT_MASK;  // repeat 番号は MAC に含めない
  uint8_t digest[32];
  hmacSha256(key, KEY_LEN, tmp, signedLen, digest);
  memcpy(mac, digest, MAC_LEN);
}

size_t encode(const Frame& f, const uint8_t key[KEY_LEN], uint8_t repeat, uint8_t* out) {
  size_t max = f.type == TYPE_UPLINK ? MAX_UPLINK_PAYLOAD : DOWNLINK_PAYLOAD;
  if (f.len > max) return 0;
  out[0] = MAGIC0;
  out[1] = MAGIC1;
  out[2] = VERSION;
  out[3] = f.type;
  out[4] = (f.flags & ~REPEAT_MASK) | ((repeat & 0x03) << 1);
  out[5] = f.deviceId & 0xFF;
  out[6] = (f.deviceId >> 8) & 0xFF;
  out[7] = (f.deviceId >> 16) & 0xFF;
  out[8] = (f.deviceId >> 24) & 0xFF;
  uint16_t seq = f.seq % SEQ_MOD;
  out[9] = seq & 0xFF;
  out[10] = seq >> 8;
  out[11] = f.len;
  memcpy(out + HEADER_LEN, f.payload, f.len);
  computeMac(out, HEADER_LEN + f.len, key, out + HEADER_LEN + f.len);
  return HEADER_LEN + f.len + MAC_LEN;
}

bool decode(const uint8_t* buf, size_t len, Frame& out) {
  if (len < HEADER_LEN + MAC_LEN || len > MAX_FRAME) return false;
  if (buf[0] != MAGIC0 || buf[1] != MAGIC1 || buf[2] != VERSION) return false;
  if (buf[3] != TYPE_UPLINK && buf[3] != TYPE_DOWNLINK) return false;
  uint8_t plen = buf[11];
  if (plen > (buf[3] == TYPE_UPLINK ? MAX_UPLINK_PAYLOAD : DOWNLINK_PAYLOAD)) return false;
  if (len != HEADER_LEN + plen + MAC_LEN) return false;
  out.type = buf[3];
  out.flags = buf[4] & ~REPEAT_MASK;
  out.deviceId = (uint32_t)buf[5] | ((uint32_t)buf[6] << 8) | ((uint32_t)buf[7] << 16) | ((uint32_t)buf[8] << 24);
  out.seq = ((uint16_t)buf[9] | ((uint16_t)buf[10] << 8)) % SEQ_MOD;
  out.len = plen;
  memcpy(out.payload, buf + HEADER_LEN, plen);
  return true;
}

bool verify(const uint8_t* buf, size_t len, const uint8_t key[KEY_LEN]) {
  if (len < HEADER_LEN + MAC_LEN || len > MAX_FRAME) return false;
  uint8_t mac[MAC_LEN];
  computeMac(buf, len - MAC_LEN, key, mac);
  uint8_t diff = 0;
  for (size_t i = 0; i < MAC_LEN; i++) diff |= mac[i] ^ buf[len - MAC_LEN + i];
  return diff == 0;
}

}  // namespace onek
