// Downlink transmission limits, enforced by the firmware whatever the host asks for.
#pragma once

#include <stdint.h>

#include "Protocol.h"

namespace streamer {

constexpr uint32_t kTxMinHz = 922100000;  // the 922.2 MHz unit channel (ARIB STD-T108): uplink frame 1 - 1 MHz
constexpr uint32_t kTxMaxHz = 922300000;
constexpr int32_t kTxMaxDbm = 10;  // 10 mW
constexpr int32_t kTxMinDbm = -9;  // the SX1262 high-power PA's minimum
// Carrier sense as sigfox-ep-lib's RC3 LBT (SIGFOX_SPECTRUM_ACCESS_LBT_M80): -80 dBm for 5 ms
constexpr uint8_t kBusyRaw = 160;  // raw RSSI = -2 * dBm, so a lower value is a stronger signal
constexpr uint32_t kCarrierSenseUs = 5000;
constexpr uint32_t kTxPauseMs = 100;        // between two transmissions (ARIB STD-T108: at least 50 ms)
constexpr uint32_t kTxBudgetMs = 360000;    // on-air time per hour
constexpr uint32_t kTxWorstCaseMs = 1000;   // the TX timeout; a frame takes 280 ms
constexpr uint32_t kBudgetWindowMs = 3600000;

class TxPolicy {
 public:
  // Whether a frame on hz may go out at nowMs, before carrier sense: Ok, Off, Range or Limit
  TxResult check(bool enabled, uint32_t hz, uint32_t nowMs);
  // A transmission ended at endMs after onAirMs on air
  void sent(uint32_t endMs, uint32_t onAirMs);

  static int32_t clampDbm(int32_t dbm);
  // The strongest raw RSSI seen during carrier sense means the channel is in use
  static bool busy(uint8_t strongestRaw) { return strongestRaw <= kBusyRaw; }

 private:
  bool txDone_ = false;  // at least one transmission since reset
  uint32_t lastTxEnd_ = 0;
  uint32_t budgetStart_ = 0;
  uint32_t budgetUsedMs_ = 0;
};

}  // namespace streamer
