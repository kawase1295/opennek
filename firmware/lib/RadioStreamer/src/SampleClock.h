// When to take the next RSSI sample, from a free-running microsecond counter that wraps.
#pragma once

#include <stdint.h>

namespace streamer {

class SampleClock {
 public:
  void start(uint32_t nowUs, uint32_t rateHz);
  void setRate(uint32_t rateHz) { periodUs_ = 1000000UL / rateHz; }
  // Whether a sample is due at nowUs. Fell far behind (e.g. while reconfiguring): resynchronises instead of
  // bursting.
  bool due(uint32_t nowUs);

 private:
  uint32_t nextUs_ = 0;
  uint32_t periodUs_ = 500;
};

}  // namespace streamer
