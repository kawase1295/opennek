#include "Sx126x.h"

#include "Protocol.h"

namespace sx126x {

static void put24(uint8_t* p, uint32_t v) {
  p[0] = v >> 16;
  p[1] = v >> 8;
  p[2] = v;
}

// Frequencies and deviations in units of 32 MHz / 2^25
static uint32_t pllSteps(uint32_t hz) { return (uint32_t)(((uint64_t)hz << 25) / kXtalHz); }

void frequency(uint32_t hz, uint8_t out[4]) {
  uint32_t frf = pllSteps(hz);
  out[0] = frf >> 24;
  out[1] = frf >> 16;
  out[2] = frf >> 8;
  out[3] = frf;
}

void imageCalibration(uint32_t hz, uint8_t out[2]) {
  if (hz > 900000000) {
    out[0] = 0xE1, out[1] = 0xE9;
  } else if (hz > 850000000) {
    out[0] = 0xD7, out[1] = 0xDB;
  } else if (hz > 770000000) {
    out[0] = 0xC1, out[1] = 0xC5;
  } else if (hz > 460000000) {
    out[0] = 0x75, out[1] = 0x81;
  } else {
    out[0] = 0x6B, out[1] = 0x6F;
  }
}

static void gfsk(uint32_t bps, uint8_t shaping, uint8_t bwCode, uint32_t deviationHz, uint8_t out[8]) {
  put24(out, 32 * kXtalHz / bps);
  out[3] = shaping;
  out[4] = bwCode;
  put24(out + 5, pllSteps(deviationHz));
}

void rxModulation(uint8_t bwCode, uint8_t out[8]) { gfsk(50000, 0x00, bwCode, 25000, out); }

void txModulation(uint8_t bwCode, uint8_t out[8]) { gfsk(600, 0x0B, bwCode, 800, out); }

void rxPacket(uint8_t preambleDetector, uint8_t out[9]) {
  // The bit rate and sync word are irrelevant: nothing is meant to be received as a packet
  const uint8_t pkt[9] = {0x00, 0x08, preambleDetector, 0x40, 0x00, 0x00, 0xFF, 0x01, 0x00};
  for (int i = 0; i < 9; i++) out[i] = pkt[i];
}

void txPacket(uint8_t out[9]) {
  const uint8_t pkt[9] = {0x00, 0x10, 0x00, 0x10, 0x00, 0x00, streamer::kDownlinkBytes, 0x01, 0x00};
  for (int i = 0; i < 9; i++) out[i] = pkt[i];
}

void paConfig(uint8_t out[4]) {
  out[0] = 0x04;  // paDutyCycle
  out[1] = 0x07;  // hpMax
  out[2] = 0x00;  // SX1262
  out[3] = 0x01;
}

void txParams(int32_t dbm, uint8_t out[2]) {
  out[0] = (uint8_t)(int8_t)dbm;
  out[1] = 0x02;  // 40 us ramp
}

void steps(uint32_t n, uint8_t out[3]) { put24(out, n); }

}  // namespace sx126x
