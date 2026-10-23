// Streaming Sigfox receiver: takes raw streamer samples as they arrive, decodes complete frames and merges
// the up to three repetitions of a message into one. Port of tools/lsm/sigfox/receiver.ts.
// Keeps the raw values (1 byte per sample, dBm = -raw / 2): 12 s at 2 kHz is 24 KB.
#pragma once
#include <functional>

#include "SigfoxDemod.h"

namespace sigfox {

struct Message {
  uint32_t deviceId = 0;
  uint16_t counter = 0;
  std::vector<uint8_t> payload;
  int bit = -1;
  bool downlink = false;
  std::vector<uint8_t> auth;
  double time = 0;   // stream time of the first frame received, in seconds
  float rssi = 0;    // best RSSI of the received frames (dBm)
  std::vector<int> ranks;  // which repetitions (0..2) were received with a valid CRC
  int clockErrorPpm = 0;
};

class Receiver {
 public:
  using MessageFn = std::function<void(const Message&)>;
  using FrameFn = std::function<void(const DecodedFrame&)>;

  Receiver(int rate, MessageFn onMessage, FrameFn onFrame = nullptr);

  int rate() const { return rate_; }
  double time() const { return (double)(base_ + len_) / rate_; }
  float floorDbm() const { return floor_; }
  // The newest samples (oldest first), e.g. to save a capture next to a bad message
  const uint8_t* samples() const { return buf_.data(); }
  size_t sampleCount() const { return len_; }

  void pushRaw(const uint8_t* raw, size_t n);
  // Emits everything still pending (end of a capture)
  void flush();

 private:
  struct Pending : Message {
    double last;
  };
  void scan(bool final);
  void merge(const DecodedFrame& f);
  void expire(bool all);

  int rate_;
  MessageFn onMessage_;
  FrameFn onFrame_;
  std::vector<uint8_t> buf_;
  size_t len_ = 0;   // samples in buf
  uint64_t base_ = 0;  // absolute index of buf[0]
  size_t sinceScan_ = 0;
  float floor_ = -120;
  std::vector<uint64_t> seen_;  // absolute start samples of bursts already decoded
  std::vector<Pending> pending_;  // in arrival order
};

}  // namespace sigfox
