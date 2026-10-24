#include "Protocol.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

namespace streamer {

static const char kDigits[] = "0123456789ABCDEF";

static int hexDigit(char c) {
  if (c >= '0' && c <= '9') return c - '0';
  if (c >= 'A' && c <= 'F') return c - 'A' + 10;
  if (c >= 'a' && c <= 'f') return c - 'a' + 10;
  return -1;
}

Action applyCommand(const char* c, Settings& s) {
  if (c[0] == 'T') return Action::Transmit;
  long v = atol(c + 1);
  switch (c[0]) {
    case 'F':
      if (v >= 150000000 && v <= 960000000) s.freqHz = v;
      break;
    case 'W':
      if (v >= 0 && v <= 0x1F) s.bwCode = v;
      break;
    case 'S':
      if (v >= 100 && v <= 4000) s.sampleHz = v;
      break;
    case 'B':
      s.boost = v != 0;
      break;
    case 'C':
      s.cfo = v != 0;
      break;
    case 'P':
      if (v >= 0 && v <= 7) s.preambleDetector = v;
      break;
    case 'E':
      s.txEnabled = v != 0;
      break;
    case 'X':
      return Action::Reset;
    case '?':
      return Action::Status;
    default:
      return Action::None;
  }
  return Action::Reconfigure;
}

bool parseDownlink(const char* args, Downlink& out) {
  char* p;
  out.hz = strtoul(args, &p, 10);
  if (*p++ != ',') return false;
  out.dbm = strtol(p, &p, 10);
  if (*p++ != ',') return false;
  for (int i = 0; i < kDownlinkBytes; i++) {
    int hi = hexDigit(p[2 * i]), lo = hi < 0 ? -1 : hexDigit(p[2 * i + 1]);
    if (lo < 0) return false;
    out.frame[i] = (hi << 4) | lo;
  }
  return p[2 * kDownlinkBytes] == 0;
}

const char* LineReader::push(char c) {
  if (c == '\r' || c == '\n') {
    if (!len_) return nullptr;
    buf_[len_] = 0;
    len_ = 0;
    return buf_;
  }
  if (len_ < sizeof(buf_) - 1) buf_[len_++] = c;
  return nullptr;
}

size_t formatStatus(char* out, size_t size, const Settings& s, const char* fw, const char* hw) {
  int n = snprintf(out, size, "I fw=%s f=%lu bw=%u sr=%lu boost=%u cfo=%u pd=%u tx=%u hw=%s\n", fw,
                   (unsigned long)s.freqHz, s.bwCode, (unsigned long)s.sampleHz, s.boost ? 1 : 0, s.cfo ? 1 : 0,
                   s.preambleDetector, s.txEnabled ? 1 : 0, hw);
  return n < 0 ? 0 : (size_t)n < size ? n : size - 1;
}

static char* putSeq(char* p, char kind, uint16_t seq) {
  *p++ = kind;
  for (int s = 12; s >= 0; s -= 4) *p++ = kDigits[(seq >> s) & 0xF];
  *p++ = ':';
  return p;
}

size_t formatSamples(char* out, uint16_t seq, const uint8_t* rssi, uint8_t n) {
  char* p = putSeq(out, 'R', seq);
  for (uint8_t i = 0; i < n; i++) {
    *p++ = kDigits[rssi[i] >> 4];
    *p++ = kDigits[rssi[i] & 0xF];
  }
  *p++ = '\n';
  return p - out;
}

size_t formatCfo(char* out, uint16_t seq, const uint16_t* cfo, uint8_t n) {
  char* p = putSeq(out, 'C', seq);
  for (uint8_t i = 0; i < n; i++)
    for (int s = 8; s >= 0; s -= 4) *p++ = kDigits[(cfo[i] >> s) & 0xF];
  *p++ = '\n';
  return p - out;
}

size_t formatTxReply(char* out, size_t size, TxResult r, uint32_t value) {
  int n = 0;
  switch (r) {
    case TxResult::Ok:
      n = snprintf(out, size, "D ok ms=%lu\n", (unsigned long)value);
      break;
    case TxResult::Busy:
      n = snprintf(out, size, "D busy rssi=-%lu.%lu\n", (unsigned long)(value / 2), (unsigned long)(value & 1) * 5);
      break;
    case TxResult::Off:
      n = snprintf(out, size, "D off\n");
      break;
    case TxResult::Range:
      n = snprintf(out, size, "D range\n");
      break;
    case TxResult::Limit:
      n = snprintf(out, size, "D limit\n");
      break;
    case TxResult::Err:
      n = snprintf(out, size, "D err\n");
      break;
  }
  return n < 0 ? 0 : (size_t)n < size ? n : size - 1;
}

}  // namespace streamer
