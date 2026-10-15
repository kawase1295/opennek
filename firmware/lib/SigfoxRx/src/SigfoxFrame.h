// Sigfox uplink frame layout, as built by sigfox-ep-lib (github.com/sigfox-tech-radio/sigfox-ep-lib):
//   PR (0xAAAA) | FT (16 bit) | LI(2) BF(1) REP(1) MC(12) | EP ID (4, LSB first) | payload | auth | CRC16
// Repetitions 2 and 3 (ranks 1 and 2 here) convolve everything after FT.
// Port of host/src/sigfox/frame.ts; bit strings are '0'/'1' characters, as there.
#pragma once
#include <stddef.h>
#include <stdint.h>

#include <string>
#include <vector>

namespace sigfox {

struct UplinkFrame {
  int rank = 0;           // 0, 1, 2 = first, second, third repetition
  uint32_t deviceId = 0;  // as printed by AT$ID
  uint16_t counter = 0;
  std::vector<uint8_t> payload;
  int bit = -1;  // 0 or 1 for single-bit messages (AT$SB), -1 otherwise
  bool downlink = false;  // BF: the device waits for a downlink
  std::vector<uint8_t> auth;
  bool crcOk = false;
  std::string repaired;  // bit-slip correction that made the CRC pass, empty if none
};

uint16_t crc16(const uint8_t* data, size_t len);
std::string convolve(const std::string& bits, int rank);
std::string deconvolve(const std::string& bits, int rank);

// Data bits of an uplink frame (tests). bit = 0/1 builds a single-bit message.
std::string encodeUplink(int rank, uint32_t deviceId, uint16_t counter, const std::vector<uint8_t>& payload,
                         const std::vector<uint8_t>& auth, bool downlink = false, int bit = -1);

// Decodes data bits that start somewhere in the preamble. Returns false when no frame type is found;
// otherwise `out` holds the frame, which may still have a bad CRC.
bool decodeUplinkBits(const std::string& bits, UplinkFrame& out);

}  // namespace sigfox
