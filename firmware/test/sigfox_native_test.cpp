// Checks the C++ Sigfox decoder (lib/SigfoxRx) against the same vectors and real captures as
// host/test/sigfox.test.ts. Run on the PC:
//   cd firmware/test && uvx --from ziglang python -m ziglang c++ -std=c++17 -O2 -I ../lib/SigfoxRx/src \
//     sigfox_native_test.cpp ../lib/SigfoxRx/src/*.cpp -o sigfox_test && ./sigfox_test ../../fixtures
// With --dump <file.rssi>, prints every frame and message instead (to compare with the TypeScript decoder).
#include <stdio.h>
#include <string.h>

#include <string>
#include <vector>

#include "SigfoxReceiver.h"

using namespace sigfox;

static int failures = 0;
#define CHECK(cond)                                                \
  do {                                                             \
    if (!(cond)) {                                                 \
      printf("FAIL %s:%d: %s\n", __FILE__, __LINE__, #cond);      \
      failures++;                                                  \
    }                                                              \
  } while (0)

static std::string hex(const std::vector<uint8_t>& b) {
  std::string s;
  char t[3];
  for (uint8_t v : b) {
    snprintf(t, sizeof(t), "%02x", v);
    s += t;
  }
  return s;
}

static std::vector<uint8_t> readFile(const std::string& path) {
  std::vector<uint8_t> out;
  FILE* f = fopen(path.c_str(), "rb");
  if (!f) {
    printf("cannot open %s\n", path.c_str());
    failures++;
    return out;
  }
  uint8_t buf[4096];
  size_t n;
  while ((n = fread(buf, 1, sizeof(buf), f)) > 0) out.insert(out.end(), buf, buf + n);
  fclose(f);
  return out;
}

static std::vector<Message> receive(const std::vector<uint8_t>& raw, std::vector<DecodedFrame>* frames = nullptr) {
  std::vector<Message> messages;
  Receiver rx(2000, [&](const Message& m) { messages.push_back(m); },
              [&](const DecodedFrame& f) {
                if (frames) frames->push_back(f);
              });
  for (size_t i = 0; i < raw.size(); i += 64) rx.pushRaw(raw.data() + i, std::min<size_t>(64, raw.size() - i));
  rx.flush();
  return messages;
}

static void testFrames() {
  const std::string bits = "1011001110001011010011110000101";
  for (int rank = 0; rank < 3; rank++) CHECK(deconvolve(convolve(bits, rank), rank) == bits);

  for (int n = 0; n <= 12; n++) {
    for (int rank = 0; rank < 3; rank++) {
      std::vector<uint8_t> payload(n), auth;
      for (int i = 0; i < n; i++) payload[i] = (i * 37 + n) & 0xFF;
      int li = n >= 2 ? 3 - ((n - 1) % 4) : 0;
      for (int i = 0; i < li + 2; i++) auth.push_back(0xA0 + i);
      std::string b = encodeUplink(rank, 0x00763DA3, 1979 + n, payload, auth, n % 2 == 1);
      // the receiver never sees the first preamble bit
      std::string seen = "1" + b.substr(1);
      UplinkFrame f;
      CHECK(decodeUplinkBits(seen, f));
      CHECK(f.rank == rank && f.deviceId == 0x00763DA3 && f.counter == 1979 + n && f.downlink == (n % 2 == 1));
      CHECK(f.crcOk && f.repaired.empty());
      CHECK(f.payload == payload && f.auth == auth);
    }
  }

  UplinkFrame bit;
  CHECK(decodeUplinkBits(encodeUplink(0, 1, 5, {}, {0, 0}, false, 1), bit));
  CHECK(bit.bit == 1 && bit.crcOk);

  std::string b = encodeUplink(0, 0x00763DA3, 1979, {0x11}, {0xAB, 0x6D});
  UplinkFrame slipped;
  CHECK(decodeUplinkBits(b.substr(0, 70) + b.substr(71), slipped));
  CHECK(slipped.crcOk && slipped.repaired.rfind("inserted", 0) == 0 && hex(slipped.payload) == "11");

  const uint8_t real[] = {0x07, 0xBB, 0xA3, 0x3D, 0x76, 0x00, 0x11, 0xAB, 0x6D};
  CHECK(crc16(real, sizeof(real)) == 0x873B);
}

static void testCaptures(const std::string& dir) {
  std::vector<uint8_t> raw = readFile(dir + "/rc3-00763DA3.rssi");
  std::vector<float> dbm(raw.size());
  for (size_t i = 0; i < raw.size(); i++) dbm[i] = -raw[i] / 2.0f;
  std::vector<DecodedFrame> frames = decodeCapture(dbm.data(), dbm.size(), 2000);
  CHECK(frames.size() == 9);
  for (size_t i = 0; i < frames.size(); i++) {
    CHECK(frames[i].counter == 1979 + i / 3 && frames[i].rank == (int)(i % 3) && frames[i].crcOk);
    CHECK(frames[i].deviceId == 0x00763DA3 && hex(frames[i].payload) == "11");
  }
  std::vector<Message> messages = receive(raw);
  CHECK(messages.size() == 3);
  for (size_t i = 0; i < messages.size(); i++) {
    CHECK(messages[i].deviceId == 0x00763DA3 && messages[i].counter == 1979 + i && hex(messages[i].payload) == "11");
    CHECK((messages[i].ranks == std::vector<int>{0, 1, 2}));
  }

  messages = receive(readFile(dir + "/rc3-00763DA3-live.rssi"));
  CHECK(messages.size() == 3);
  const char* payloads[] = {"1234567890", "11", "11"};
  for (size_t i = 0; i < messages.size() && i < 3; i++) {
    CHECK(messages[i].counter == 1982 + i && hex(messages[i].payload) == payloads[i]);
    CHECK(messages[i].downlink == (i == 2) && (messages[i].ranks == std::vector<int>{0, 1, 2}));
  }
}

static void dump(const std::string& path) {
  std::vector<DecodedFrame> frames;
  std::vector<Message> messages = receive(readFile(path), &frames);
  for (auto& f : frames)
    printf("F t=%.3f rank=%d id=%08X seq=%u crc=%d repaired=%s rssi=%.1f clock=%d data=%s auth=%s\n", f.time, f.rank, f.deviceId,
           f.counter, f.crcOk, f.repaired.empty() ? "-" : f.repaired.c_str(), f.rssi, f.clockErrorPpm, hex(f.payload).c_str(),
           hex(f.auth).c_str());
  for (auto& m : messages) {
    std::string ranks;
    for (int r : m.ranks) ranks += std::to_string(r);
    printf("M t=%.3f id=%08X seq=%u data=%s bit=%d downlink=%d ranks=%s rssi=%.1f\n", m.time, m.deviceId, m.counter,
           hex(m.payload).c_str(), m.bit, m.downlink, ranks.c_str(), m.rssi);
  }
}

int main(int argc, char** argv) {
  if (argc == 3 && !strcmp(argv[1], "--dump")) {
    dump(argv[2]);
    return failures ? 1 : 0;
  }
  testFrames();
  testCaptures(argc > 1 ? argv[1] : "../../fixtures");
  printf(failures ? "%d failure(s)\n" : "all sigfox tests passed\n", failures);
  return failures ? 1 : 0;
}
