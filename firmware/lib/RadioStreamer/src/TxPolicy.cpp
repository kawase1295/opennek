#include "TxPolicy.h"

namespace streamer {

TxResult TxPolicy::check(bool enabled, uint32_t hz, uint32_t nowMs) {
  if (!enabled) return TxResult::Off;
  if (hz < kTxMinHz || hz > kTxMaxHz) return TxResult::Range;
  if (nowMs - budgetStart_ >= kBudgetWindowMs) {
    budgetStart_ = nowMs;
    budgetUsedMs_ = 0;
  }
  if (txDone_ && nowMs - lastTxEnd_ < kTxPauseMs) return TxResult::Limit;
  if (budgetUsedMs_ + kTxWorstCaseMs > kTxBudgetMs) return TxResult::Limit;
  return TxResult::Ok;
}

void TxPolicy::sent(uint32_t endMs, uint32_t onAirMs) {
  txDone_ = true;
  lastTxEnd_ = endMs;
  budgetUsedMs_ += onAirMs;
}

int32_t TxPolicy::clampDbm(int32_t dbm) {
  if (dbm > kTxMaxDbm) return kTxMaxDbm;
  if (dbm < kTxMinDbm) return kTxMinDbm;
  return dbm;
}

}  // namespace streamer
