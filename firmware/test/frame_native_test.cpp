// ホスト PC でフレーム実装を検証する (ホスト側 TypeScript と同じゴールデンベクタ)
//   cd firmware/test && uvx --from ziglang python -m ziglang c++ -std=c++17 -I ../lib/OpenNEK/src frame_native_test.cpp ../lib/OpenNEK/src/OpenNekFrame.cpp -o frame_test && ./frame_test
#include <stdio.h>
#include <string.h>

#include "OpenNekFrame.h"

// --- テスト用の最小 SHA-256 / HMAC (ESP32 では mbedtls を使う)
static const uint32_t K[64] = {
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be,
    0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa,
    0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85,
    0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
    0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f,
    0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2};
static uint32_t ror(uint32_t x, int n) { return (x >> n) | (x << (32 - n)); }

static void sha256(const uint8_t* data, size_t len, uint8_t out[32]) {
  uint32_t h[8] = {0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19};
  size_t total = ((len + 9 + 63) / 64) * 64;
  uint8_t msg[256] = {};
  memcpy(msg, data, len);
  msg[len] = 0x80;
  uint64_t bits = (uint64_t)len * 8;
  for (int i = 0; i < 8; i++) msg[total - 1 - i] = bits >> (8 * i);
  for (size_t off = 0; off < total; off += 64) {
    uint32_t w[64];
    for (int i = 0; i < 16; i++) w[i] = (msg[off + i * 4] << 24) | (msg[off + i * 4 + 1] << 16) | (msg[off + i * 4 + 2] << 8) | msg[off + i * 4 + 3];
    for (int i = 16; i < 64; i++) {
      uint32_t s0 = ror(w[i - 15], 7) ^ ror(w[i - 15], 18) ^ (w[i - 15] >> 3);
      uint32_t s1 = ror(w[i - 2], 17) ^ ror(w[i - 2], 19) ^ (w[i - 2] >> 10);
      w[i] = w[i - 16] + s0 + w[i - 7] + s1;
    }
    uint32_t a = h[0], b = h[1], c = h[2], d = h[3], e = h[4], f = h[5], g = h[6], hh = h[7];
    for (int i = 0; i < 64; i++) {
      uint32_t t1 = hh + (ror(e, 6) ^ ror(e, 11) ^ ror(e, 25)) + ((e & f) ^ (~e & g)) + K[i] + w[i];
      uint32_t t2 = (ror(a, 2) ^ ror(a, 13) ^ ror(a, 22)) + ((a & b) ^ (a & c) ^ (b & c));
      hh = g, g = f, f = e, e = d + t1, d = c, c = b, b = a, a = t1 + t2;
    }
    h[0] += a, h[1] += b, h[2] += c, h[3] += d, h[4] += e, h[5] += f, h[6] += g, h[7] += hh;
  }
  for (int i = 0; i < 8; i++)
    for (int j = 0; j < 4; j++) out[i * 4 + j] = h[i] >> (24 - 8 * j);
}

void onek::hmacSha256(const uint8_t* key, size_t keyLen, const uint8_t* data, size_t len, uint8_t out[32]) {
  uint8_t pad[64] = {}, buf[64 + 128], inner[32];
  memcpy(pad, key, keyLen);
  for (int i = 0; i < 64; i++) buf[i] = pad[i] ^ 0x36;
  memcpy(buf + 64, data, len);
  sha256(buf, 64 + len, inner);
  for (int i = 0; i < 64; i++) buf[i] = pad[i] ^ 0x5c;
  memcpy(buf + 64, inner, 32);
  sha256(buf, 96, out);
}

static int failures = 0;
#define CHECK(cond)                                         \
  do {                                                      \
    if (!(cond)) {                                          \
      printf("FAIL %s:%d: %s\n", __FILE__, __LINE__, #cond); \
      failures++;                                           \
    }                                                       \
  } while (0)

static void hex(const uint8_t* b, size_t n, char* out) {
  for (size_t i = 0; i < n; i++) sprintf(out + i * 2, "%02x", b[i]);
}

int main() {
  const uint8_t key[16] = {0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15};

  // host/test/__snapshots__/frame.test.ts.snap と同じ
  onek::Frame f;
  f.type = onek::TYPE_DOWNLINK;
  f.deviceId = 0x0A1B2C3D;
  f.seq = 0x123;
  f.len = 8;
  for (int i = 0; i < 8; i++) f.payload[i] = i + 1;
  uint8_t buf[onek::MAX_FRAME];
  size_t n = onek::encode(f, key, 0, buf);
  char s[onek::MAX_FRAME * 2 + 1];
  hex(buf, n, s);
  printf("downlink golden: %s\n", s);
  CHECK(!strcmp(s, "4e4b0102003d2c1b0a230108010203040506070892cafeba"));

  // repeat が違っても MAC は同じで検証が通る
  onek::Frame up;
  up.flags = onek::FLAG_ACK;
  up.deviceId = 0x0A1B2C3D;
  up.seq = 4097;
  up.len = 2;
  up.payload[0] = 0xCA;
  up.payload[1] = 0xFE;
  uint8_t r0[onek::MAX_FRAME], r2[onek::MAX_FRAME];
  size_t n0 = onek::encode(up, key, 0, r0), n2 = onek::encode(up, key, 2, r2);
  CHECK(n0 == 18 && n2 == 18);
  CHECK(r2[4] == (onek::FLAG_ACK | 0x04));
  CHECK(!memcmp(r0 + 14, r2 + 14, 4));
  CHECK(onek::verify(r2, n2, key));
  onek::Frame d;
  CHECK(onek::decode(r2, n2, d) && d.seq == 1 && d.flags == onek::FLAG_ACK && d.len == 2 && d.deviceId == 0x0A1B2C3D);

  r2[12] ^= 1;
  CHECK(!onek::verify(r2, n2, key));
  CHECK(!onek::decode(r2, n2 - 1, d));
  up.len = 13;
  CHECK(onek::encode(up, key, 0, r0) == 0);

  printf(failures ? "%d FAILED\n" : "all passed\n", failures);
  return failures != 0;
}
