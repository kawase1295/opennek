#include "SampleClock.h"

namespace streamer {

static constexpr int32_t kResyncUs = 100000;

void SampleClock::start(uint32_t nowUs, uint32_t rateHz) {
  nextUs_ = nowUs;
  setRate(rateHz);
}

bool SampleClock::due(uint32_t nowUs) {
  if ((int32_t)(nowUs - nextUs_) < 0) return false;
  nextUs_ += periodUs_;
  if ((int32_t)(nowUs - nextUs_) > kResyncUs) nextUs_ = nowUs + periodUs_;
  return true;
}

}  // namespace streamer
