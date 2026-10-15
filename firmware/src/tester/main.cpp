// Standalone Sigfox tester for the M5Stack Cardputer (ESP32-S3).
//
// An LSM100A running the OpenNEK RSSI streamer (firmware-lsm100a) sits on the Grove port (G1 = RX,
// G2 = TX, 115200 bps). The Cardputer decodes Sigfox RC3 uplinks itself and checks every message from the
// registered devices: all three repetitions received, sequence number continuity, expected payload and
// signal margin. Results are shown on screen, written to the SD card (/opennek) and printed on USB serial
// as JSON lines.
//
// Receive only: nothing here transmits. The LSM100A firmware never enters TX, and this firmware never
// starts Wi-Fi or Bluetooth. Messages from devices that are not registered are neither shown nor logged,
// except for the device ID offered in learn mode.
//
// Keys: L learn  I add ID  P expected payload  D devices  H history  M mute  R reset  ` (esc) back
#include <M5Cardputer.h>
#include <Preferences.h>
#include <SD.h>
#include <SPI.h>

#include <deque>
#include <memory>

#include "SigfoxReceiver.h"

#define FW_VERSION "0.1.0"
#define LSM_RX 1  // Grove G1 <- LSM100A UART TX
#define LSM_TX 2  // Grove G2 -> LSM100A UART RX
#define SD_SCK 40
#define SD_MISO 39
#define SD_MOSI 14
#define SD_CS 12

static const int MAX_DEVICES = 8;
static const int HISTORY = 30;
static const float WEAK_MARGIN_DB = 25;  // decoding needs ~15 dB above the floor; warn before that
static const uint32_t RAW_KEEP_S = 30;  // samples kept for captures next to a bad message

enum Verdict { V_PASS, V_WARN, V_FAIL };
static const char* const VERDICT_NAMES[] = {"PASS", "WARN", "FAIL"};
static const uint16_t VERDICT_COLORS[] = {TFT_GREEN, TFT_YELLOW, TFT_RED};

struct Result {
  uint32_t uptimeS;
  sigfox::Message m;
  Verdict verdict;
  int seqDelta;  // vs the previous message of the device, 0 = first one seen
  int repaired;  // repetitions that needed a bit-slip repair
  float floorDbm;
  String issues;
};

enum Screen { MAIN, HISTORY_SCREEN, DEVICES, INPUT_ID, INPUT_PAYLOAD };

static HardwareSerial lsm(1);
static M5Canvas canvas(&M5Cardputer.Display);
static Preferences prefs;
static std::unique_ptr<sigfox::Receiver> rx;

static std::vector<uint32_t> devices;
static String expectedPayload;  // hex, empty = any
static bool muted = false;
static bool sdOk = false;

static Screen screen = MAIN;
static String input;
static bool learning = false;
static uint32_t candidate = 0;  // learn mode: unregistered device offered for registration

static std::deque<Result> history;
static int counts[3] = {};
static uint32_t badFrames = 0;
static uint16_t lastSeq[MAX_DEVICES];
static bool haveSeq[MAX_DEVICES] = {};
struct Progress {
  uint32_t id;
  uint16_t counter;
  uint8_t ranks;  // bit per repetition received
  int repaired;
};
static std::vector<Progress> progress;

// Stream health
static int sampleRate = 2000;
static String streamerStatus;
static uint32_t lastSampleMs = 0;
static uint32_t samplesThisSecond = 0, samplesPerSecond = 0, rateWindowMs = 0;
static int32_t lastLineSeq = -1;
static uint32_t droppedLines = 0;
static uint32_t lastQueryMs = 0;

// Raw samples of the last RAW_KEEP_S seconds, saved to SD next to a V_WARN/V_FAIL result
static uint8_t* rawRing = nullptr;
static size_t rawSize = 0, rawPos = 0;
static bool rawFull = false;

static bool dirty = true;
static uint32_t lastDrawMs = 0;

static String idHex(uint32_t id) {
  char s[9];
  snprintf(s, sizeof(s), "%08lX", (unsigned long)id);
  return s;
}

static String bytesHex(const std::vector<uint8_t>& b) {
  String s;
  char t[3];
  for (uint8_t v : b) {
    snprintf(t, sizeof(t), "%02x", v);
    s += t;
  }
  return s;
}

static String uptime(uint32_t s) {
  char t[16];
  if (s >= 3600)
    snprintf(t, sizeof(t), "%lu:%02lu:%02lu", (unsigned long)(s / 3600), (unsigned long)(s / 60 % 60), (unsigned long)(s % 60));
  else
    snprintf(t, sizeof(t), "%02lu:%02lu", (unsigned long)(s / 60), (unsigned long)(s % 60));
  return t;
}

static int deviceIndex(uint32_t id) {
  for (size_t i = 0; i < devices.size(); i++)
    if (devices[i] == id) return i;
  return -1;
}

static void saveSettings() {
  prefs.putBytes("devices", devices.data(), devices.size() * sizeof(uint32_t));
  prefs.putString("payload", expectedPayload);
  prefs.putBool("muted", muted);
}

static void loadSettings() {
  prefs.begin("opennek-tester");
  uint32_t ids[MAX_DEVICES];
  size_t n = prefs.getBytes("devices", ids, sizeof(ids)) / sizeof(uint32_t);
  devices.assign(ids, ids + n);
  expectedPayload = prefs.getString("payload", "");
  muted = prefs.getBool("muted", false);
}

static void addDevice(uint32_t id) {
  if (deviceIndex(id) >= 0 || (int)devices.size() >= MAX_DEVICES) return;
  devices.push_back(id);
  haveSeq[devices.size() - 1] = false;
  saveSettings();
}

static void removeDevice(size_t i) {
  if (i >= devices.size()) return;
  devices.erase(devices.begin() + i);
  for (size_t k = i; k + 1 < MAX_DEVICES; k++) {
    lastSeq[k] = lastSeq[k + 1];
    haveSeq[k] = haveSeq[k + 1];
  }
  haveSeq[MAX_DEVICES - 1] = false;
  saveSettings();
}

static void beep(Verdict v) {
  if (muted) return;
  if (v == V_PASS) {
    M5Cardputer.Speaker.tone(2400, 80);
  } else if (v == V_WARN) {
    M5Cardputer.Speaker.tone(1200, 120);
    delay(180);
    M5Cardputer.Speaker.tone(1200, 120);
  } else {
    M5Cardputer.Speaker.tone(440, 500);
  }
}

// ---- SD card

static void saveCapture(const Result& r) {
  if (!sdOk || !rawRing) return;
  char path[48];
  snprintf(path, sizeof(path), "/opennek/raw-%lu-%s-%u.rssi", (unsigned long)r.uptimeS, idHex(r.m.deviceId).c_str(), r.m.counter);
  File f = SD.open(path, FILE_WRITE);
  if (!f) return;
  // Same format as host/test/fixtures: raw streamer values, oldest first
  if (rawFull) f.write(rawRing + rawPos, rawSize - rawPos);
  f.write(rawRing, rawPos);
  f.close();
}

static void logResult(const Result& r) {
  if (!sdOk) return;
  bool fresh = !SD.exists("/opennek/tester.csv");
  File f = SD.open("/opennek/tester.csv", FILE_APPEND);
  if (!f) return;
  if (fresh) f.println("uptime_s,device,seq,data,bit,downlink,frames,rssi_dbm,floor_dbm,clock_ppm,repaired,verdict,issues");
  String ranks;
  for (int k : r.m.ranks) ranks += String(k + 1);
  f.printf("%lu,%s,%u,%s,%d,%d,%s,%.1f,%.1f,%d,%d,%s,\"%s\"\n", (unsigned long)r.uptimeS, idHex(r.m.deviceId).c_str(), r.m.counter,
           bytesHex(r.m.payload).c_str(), r.m.bit, r.m.downlink, ranks.c_str(), r.m.rssi, r.floorDbm, r.m.clockErrorPpm, r.repaired,
           VERDICT_NAMES[r.verdict], r.issues.c_str());
  f.close();
  if (r.verdict != V_PASS) saveCapture(r);
}

// ---- Checks

static void onMessage(const sigfox::Message& m) {
  int idx = deviceIndex(m.deviceId);
  if (idx < 0) {
    if (learning) {
      candidate = m.deviceId;
      dirty = true;
    }
    return;
  }
  Result r;
  r.uptimeS = millis() / 1000;
  r.m = m;
  r.floorDbm = rx->floorDbm();
  r.repaired = 0;
  for (size_t i = 0; i < progress.size(); i++) {
    if (progress[i].id == m.deviceId && progress[i].counter == m.counter) {
      r.repaired = progress[i].repaired;
      progress.erase(progress.begin() + i);
      break;
    }
  }
  Verdict v = V_PASS;
  auto issue = [&](Verdict level, const String& text) {
    if (level > v) v = level;
    if (r.issues.length()) r.issues += "; ";
    r.issues += text;
  };

  if (m.ranks.size() < 3) {
    String missing;
    for (int k = 0; k < 3; k++)
      if (std::find(m.ranks.begin(), m.ranks.end(), k) == m.ranks.end()) missing += String(missing.length() ? "," : "") + String(k + 1);
    issue(V_WARN, String("frames ") + m.ranks.size() + "/3, missing #" + missing);
  }
  r.seqDelta = 0;
  if (haveSeq[idx]) {
    r.seqDelta = (m.counter - lastSeq[idx]) & 0xFFF;
    if (r.seqDelta == 0)
      issue(V_WARN, "same seq as before");
    else if (r.seqDelta > 2048)
      issue(V_WARN, String("seq went back ") + (4096 - r.seqDelta));
    else if (r.seqDelta > 1)
      issue(V_WARN, String("seq +") + r.seqDelta + " (" + (r.seqDelta - 1) + " missed)");
  }
  lastSeq[idx] = m.counter;
  haveSeq[idx] = true;
  if (expectedPayload.length()) {
    String got = m.bit >= 0 ? String("bit") + m.bit : bytesHex(m.payload);
    if (!got.equalsIgnoreCase(expectedPayload)) issue(V_FAIL, "payload " + got + " != " + expectedPayload);
  }
  if (m.rssi - r.floorDbm < WEAK_MARGIN_DB) issue(V_WARN, String("weak: ") + String(m.rssi - r.floorDbm, 0) + " dB above noise");
  r.verdict = v;

  counts[v]++;
  history.push_front(r);
  if (history.size() > HISTORY) history.pop_back();
  logResult(r);

  String ranks;
  for (int k : m.ranks) ranks += String(ranks.length() ? "," : "") + String(k + 1);
  Serial.printf(
      "{\"type\":\"message\",\"deviceId\":\"%s\",\"seqNumber\":%u,\"data\":\"%s\",\"bit\":%d,\"ack\":%s,\"rssi\":%.1f,\"floor\":%.1f,"
      "\"frames\":[%s],\"clockErrorPpm\":%d,\"verdict\":\"%s\",\"issues\":\"%s\",\"uptime\":%lu}\n",
      idHex(m.deviceId).c_str(), m.counter, bytesHex(m.payload).c_str(), m.bit, m.downlink ? "true" : "false", m.rssi, r.floorDbm,
      ranks.c_str(), m.clockErrorPpm, VERDICT_NAMES[v], r.issues.c_str(), (unsigned long)r.uptimeS);
  dirty = true;
  if (screen == HISTORY_SCREEN) screen = MAIN;
  beep(v);
}

static void onFrame(const sigfox::DecodedFrame& f) {
  // A frame with a bad CRC cannot be attributed to a device; only count it
  if (!f.crcOk) {
    badFrames++;
    dirty = true;
    return;
  }
  if (deviceIndex(f.deviceId) < 0) return;
  for (Progress& p : progress) {
    if (p.id == f.deviceId && p.counter == f.counter) {
      p.ranks |= 1 << f.rank;
      p.repaired += !f.repaired.empty();
      dirty = true;
      return;
    }
  }
  progress.push_back({f.deviceId, f.counter, (uint8_t)(1 << f.rank), f.repaired.empty() ? 0 : 1});
  dirty = true;
}

static void startReceiver(int rate) {
  sampleRate = rate;
  rx.reset(new sigfox::Receiver(rate, onMessage, onFrame));
  progress.clear();
  free(rawRing);
  rawSize = RAW_KEEP_S * rate;
  rawRing = (uint8_t*)malloc(rawSize);
  rawPos = 0;
  rawFull = false;
}

// ---- Streamer input

static void handleLine(const char* line, size_t len) {
  if (line[0] == 'I' && line[1] == ' ') {
    streamerStatus = String(line + 2);
    const char* sr = strstr(line, "sr=");
    int rate = sr ? atoi(sr + 3) : sampleRate;
    if (rate >= 100 && rate != sampleRate) startReceiver(rate);
    dirty = true;
    return;
  }
  if (line[0] != 'R' || len < 7 || line[5] != ':') return;
  int32_t seq = strtol(String(line + 1).substring(0, 4).c_str(), nullptr, 16);
  if (lastLineSeq >= 0 && seq != ((lastLineSeq + 1) & 0xFFFF)) droppedLines += (seq - lastLineSeq - 1 + 0x10000) & 0xFFFF;
  lastLineSeq = seq;
  uint8_t raw[80];
  size_t n = 0;
  auto nibble = [](char c) { return c <= '9' ? c - '0' : (c & ~0x20) - 'A' + 10; };
  for (size_t i = 6; i + 1 < len && n < sizeof(raw); i += 2) raw[n++] = (nibble(line[i]) << 4) | nibble(line[i + 1]);
  if (rawRing) {
    for (size_t i = 0; i < n; i++) {
      rawRing[rawPos++] = raw[i];
      if (rawPos == rawSize) {
        rawPos = 0;
        rawFull = true;
      }
    }
  }
  rx->pushRaw(raw, n);
  samplesThisSecond += n;
  lastSampleMs = millis();
}

static void readStreamer() {
  static char line[192];
  static size_t len = 0;
  while (lsm.available()) {
    char c = lsm.read();
    if (c == '\n' || c == '\r') {
      if (len) {
        line[len] = 0;
        handleLine(line, len);
        len = 0;
      }
    } else if (len < sizeof(line) - 1) {
      line[len++] = c;
    } else {
      len = 0;  // garbage
    }
  }
}

// ---- Display (240 x 135)

static void drawHeader() {
  canvas.fillRect(0, 0, 240, 12, 0x18E3);
  canvas.setFont(&fonts::Font0);
  canvas.setTextColor(TFT_WHITE);
  canvas.setCursor(3, 2);
  canvas.print("SIGFOX TESTER RC3");
  bool alive = millis() - lastSampleMs < 2000;
  canvas.setCursor(112, 2);
  if (!alive) {
    canvas.setTextColor(TFT_RED);
    canvas.print("NO DATA FROM LSM100A");
    return;
  }
  canvas.setTextColor(TFT_LIGHTGREY);
  canvas.printf("noise %.0fdBm %s%s", rx->floorDbm(), sdOk ? "SD" : "--", muted ? " mute" : "");
}

static void drawFooter(const char* text) {
  canvas.fillRect(0, 124, 240, 11, 0x18E3);
  canvas.setFont(&fonts::Font0);
  canvas.setTextColor(TFT_LIGHTGREY);
  canvas.setCursor(3, 126);
  canvas.print(text);
}

static void drawProgress(int y) {
  canvas.setFont(&fonts::Font0);
  for (const Progress& p : progress) {
    canvas.setTextColor(TFT_CYAN);
    canvas.setCursor(4, y);
    canvas.printf("receiving %s #%u ", idHex(p.id).c_str(), p.counter);
    for (int k = 0; k < 3; k++) {
      int x = 150 + k * 16;
      if (p.ranks & (1 << k))
        canvas.fillRect(x, y, 12, 8, TFT_CYAN);
      else
        canvas.drawRect(x, y, 12, 8, TFT_DARKGREY);
    }
    y += 10;
    if (y > 112) break;
  }
}

static void drawMain() {
  drawHeader();
  int y = 16;
  if (devices.empty() && !learning) {
    canvas.setFont(&fonts::Font2);
    canvas.setTextColor(TFT_WHITE);
    canvas.setCursor(6, 30);
    canvas.print("No device registered.");
    canvas.setCursor(6, 52);
    canvas.print("L: learn from the next");
    canvas.setCursor(6, 68);
    canvas.print("   message   I: type ID");
  } else if (learning) {
    canvas.setFont(&fonts::Font2);
    canvas.setTextColor(TFT_CYAN);
    canvas.setCursor(6, 22);
    if (!candidate) {
      canvas.print("Learn: send one message");
      canvas.setCursor(6, 40);
      canvas.print("from the device to add.");
    } else {
      canvas.print("Device heard:");
      canvas.setFont(&fonts::FreeMonoBold12pt7b);
      canvas.setCursor(6, 44);
      canvas.print(idHex(candidate));
      canvas.setFont(&fonts::Font2);
      canvas.setCursor(6, 74);
      canvas.setTextColor(TFT_WHITE);
      canvas.print("Add it? Y / N");
    }
  } else if (history.empty()) {
    canvas.setFont(&fonts::Font2);
    canvas.setTextColor(TFT_WHITE);
    canvas.setCursor(6, 22);
    canvas.printf("Waiting for %d device%s", devices.size(), devices.size() > 1 ? "s" : "");
    canvas.setFont(&fonts::Font0);
    canvas.setTextColor(TFT_LIGHTGREY);
    for (size_t i = 0; i < devices.size(); i++) {
      canvas.setCursor(10 + (i % 3) * 76, 44 + (i / 3) * 10);
      canvas.print(idHex(devices[i]));
    }
    canvas.setCursor(6, 76);
    canvas.printf("payload: %s", expectedPayload.length() ? expectedPayload.c_str() : "any");
    drawProgress(92);
  } else {
    const Result& r = history.front();
    uint16_t color = VERDICT_COLORS[r.verdict];
    canvas.fillRoundRect(176, y, 60, 26, 4, color);
    canvas.setFont(&fonts::FreeSansBold9pt7b);
    canvas.setTextColor(TFT_BLACK);
    canvas.setTextDatum(middle_center);
    canvas.drawString(VERDICT_NAMES[r.verdict], 206, y + 13);
    canvas.setTextDatum(top_left);

    canvas.setFont(&fonts::FreeMonoBold9pt7b);
    canvas.setTextColor(TFT_WHITE);
    canvas.setCursor(4, y + 12);
    canvas.print(idHex(r.m.deviceId));
    canvas.setFont(&fonts::Font2);
    canvas.setCursor(108, y + 1);
    canvas.printf("#%u", r.m.counter);

    canvas.setFont(&fonts::Font0);
    canvas.setTextColor(TFT_LIGHTGREY);
    canvas.setCursor(4, y + 32);
    String data = r.m.bit >= 0 ? String("bit ") + r.m.bit : (r.m.payload.empty() ? String("(empty)") : bytesHex(r.m.payload));
    canvas.printf("data  %s%s", data.c_str(), r.m.downlink ? "  +downlink req" : "");
    canvas.setCursor(4, y + 44);
    canvas.printf("rssi  %.1f dBm  (+%.0f dB)", r.m.rssi, r.m.rssi - r.floorDbm);
    canvas.setCursor(4, y + 56);
    canvas.print("frames");
    for (int k = 0; k < 3; k++) {
      bool got = std::find(r.m.ranks.begin(), r.m.ranks.end(), k) != r.m.ranks.end();
      int x = 46 + k * 16;
      if (got)
        canvas.fillRect(x, y + 56, 12, 8, TFT_GREEN);
      else
        canvas.fillRect(x, y + 56, 12, 8, TFT_RED);
    }
    canvas.setCursor(100, y + 56);
    canvas.printf("seq %s", r.seqDelta == 0 ? "first" : (String("+") + r.seqDelta).c_str());
    canvas.setCursor(160, y + 56);
    canvas.print(uptime(r.uptimeS));
    canvas.setTextColor(color);
    canvas.setCursor(4, y + 68);
    String issues = r.issues.length() ? r.issues : String("all checks passed");
    canvas.print(issues.substring(0, 38));
    if (issues.length() > 38) {
      canvas.setCursor(4, y + 78);
      canvas.print(issues.substring(38, 76));
    }
    canvas.setTextColor(TFT_DARKGREY);
    canvas.setCursor(4, y + 90);
    canvas.printf("P%d W%d F%d  bad CRC %lu", counts[V_PASS], counts[V_WARN], counts[V_FAIL], (unsigned long)badFrames);
    if (!progress.empty()) {
      canvas.fillRect(0, y + 88, 240, 20, TFT_BLACK);
      drawProgress(y + 92);
    }
  }
  drawFooter(learning ? "Y add  N skip  ` stop learning" : "L learn I id P payload D dev H hist M R");
}

static void drawHistory() {
  drawHeader();
  canvas.setFont(&fonts::Font0);
  int y = 16;
  if (history.empty()) {
    canvas.setTextColor(TFT_LIGHTGREY);
    canvas.setCursor(4, y);
    canvas.print("No messages yet.");
  }
  for (const Result& r : history) {
    canvas.setTextColor(VERDICT_COLORS[r.verdict]);
    canvas.setCursor(2, y);
    String data = r.m.bit >= 0 ? String("bit") + r.m.bit : bytesHex(r.m.payload);
    canvas.printf("%-5s %s %s #%-4u %u/3 %.0f %s", uptime(r.uptimeS).c_str(), VERDICT_NAMES[r.verdict], idHex(r.m.deviceId).c_str(),
                  r.m.counter, r.m.ranks.size(), r.m.rssi, data.substring(0, 8).c_str());
    y += 10;
    if (y > 114) break;
  }
  drawFooter("` back");
}

static void drawDevices() {
  drawHeader();
  canvas.setFont(&fonts::Font2);
  canvas.setTextColor(TFT_WHITE);
  int y = 16;
  for (size_t i = 0; i < devices.size(); i++) {
    canvas.setCursor(6 + (i / 4) * 116, y + (i % 4) * 18);
    canvas.printf("%d  %s", (int)i + 1, idHex(devices[i]).c_str());
  }
  if (devices.empty()) {
    canvas.setCursor(6, y);
    canvas.print("No devices.");
  }
  canvas.setFont(&fonts::Font0);
  canvas.setTextColor(TFT_LIGHTGREY);
  canvas.setCursor(6, 94);
  canvas.printf("payload check: %s", expectedPayload.length() ? expectedPayload.c_str() : "off");
  canvas.setCursor(6, 106);
  canvas.printf("streamer: %s", streamerStatus.length() ? streamerStatus.substring(0, 30).c_str() : "?");
  drawFooter("1-8 remove  I add  ` back");
}

static void drawInput(const char* title, const char* hint) {
  drawHeader();
  canvas.setFont(&fonts::Font2);
  canvas.setTextColor(TFT_WHITE);
  canvas.setCursor(6, 22);
  canvas.print(title);
  canvas.drawRect(4, 44, 232, 26, TFT_LIGHTGREY);
  canvas.setFont(&fonts::FreeMonoBold9pt7b);
  canvas.setCursor(10, 52);
  canvas.print(input + "_");
  canvas.setFont(&fonts::Font0);
  canvas.setTextColor(TFT_LIGHTGREY);
  canvas.setCursor(6, 80);
  canvas.print(hint);
  drawFooter("Enter save  Del erase  ` cancel");
}

static void draw() {
  canvas.fillSprite(TFT_BLACK);
  switch (screen) {
    case MAIN:
      drawMain();
      break;
    case HISTORY_SCREEN:
      drawHistory();
      break;
    case DEVICES:
      drawDevices();
      break;
    case INPUT_ID:
      drawInput("Device ID (8 hex digits)", "As printed by AT$ID, e.g. 00763DA3");
      break;
    case INPUT_PAYLOAD:
      drawInput("Expected payload (hex)", "Empty = accept any payload. bit0/bit1 for AT$SB");
      break;
  }
  canvas.pushSprite(0, 0);
}

// ---- Keyboard

static bool isHex(const String& s) {
  for (char c : s)
    if (!isxdigit((unsigned char)c)) return false;
  return true;
}

static void handleKey(char c, bool enter, bool del) {
  if (screen == INPUT_ID || screen == INPUT_PAYLOAD) {
    if (c == '`') {
      screen = MAIN;
    } else if (del) {
      if (input.length()) input.remove(input.length() - 1);
    } else if (enter) {
      if (screen == INPUT_ID && input.length() >= 1 && input.length() <= 8 && isHex(input)) {
        addDevice(strtoul(input.c_str(), nullptr, 16));
        screen = MAIN;
      } else if (screen == INPUT_PAYLOAD) {
        input.toLowerCase();
        if (input.isEmpty() || input == "bit0" || input == "bit1" || (isHex(input) && input.length() % 2 == 0 && input.length() <= 24)) {
          expectedPayload = input;
          saveSettings();
          screen = MAIN;
        }
      }
    } else if (c && input.length() < 24) {
      input += c;
    }
    return;
  }
  if (learning && candidate) {
    if (c == 'y' || c == 'Y' || enter) addDevice(candidate);
    if (c == 'y' || c == 'Y' || c == 'n' || c == 'N' || enter) {
      candidate = 0;
      learning = false;
      return;
    }
  }
  switch (c) {
    case '`':
      screen = MAIN;
      learning = false;
      candidate = 0;
      break;
    case 'l':
    case 'L':
      learning = !learning;
      candidate = 0;
      screen = MAIN;
      break;
    case 'i':
    case 'I':
      input = "";
      screen = INPUT_ID;
      break;
    case 'p':
    case 'P':
      input = expectedPayload;
      screen = INPUT_PAYLOAD;
      break;
    case 'd':
    case 'D':
      screen = screen == DEVICES ? MAIN : DEVICES;
      break;
    case 'h':
    case 'H':
      screen = screen == HISTORY_SCREEN ? MAIN : HISTORY_SCREEN;
      break;
    case 'm':
    case 'M':
      muted = !muted;
      saveSettings();
      break;
    case 'r':
    case 'R':
      history.clear();
      counts[0] = counts[1] = counts[2] = 0;
      badFrames = 0;
      for (bool& h : haveSeq) h = false;
      break;
    default:
      if (screen == DEVICES && c >= '1' && c <= '8') removeDevice(c - '1');
      break;
  }
}

static void readKeyboard() {
  if (!M5Cardputer.Keyboard.isChange() || !M5Cardputer.Keyboard.isPressed()) return;
  auto& keys = M5Cardputer.Keyboard.keysState();
  if (keys.enter || keys.del) handleKey(0, keys.enter, keys.del);
  for (char c : keys.word) handleKey(c, false, false);
  dirty = true;
}

void setup() {
  auto cfg = M5.config();
  M5Cardputer.begin(cfg, true);
  M5Cardputer.Display.setRotation(1);
  M5Cardputer.Speaker.setVolume(96);
  canvas.setColorDepth(16);
  canvas.createSprite(240, 135);

  Serial.begin();
  lsm.setRxBufferSize(8192);
  lsm.begin(115200, SERIAL_8N1, LSM_RX, LSM_TX);

  loadSettings();
  SPI.begin(SD_SCK, SD_MISO, SD_MOSI, SD_CS);
  sdOk = SD.begin(SD_CS, SPI, 25000000);
  if (sdOk && !SD.exists("/opennek")) SD.mkdir("/opennek");

  startReceiver(sampleRate);
  lsm.write("?\n");
  Serial.printf("{\"type\":\"status\",\"tester\":\"%s\",\"sd\":%s,\"devices\":%u}\n", FW_VERSION, sdOk ? "true" : "false", devices.size());
}

void loop() {
  M5Cardputer.update();
  readStreamer();
  readKeyboard();

  uint32_t now = millis();
  if (now - rateWindowMs >= 1000) {
    samplesPerSecond = samplesThisSecond;
    samplesThisSecond = 0;
    rateWindowMs = now;
    dirty = true;
    static uint32_t ticks = 0;
    if (++ticks % 10 == 0)
      Serial.printf(
          "{\"type\":\"health\",\"uptime\":%lu,\"samplesPerSecond\":%lu,\"floor\":%.1f,\"droppedLines\":%lu,\"badFrames\":%lu,"
          "\"devices\":%u,\"sd\":%s,\"streamer\":\"%s\",\"freeHeap\":%lu}\n",
          (unsigned long)(now / 1000), (unsigned long)samplesPerSecond, rx->floorDbm(), (unsigned long)droppedLines,
          (unsigned long)badFrames, devices.size(), sdOk ? "true" : "false", streamerStatus.c_str(), (unsigned long)ESP.getFreeHeap());
  }
  // Ask the streamer for its status while it is silent (e.g. the LSM100A was powered up later)
  if (now - lastSampleMs > 2000 && now - lastQueryMs > 3000) {
    lsm.write("?\n");
    lastQueryMs = now;
  }
  if (dirty && now - lastDrawMs > 150) {
    dirty = false;
    lastDrawMs = now;
    draw();
  }
}
