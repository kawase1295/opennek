#include "SigfoxFrame.h"

namespace sigfox {

static const uint16_t PREAMBLE = 0xAAAA;
// Frame types per repetition rank, indexed by payload class: empty/bit, 1 byte, 2-4, 5-8, 9-12 bytes
static const uint16_t FRAME_TYPES[3][5] = {
    {0xa06b, 0xa08d, 0xa35f, 0xa611, 0xa94c},
    {0xa6e0, 0xa0d2, 0xa598, 0xa6bf, 0xa971},
    {0xa034, 0xa302, 0xa5a3, 0xa72c, 0xa997},
};
static const uint8_t CONV_NEXT[4] = {0, 0, 1, 1};
static const uint8_t CONV_OUT[3][4] = {{0, 0, 0, 0}, {0, 1, 1, 0}, {0, 1, 0, 1}};

uint16_t crc16(const uint8_t* data, size_t len) {
  uint16_t c = 0;
  for (size_t n = 0; n < len; n++) {
    c ^= data[n] << 8;
    for (int i = 0; i < 8; i++) c = c & 0x8000 ? (c << 1) ^ 0x1021 : c << 1;
  }
  return c ^ 0xFFFF;
}

static void appendBits(std::string& s, uint32_t v, int n) {
  for (int i = n - 1; i >= 0; i--) s += (v >> i) & 1 ? '1' : '0';
}

static uint32_t bitsValue(const std::string& s, size_t from, size_t n) {
  uint32_t v = 0;
  for (size_t i = 0; i < n; i++) v = (v << 1) | (s[from + i] == '1');
  return v;
}

std::string convolve(const std::string& bits, int rank) {
  std::string out;
  out.reserve(bits.size());
  int state = 0;
  for (char b : bits) {
    int input = b == '1';
    out += (input ^ CONV_OUT[rank][state]) ? '1' : '0';
    state = CONV_NEXT[state] + (input ? 2 : 0);
  }
  return out;
}

std::string deconvolve(const std::string& bits, int rank) {
  std::string out;
  out.reserve(bits.size());
  int state = 0;
  for (char b : bits) {
    int input = (b == '1') ^ CONV_OUT[rank][state];
    out += input ? '1' : '0';
    state = CONV_NEXT[state] + (input ? 2 : 0);
  }
  return out;
}

// Payload and auth sizes for a payload class and the LI read from the frame
static void sizes(int cls, int li, int& payload, int& auth) {
  if (cls == 0 || cls == 1) {
    payload = cls;
    auth = 2;
  } else {
    payload = 4 * (cls - 1) - li;
    auth = li + 2;
  }
}

std::string encodeUplink(int rank, uint32_t deviceId, uint16_t counter, const std::vector<uint8_t>& payload,
                         const std::vector<uint8_t>& auth, bool downlink, int bit) {
  int n = payload.size();
  int cls = bit >= 0 || n == 0 ? 0 : n == 1 ? 1 : (n - 1) / 4 + 2;
  int li = bit >= 0 ? 2 + bit : cls >= 2 ? 3 - ((n - 1) % 4) : 0;
  std::vector<uint8_t> body;
  body.push_back((li << 6) | ((downlink ? 1 : 0) << 5) | ((counter >> 8) & 0x0F));
  body.push_back(counter & 0xFF);
  for (int i = 0; i < 4; i++) body.push_back(deviceId >> (8 * i));
  body.insert(body.end(), payload.begin(), payload.end());
  body.insert(body.end(), auth.begin(), auth.end());
  std::string bits;
  for (uint8_t b : body) appendBits(bits, b, 8);
  appendBits(bits, crc16(body.data(), body.size()), 16);
  if (rank > 0) bits = convolve(bits, rank);
  std::string out;
  appendBits(out, PREAMBLE, 16);
  appendBits(out, FRAME_TYPES[rank][cls], 16);
  return out + bits;
}

static bool parseBody(const std::string& bodyBits, int rank, int cls, const std::string& repaired, UplinkFrame& f) {
  const std::string plain = rank > 0 ? deconvolve(bodyBits, rank) : bodyBits;
  if (plain.size() < 16) return false;
  int li = bitsValue(plain, 0, 2);
  int payload, auth;
  sizes(cls, li, payload, auth);
  if (payload < 0) return false;
  size_t bodyBytes = 2 + 4 + payload + auth;
  if (plain.size() < (bodyBytes + 2) * 8) return false;
  uint8_t bytes[32];
  for (size_t i = 0; i < bodyBytes + 2; i++) bytes[i] = bitsValue(plain, i * 8, 8);
  f.rank = rank;
  f.crcOk = crc16(bytes, bodyBytes) == ((bytes[bodyBytes] << 8) | bytes[bodyBytes + 1]);
  f.deviceId = bytes[2] | (bytes[3] << 8) | (bytes[4] << 16) | ((uint32_t)bytes[5] << 24);
  f.counter = ((bytes[0] & 0x0F) << 8) | bytes[1];
  f.payload.assign(bytes + 6, bytes + 6 + payload);
  f.bit = cls == 0 && li >= 2 ? li - 2 : -1;
  f.downlink = (bytes[0] & 0x20) != 0;
  f.auth.assign(bytes + 6 + payload, bytes + 6 + payload + auth);
  f.repaired = repaired;
  return true;
}

// The frame type is searched near where it should be, because the first preamble bits are not observable
// and the leading edge can add a spurious bit. When the CRC fails, single slipped, extra or flipped bits
// are tried (the receiver clock drifts).
bool decodeUplinkBits(const std::string& bits, UplinkFrame& out) {
  for (size_t start = 8; start <= 22 && start + 16 <= bits.size(); start++) {
    uint32_t ft = bitsValue(bits, start, 16);
    for (int rank = 0; rank < 3; rank++) {
      int cls = -1;
      for (int c = 0; c < 5; c++)
        if (FRAME_TYPES[rank][c] == ft) cls = c;
      if (cls < 0) continue;
      const std::string body = bits.substr(start + 16);
      bool found = parseBody(body, rank, cls, "", out);
      if (found && out.crcOk) return true;
      UplinkFrame fixed;
      std::string candidate;
      for (size_t i = 0; i < body.size(); i++) {
        const std::string head = body.substr(0, i);
        const char flipped = body[i] == '1' ? '0' : '1';
        const std::string reasons[4] = {"inserted 1 at ", "inserted 0 at ", "deleted bit ", "flipped bit "};
        for (int k = 0; k < 4; k++) {
          if (k == 0) candidate = head + '1' + body.substr(i);
          if (k == 1) candidate = head + '0' + body.substr(i);
          if (k == 2) candidate = head + body.substr(i + 1);
          if (k == 3) candidate = head + flipped + body.substr(i + 1);
          if (parseBody(candidate, rank, cls, reasons[k] + std::to_string(i), fixed) && fixed.crcOk) {
            out = fixed;
            return true;
          }
        }
      }
      return found;
    }
  }
  return false;
}

}  // namespace sigfox
