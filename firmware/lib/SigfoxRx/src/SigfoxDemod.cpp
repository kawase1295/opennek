#include "SigfoxDemod.h"

#include <algorithm>
#include <set>

namespace sigfox {

// M_PI is not standard C++ and is missing from some libcs (zig's Windows target)
constexpr double kPi = 3.14159265358979323846;

float noiseFloor(const float* dbm, size_t n) {
  if (!n) return -120;
  std::vector<float> sorted(dbm, dbm + n);
  auto nth = sorted.begin() + (size_t)(n * 0.2);
  std::nth_element(sorted.begin(), nth, sorted.end());
  return *nth;
}

float noiseFloorRaw(const uint8_t* raw, size_t n) {
  if (!n) return -120;
  // Ascending dBm is descending raw value
  uint32_t histogram[256] = {};
  for (size_t i = 0; i < n; i++) histogram[raw[i]]++;
  size_t rank = n * 0.2;
  for (int v = 255; v >= 0; v--) {
    if (rank < histogram[v]) return -v / 2.0f;
    rank -= histogram[v];
  }
  return -120;
}

bool burstBits(const float* seg, size_t n, int rate, Bits& out) {
  const double nominal = (double)rate / BIT_RATE;
  if (n < 3) return false;
  std::vector<float> sorted(seg, seg + n);
  auto nth = sorted.begin() + (size_t)(n * 0.9);
  std::nth_element(sorted.begin(), nth, sorted.end());
  const double level = *nth;
  // Dips are measured against the local envelope on each side (max over 1.5 bits). A real dip has a high
  // level on both sides; this rejects the rising and falling edges of the frame, and copes with the
  // transmit power ramping during the first part of a frame.
  const long win = (long)jsRound(nominal * 1.5);
  auto sideMax = [&](long from, long to) {
    float m = -200;
    for (long k = std::max(0L, from); k <= std::min((long)n - 1, to); k++) m = std::max(m, seg[k]);
    return (double)m;
  };
  // Dip centres: local minima at least 6 dB below both sides, half a bit apart, refined to sub-sample
  std::vector<double> dips;
  const long half = (long)jsRound(nominal * 0.4);
  for (long i = 1; i < (long)n - 1; i++) {
    const double v = seg[i];
    const double left = sideMax(i - win, i - 1);
    const double right = sideMax(i + 1, i + win);
    if (v >= std::min(left, right) - 6 || std::max(left, right) < level - 25) continue;
    bool isMin = true;
    for (long k = std::max(0L, i - half); k <= std::min((long)n - 1, i + half); k++) {
      if (seg[k] < v || (seg[k] == v && k < i)) {
        isMin = false;
        break;
      }
    }
    if (!isMin) continue;
    const double a = seg[i - 1];
    const double c = seg[i + 1];
    const double denom = a - 2 * v + c;
    dips.push_back(i + (denom != 0 ? (0.5 * (a - c)) / denom : 0));
  }
  if (dips.size() < 8) return false;

  // Bit clock: assign each dip to a bit index with the current estimate, then refit period and phase by
  // least squares. Tracking dip by dip keeps a 1 % clock error from slipping a whole bit over a frame.
  double period = nominal;
  // Anchor the grid on the mean phase of the first dips (circular mean modulo one bit)
  double c = 0, s = 0;
  for (size_t j = 0; j < dips.size() && j < 12; j++) {
    const double a = (2 * kPi * (dips[j] - dips[0])) / nominal;
    c += cos(a);
    s += sin(a);
  }
  double phase = dips[0] + (atan2(s, c) / (2 * kPi)) * nominal;
  std::vector<std::pair<long, double>> points;
  for (double d : dips) {
    const long k = (long)jsRound((d - phase) / period);
    const double predicted = phase + k * period;
    if (fabs(d - predicted) > 0.35 * period || (!points.empty() && k <= points.back().first)) continue;
    points.push_back({k, d});
    if (points.size() >= 4) {
      double sk = 0, sd = 0, skk = 0, skd = 0;
      for (auto& p : points) {
        sk += p.first;
        sd += p.second;
        skk += (double)p.first * p.first;
        skd += p.first * p.second;
      }
      const double np = points.size();
      const double det = np * skk - sk * sk;
      if (det > 0) {
        period = (np * skd - sk * sd) / det;
        phase = (sd - period * sk) / np;
      }
    }
  }
  if (points.size() < 8) return false;
  const long first = points.front().first;
  const long last = points.back().first;
  std::set<long> onGrid;
  for (auto& p : points) onGrid.insert(p.first);
  out.bits = "1";  // the first preamble bit has no phase reference
  // Bits after the last dip are 1s (no reversal); count them up to where the signal ends
  long lastHigh = n - 1;
  while (lastHigh > 0 && seg[lastHigh] < level - 15) lastHigh--;
  const long end = std::max(last, (long)floor((lastHigh - phase) / period));
  for (long k = first; k <= end; k++) out.bits += onGrid.count(k) ? '0' : '1';
  out.periodSamples = period;
  out.clockErrorPpm = (int)jsRound((period / nominal - 1) * 1e6);
  out.levelDbm = level;
  out.dips = points.size();
  return true;
}

std::vector<DecodedFrame> decodeCapture(const float* dbm, size_t n, int rate, double t0) {
  std::vector<DecodedFrame> out;
  auto get = [dbm](size_t i) { return dbm[i]; };
  for (const Burst& b : findBursts(get, n, rate, noiseFloor(dbm, n))) {
    DecodedFrame f;
    if (decodeBurst(get, n, rate, b, t0, f)) out.push_back(f);
  }
  return out;
}

}  // namespace sigfox
