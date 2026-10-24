// Checks the radio-independent part of the RSSI streamer (lib/RadioStreamer): the serial protocol, the transmit
// limits, the SX126x parameter bytes and the sample clock. Run on the PC:
//   cd firmware/test && uvx --from ziglang python -m ziglang c++ -std=c++17 -O2 -I ../lib/RadioStreamer/src \
//     streamer_native_test.cpp ../lib/RadioStreamer/src/*.cpp -o streamer_test && ./streamer_test
#include <stdio.h>
#include <string.h>

#include <string>
#include <vector>

#include "Protocol.h"
#include "SampleClock.h"
#include "Sx126x.h"
#include "TxPolicy.h"

using namespace streamer;

static int failures = 0;
#define CHECK(cond)                                                \
  do {                                                             \
    if (!(cond)) {                                                 \
      printf("FAIL %s:%d: %s\n", __FILE__, __LINE__, #cond);      \
      failures++;                                                  \
    }                                                              \
  } while (0)

template <size_t N>
static std::vector<uint8_t> bytes(const uint8_t (&b)[N]) {
  return std::vector<uint8_t>(b, b + N);
}

static void testCommands() {
  Settings s;
  CHECK(applyCommand("F922200000", s) == Action::Reconfigure && s.freqHz == 922200000);
  CHECK(applyCommand("F100", s) == Action::Reconfigure && s.freqHz == 922200000);
  CHECK(applyCommand("F970000000", s) == Action::Reconfigure && s.freqHz == 922200000);
  CHECK(applyCommand("W31", s) == Action::Reconfigure && s.bwCode == 31);
  CHECK(applyCommand("W32", s) == Action::Reconfigure && s.bwCode == 31);
  CHECK(applyCommand("S4000", s) == Action::Reconfigure && s.sampleHz == 4000);
  CHECK(applyCommand("S99", s) == Action::Reconfigure && s.sampleHz == 4000);
  CHECK(applyCommand("B0", s) == Action::Reconfigure && !s.boost);
  CHECK(applyCommand("C1", s) == Action::Reconfigure && s.cfo);
  CHECK(applyCommand("P7", s) == Action::Reconfigure && s.preambleDetector == 7);
  CHECK(applyCommand("P8", s) == Action::Reconfigure && s.preambleDetector == 7);
  CHECK(applyCommand("E1", s) == Action::Reconfigure && s.txEnabled);
  CHECK(applyCommand("E0", s) == Action::Reconfigure && !s.txEnabled);
  CHECK(applyCommand("?", s) == Action::Status);
  CHECK(applyCommand("X", s) == Action::Reset);
  CHECK(applyCommand("T922200000,10,00", s) == Action::Transmit);
  CHECK(applyCommand("Z1", s) == Action::None);
  CHECK(applyCommand("", s) == Action::None);
}

static void testDownlink() {
  Downlink d;
  CHECK(parseDownlink("922200000,10,AAAAB2270102030405060708ABcdEF", d));
  CHECK(d.hz == 922200000 && d.dbm == 10);
  CHECK(d.frame[0] == 0xAA && d.frame[3] == 0x27 && d.frame[12] == 0xAB && d.frame[14] == 0xEF);
  CHECK(parseDownlink("922200000,-5,000000000000000000000000000000", d) && d.dbm == -5);
  CHECK(!parseDownlink("922200000 10,000000000000000000000000000000", d));
  CHECK(!parseDownlink("922200000,10;000000000000000000000000000000", d));
  CHECK(!parseDownlink("922200000,10,00000000000000000000000000000", d));    // 29 digits
  CHECK(!parseDownlink("922200000,10,0000000000000000000000000000000", d));  // 31 digits
  CHECK(!parseDownlink("922200000,10,00000000000000000000000000000G", d));
}

static void testLineReader() {
  LineReader r;
  CHECK(r.push('F') == nullptr && r.push('1') == nullptr);
  const char* line = r.push('\r');
  CHECK(line && strcmp(line, "F1") == 0);
  CHECK(r.push('\n') == nullptr);  // the empty line after \r
  for (int i = 0; i < 100; i++) r.push('x');
  line = r.push('\n');
  CHECK(line && strlen(line) == 63);
}

static void testFormat() {
  char out[kMaxLine + 1] = {};
  Settings s;
  size_t n = formatStatus(out, sizeof(out), s, "0.3.0", "xiao-sx1262");
  CHECK(std::string(out, n) == "I fw=0.3.0 f=923200000 bw=10 sr=2000 boost=1 cfo=0 pd=4 tx=0 hw=xiao-sx1262\n");

  const uint8_t rssi[3] = {0x00, 0xA0, 0xFF};
  n = formatSamples(out, 0x1A2B, rssi, 3);
  CHECK(std::string(out, n) == "R1A2B:00A0FF\n");
  uint8_t full[kLineSamples];
  memset(full, 0x9C, sizeof(full));
  CHECK(formatSamples(out, 0, full, kLineSamples) == kMaxLine && out[kMaxLine - 1] == '\n');

  const uint16_t cfo[2] = {0x001, 0xFFFF};
  n = formatCfo(out, 1, cfo, 2);
  CHECK(std::string(out, n) == "C0001:001FFF\n");

  n = formatTxReply(out, sizeof(out), TxResult::Ok, 280);
  CHECK(std::string(out, n) == "D ok ms=280\n");
  n = formatTxReply(out, sizeof(out), TxResult::Busy, 161);
  CHECK(std::string(out, n) == "D busy rssi=-80.5\n");
  n = formatTxReply(out, sizeof(out), TxResult::Busy, 160);
  CHECK(std::string(out, n) == "D busy rssi=-80.0\n");
  n = formatTxReply(out, sizeof(out), TxResult::Off, 0);
  CHECK(std::string(out, n) == "D off\n");
  n = formatTxReply(out, sizeof(out), TxResult::Range, 0);
  CHECK(std::string(out, n) == "D range\n");
  n = formatTxReply(out, sizeof(out), TxResult::Limit, 0);
  CHECK(std::string(out, n) == "D limit\n");
  n = formatTxReply(out, sizeof(out), TxResult::Err, 0);
  CHECK(std::string(out, n) == "D err\n");
}

static void testTxPolicy() {
  TxPolicy p;
  CHECK(p.check(false, 922200000, 0) == TxResult::Off);
  CHECK(p.check(true, 922099999, 0) == TxResult::Range);
  CHECK(p.check(true, 922300001, 0) == TxResult::Range);
  CHECK(p.check(true, 922100000, 0) == TxResult::Ok);
  CHECK(p.check(true, 922300000, 0) == TxResult::Ok);

  p.sent(1000, 280);
  CHECK(p.check(true, 922200000, 1099) == TxResult::Limit);
  CHECK(p.check(true, 922200000, 1100) == TxResult::Ok);

  // 359.5 s used: one more worst-case frame would exceed the hour's budget, until the window restarts
  TxPolicy q;
  q.sent(10000, 359500);
  CHECK(q.check(true, 922200000, 20000) == TxResult::Limit);
  CHECK(q.check(true, 922200000, kBudgetWindowMs) == TxResult::Ok);

  CHECK(TxPolicy::clampDbm(22) == 10 && TxPolicy::clampDbm(10) == 10 && TxPolicy::clampDbm(0) == 0);
  CHECK(TxPolicy::clampDbm(-30) == -9);
  CHECK(TxPolicy::busy(160) && !TxPolicy::busy(161));
}

static void testSx126x() {
  uint8_t f[4];
  sx126x::frequency(923200000, f);  // 923.2 MHz * 2^25 / 32 MHz
  CHECK(bytes(f) == std::vector<uint8_t>({0x39, 0xB3, 0x33, 0x33}));

  uint8_t cal[2];
  sx126x::imageCalibration(923200000, cal);
  CHECK(bytes(cal) == std::vector<uint8_t>({0xE1, 0xE9}));
  sx126x::imageCalibration(868000000, cal);
  CHECK(bytes(cal) == std::vector<uint8_t>({0xD7, 0xDB}));
  sx126x::imageCalibration(780000000, cal);
  CHECK(bytes(cal) == std::vector<uint8_t>({0xC1, 0xC5}));
  sx126x::imageCalibration(490000000, cal);
  CHECK(bytes(cal) == std::vector<uint8_t>({0x75, 0x81}));
  sx126x::imageCalibration(433000000, cal);
  CHECK(bytes(cal) == std::vector<uint8_t>({0x6B, 0x6F}));

  uint8_t mod[8];
  sx126x::rxModulation(0x0A, mod);
  CHECK(bytes(mod) == std::vector<uint8_t>({0x00, 0x50, 0x00, 0x00, 0x0A, 0x00, 0x66, 0x66}));
  sx126x::txModulation(0x0A, mod);
  CHECK(bytes(mod) == std::vector<uint8_t>({0x1A, 0x0A, 0xAA, 0x0B, 0x0A, 0x00, 0x03, 0x46}));

  uint8_t pkt[9];
  sx126x::rxPacket(0x04, pkt);
  CHECK(bytes(pkt) == std::vector<uint8_t>({0x00, 0x08, 0x04, 0x40, 0x00, 0x00, 0xFF, 0x01, 0x00}));
  sx126x::txPacket(pkt);
  CHECK(bytes(pkt) == std::vector<uint8_t>({0x00, 0x10, 0x00, 0x10, 0x00, 0x00, kDownlinkBytes, 0x01, 0x00}));

  uint8_t pa[4];
  sx126x::paConfig(pa);
  CHECK(bytes(pa) == std::vector<uint8_t>({0x04, 0x07, 0x00, 0x01}));
  uint8_t tx[2];
  sx126x::txParams(10, tx);
  CHECK(bytes(tx) == std::vector<uint8_t>({0x0A, 0x02}));
  sx126x::txParams(-9, tx);
  CHECK(bytes(tx) == std::vector<uint8_t>({0xF7, 0x02}));

  uint8_t t[3];
  sx126x::steps(0x00FA00, t);
  CHECK(bytes(t) == std::vector<uint8_t>({0x00, 0xFA, 0x00}));
}

static void testSampleClock() {
  SampleClock c;
  c.start(0, 2000);
  CHECK(c.due(0));
  CHECK(!c.due(100));
  CHECK(c.due(500));
  CHECK(!c.due(999));
  CHECK(c.due(1000));

  // Late by less than 100 ms: catches up one sample per call
  CHECK(c.due(1700) && c.due(1700) == false);
  // Far behind: resynchronises, no burst
  CHECK(c.due(1000000));
  CHECK(!c.due(1000001));
  CHECK(c.due(1000500));

  // Across the wrap of the microsecond counter
  c.start(0xFFFFFF00, 2000);
  CHECK(c.due(0xFFFFFF00));
  CHECK(!c.due(0xFFFFFFFF));
  CHECK(c.due(0xFFFFFF00 + 500));

  // A new rate applies from the sample already scheduled
  c.setRate(100);
  CHECK(c.due(0xFFFFFF00 + 1000));
  CHECK(!c.due(0xFFFFFF00 + 10999));
  CHECK(c.due(0xFFFFFF00 + 11000));
}

int main() {
  testCommands();
  testDownlink();
  testLineReader();
  testFormat();
  testTxPolicy();
  testSx126x();
  testSampleClock();
  if (failures) {
    printf("%d failure(s)\n", failures);
    return 1;
  }
  printf("all streamer tests passed\n");
  return 0;
}
