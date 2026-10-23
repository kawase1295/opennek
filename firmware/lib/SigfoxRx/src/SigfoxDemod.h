// Envelope (RSSI) demodulator for Sigfox DBPSK uplinks. Port of tools/lsm/sigfox/demod.ts.
// Pulse-shaped DBPSK makes the amplitude dip towards zero at every phase reversal, so the dips alone give
// the differentially decoded bits: a dip on a bit boundary means data bit 0, no dip means 1.
#pragma once
#include <math.h>

#include "SigfoxFrame.h"

namespace sigfox {

const int BIT_RATE = 100;

// JavaScript's Math.round (halves round up), so both implementations make the same choices
inline double jsRound(double x) { return floor(x + 0.5); }

struct Burst {
  int start, end;  // sample indices, end exclusive
  float peakDbm, floorDbm;
};

struct BurstOpts {
  double aboveDb = 15, minMs = 300, gapMs = 40;
};

// 20th percentile of the samples
float noiseFloor(const float* dbm, size_t n);
// Same for raw streamer values (dBm = -raw / 2), without copying them
float noiseFloorRaw(const uint8_t* raw, size_t n);

// Frames last over a second and sit well above the noise; the dips inside a frame are short gaps.
// `get(i)` returns sample i in dBm.
template <class Get>
std::vector<Burst> findBursts(Get get, size_t n, int rate, float floor, BurstOpts opts = BurstOpts()) {
  const double threshold = floor + opts.aboveDb;
  const long gap = (long)jsRound(opts.gapMs / 1000 * rate);
  std::vector<Burst> bursts;
  long start = -1, lastAbove = -1;
  for (size_t i = 0; i <= n; i++) {
    bool above = i < n && get(i) > threshold;
    if (above) {
      if (start < 0) start = i;
      lastAbove = i;
    } else if (start >= 0 && (i == n || (long)i - lastAbove > gap)) {
      if ((double)(lastAbove - start) / rate >= opts.minMs / 1000) {
        float peak = -200;
        for (long k = start; k <= lastAbove; k++) peak = fmaxf(peak, get(k));
        bursts.push_back({(int)start, (int)lastAbove + 1, peak, floor});
      }
      start = -1;
    }
  }
  return bursts;
}

struct Bits {
  std::string bits;
  double periodSamples;
  int clockErrorPpm;
  float levelDbm;
  int dips;
};

// Finds the dips of one burst, recovers the bit clock from their positions and returns the data bits
bool burstBits(const float* seg, size_t n, int rate, Bits& out);

struct DecodedFrame : UplinkFrame {
  double time = 0;  // seconds
  float rssi = 0;   // peak of the burst (dBm)
  int clockErrorPpm = 0;
};

// Decodes the burst `b` of a signal of `n` samples; `t0` is the time of sample 0 in seconds
template <class Get>
bool decodeBurst(Get get, size_t n, int rate, const Burst& b, double t0, DecodedFrame& out) {
  const long pad = (long)jsRound(rate * 0.02);
  const long from = b.start - pad > 0 ? b.start - pad : 0;
  const long to = (long)n < b.end + pad ? (long)n : b.end + pad;
  std::vector<float> seg(to - from);
  for (long i = from; i < to; i++) seg[i - from] = get(i);
  Bits bits;
  if (!burstBits(seg.data(), seg.size(), rate, bits)) return false;
  if (!decodeUplinkBits(bits.bits, out)) return false;
  out.time = t0 + (double)b.start / rate;
  out.rssi = b.peakDbm;
  out.clockErrorPpm = bits.clockErrorPpm;
  return true;
}

// Decodes every frame found in a block of samples (dBm)
std::vector<DecodedFrame> decodeCapture(const float* dbm, size_t n, int rate, double t0 = 0);

}  // namespace sigfox
