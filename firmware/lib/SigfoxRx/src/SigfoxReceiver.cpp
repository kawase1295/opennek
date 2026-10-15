#include "SigfoxReceiver.h"

#include <string.h>

#include <algorithm>

namespace sigfox {

static const double BUFFER_S = 12;
static const double STEP_S = 0.5;
static const double TAIL_S = 0.3;  // a burst counts as finished once it ended this long before the newest sample
// Repetitions start ~3.7-4 s apart and a frame is only decoded ~2 s after it starts, so wait this long
// after the latest repetition before calling a message complete
static const double QUIET_S = 10;
static const double SAME_BURST_S = 0.3;  // the detected start of a burst moves a little between scans

Receiver::Receiver(int rate, MessageFn onMessage, FrameFn onFrame)
    : rate_(rate), onMessage_(onMessage), onFrame_(onFrame), buf_((size_t)jsRound(BUFFER_S * rate)) {}

void Receiver::pushRaw(const uint8_t* raw, size_t n) {
  for (size_t off = 0; off < n;) {
    if (len_ == buf_.size()) {
      // keep the newest half
      const size_t keep = buf_.size() >> 1;
      memmove(buf_.data(), buf_.data() + len_ - keep, keep);
      base_ += len_ - keep;
      len_ = keep;
    }
    const size_t take = std::min(n - off, buf_.size() - len_);
    memcpy(buf_.data() + len_, raw + off, take);
    len_ += take;
    off += take;
    sinceScan_ += take;
    if (sinceScan_ >= STEP_S * rate_) {
      sinceScan_ = 0;
      scan(false);
    }
  }
  expire(false);
}

void Receiver::flush() {
  scan(true);
  expire(true);
}

void Receiver::scan(bool final) {
  const uint8_t* view = buf_.data();
  auto get = [view](size_t i) { return -view[i] / 2.0f; };
  const long tail = final ? 0 : (long)jsRound(TAIL_S * rate_);
  floor_ = noiseFloorRaw(view, len_);
  for (const Burst& b : findBursts(get, len_, rate_, floor_)) {
    // Only bursts that have ended; ones touching the start of the buffer were decoded earlier
    if (b.end > (long)len_ - tail || b.start == 0) continue;
    const uint64_t start = base_ + b.start;
    bool seen = false;
    for (uint64_t s : seen_) seen |= fabs((double)s - (double)start) < SAME_BURST_S * rate_;
    if (seen) continue;
    seen_.push_back(start);
    DecodedFrame frame;
    if (!decodeBurst(get, len_, rate_, b, (double)base_ / rate_, frame)) continue;
    if (onFrame_) onFrame_(frame);
    if (frame.crcOk) merge(frame);
  }
  seen_.erase(std::remove_if(seen_.begin(), seen_.end(), [this](uint64_t s) { return (double)s < (double)base_ - rate_; }),
              seen_.end());
}

void Receiver::merge(const DecodedFrame& f) {
  for (Pending& m : pending_) {
    if (m.deviceId == f.deviceId && m.counter == f.counter) {
      if (std::find(m.ranks.begin(), m.ranks.end(), f.rank) == m.ranks.end()) m.ranks.push_back(f.rank);
      m.rssi = std::max(m.rssi, f.rssi);
      m.last = f.time;
      return;
    }
  }
  Pending m;
  m.deviceId = f.deviceId;
  m.counter = f.counter;
  m.payload = f.payload;
  m.bit = f.bit;
  m.downlink = f.downlink;
  m.auth = f.auth;
  m.time = f.time;
  m.rssi = f.rssi;
  m.ranks = {f.rank};
  m.clockErrorPpm = f.clockErrorPpm;
  m.last = f.time;
  pending_.push_back(m);
}

void Receiver::expire(bool all) {
  for (size_t i = 0; i < pending_.size();) {
    Pending& m = pending_[i];
    if (all || m.ranks.size() == 3 || time() - m.last > QUIET_S) {
      Message message = m;
      pending_.erase(pending_.begin() + i);
      std::sort(message.ranks.begin(), message.ranks.end());
      onMessage_(message);
    } else {
      i++;
    }
  }
}

}  // namespace sigfox
