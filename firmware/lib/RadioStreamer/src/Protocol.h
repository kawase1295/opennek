// The OpenNEK RSSI streamer's serial protocol (115200 bps, one line per command or report), shared by every
// radio that speaks it. See firmware-lsm100a/README.md for the table; host/crates/opennek-core/src/radio.rs is the
// other end. No I/O here: the firmware feeds characters in and writes the formatted lines out.
#pragma once

#include <stddef.h>
#include <stdint.h>

namespace streamer {

constexpr uint8_t kLineSamples = 64;  // RSSI samples per R line
constexpr uint8_t kCfoEvery = 8;      // one CFO value per this many samples
constexpr uint8_t kDownlinkBytes = 15;
// "R" + seq + ":" + 2 hex digits per sample + "\n"
constexpr size_t kMaxLine = 1 + 4 + 1 + 2 * kLineSamples + 1;

struct Settings {
  uint32_t freqHz = 923200000;  // Sigfox RC3 uplink centre
  uint8_t bwCode = 0x0A;        // SX126x RX_BW 234.3 kHz covers the 192 kHz uplink band
  uint32_t sampleHz = 2000;     // 20 samples per bit at 100 bps
  bool boost = true;            // boosted LNA gain
  bool cfo = false;             // C lines
  uint8_t preambleDetector = 0x04;
  bool txEnabled = false;  // off after every reset
};

// What the radio has to do after a command
enum class Action { None, Status, Reconfigure, Transmit, Reset };

// Applies one command line (without its line ending) to the settings. Values out of range are ignored, as the
// LSM100A streamer does; the radio is still reconfigured and the status reported.
Action applyCommand(const char* line, Settings& s);

struct Downlink {
  uint32_t hz;
  int32_t dbm;
  uint8_t frame[kDownlinkBytes];
};

// The arguments of T: <hz>,<dBm>,<30 hex digits>
bool parseDownlink(const char* args, Downlink& out);

// Collects command characters into lines. Characters past the buffer are dropped.
class LineReader {
 public:
  // The completed line at '\r' or '\n' (valid until the next push), or nullptr
  const char* push(char c);

 private:
  char buf_[64];
  uint8_t len_ = 0;
};

enum class TxResult { Ok, Busy, Off, Range, Limit, Err };

// Output lines, each ending in '\n'. They return the length written (without a terminating NUL).
size_t formatStatus(char* out, size_t size, const Settings& s, const char* fw, const char* hw);
// out holds at least kMaxLine characters; n <= kLineSamples
size_t formatSamples(char* out, uint16_t seq, const uint8_t* rssi, uint8_t n);
// 12-bit values as 3 hex digits each
size_t formatCfo(char* out, uint16_t seq, const uint16_t* cfo, uint8_t n);
// value: the on-air time in ms for Ok, the strongest raw RSSI for Busy, unused otherwise
size_t formatTxReply(char* out, size_t size, TxResult r, uint32_t value);

}  // namespace streamer
