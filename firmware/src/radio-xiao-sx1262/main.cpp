// OpenNEK radio for the Seeed Studio XIAO ESP32S3 & Wio-SX1262 kit: the RSSI streamer of firmware-lsm100a on an
// external SX1262, over the S3's USB serial (USB-JTAG, 115200 bps nominal).
//
// Keeps the SX1262 in wide-band continuous RX and streams the instantaneous RSSI. Sigfox DBPSK is pulse shaped,
// so every phase reversal shows up as a dip in the envelope; the host turns those dips back into bits. It also
// sends the Sigfox downlinks the host asks for, within the limits of TxPolicy.h. The protocol is the one of
// firmware-lsm100a 0.3.0 (lib/RadioStreamer/src/Protocol.h); the status line adds hw=xiao-sx1262.
//
// Wio-SX1262 for XIAO ESP32S3 (as Meshtastic's seeed_xiao_s3 variant): SPI SCK 7 / MISO 8 / MOSI 9, NSS 41,
// RESET 42, BUSY 40. DIO2 drives the antenna switch's TX side and GPIO38 its RX side; DIO3 powers the 1.8 V TCXO.
#include <Arduino.h>

#include "Protocol.h"
#include "SampleClock.h"
#include "Sx126x.h"
#include "Sx126xSpi.h"
#include "TxPolicy.h"

#define FW_VERSION "0.3.0"  // the protocol version of firmware-lsm100a this radio speaks
#define HW_NAME "xiao-sx1262"

#define PIN_RXEN 38
#define TCXO_1V8 0x02
#define TX_TIMEOUT_STEPS 0x00FA00  // 1 s in 15.625 us steps; a frame takes 280 ms

using namespace streamer;

static Sx126xSpi radio({7, 8, 9, 41, 42, 40});
static bool radioOk = false;
static Settings settings;
static TxPolicy txPolicy;
static SampleClock sampleClock;
static LineReader commands;

static uint8_t samples[kLineSamples];
static uint16_t cfo[kLineSamples / kCfoEvery];
static uint8_t nSamples = 0;
static uint16_t lineSeq = 0;

// Writes a whole line or nothing: a full USB buffer (no host reading) must not stall sampling
static void writeLine(const char* line, size_t n) {
  if ((size_t)Serial.availableForWrite() >= n) Serial.write((const uint8_t*)line, n);
}

static void standby() {
  const uint8_t rc = 0x00;
  radio.command(sx126x::SetStandby, &rc, 1);
}

static void configureRxAt(uint32_t hz) {
  standby();
  uint8_t cal[2], freq[4], mod[8], pkt[9];
  sx126x::imageCalibration(hz, cal);
  radio.command(sx126x::CalibrateImage, cal, 2);
  sx126x::frequency(hz, freq);
  radio.command(sx126x::SetRfFrequency, freq, 4);
  sx126x::rxModulation(settings.bwCode, mod);
  radio.command(sx126x::SetModulationParams, mod, 8);
  sx126x::rxPacket(settings.preambleDetector, pkt);
  radio.command(sx126x::SetPacketParams, pkt, 9);
  const uint8_t sync[8] = {0x9C, 0x3B, 0x51, 0xE7, 0x2A, 0xD4, 0x68, 0x0F};
  radio.writeRegisters(sx126x::RegSyncWord, sync, 8);
  const uint8_t gain = settings.boost ? sx126x::kRxGainBoosted : sx126x::kRxGainPowerSaving;
  radio.writeRegisters(sx126x::RegRxGain, &gain, 1);
  const uint8_t noIrq[8] = {};
  radio.command(sx126x::SetDioIrqParams, noIrq, 8);

  digitalWrite(PIN_RXEN, HIGH);
  uint8_t continuous[3];
  sx126x::steps(0xFFFFFF, continuous);
  radio.command(sx126x::SetRx, continuous, 3);
}

static void configureRx() { configureRxAt(settings.freqHz); }

// The sync word written by configureRx reads back only from a real SX126x
static bool radioPresent() {
  uint8_t sync[2] = {};
  radio.readRegisters(sx126x::RegSyncWord, sync, 2);
  return sync[0] == 0x9C && sync[1] == 0x3B;
}

static bool initRadio() {
  if (!radio.begin()) return false;
  standby();
  const uint8_t dcdc = 0x01;
  radio.command(sx126x::SetRegulatorMode, &dcdc, 1);
  uint8_t tcxo[4] = {TCXO_1V8};
  sx126x::steps(320, tcxo + 1);  // 5 ms start-up
  radio.command(sx126x::SetDio3AsTcxoCtrl, tcxo, 4);
  const uint8_t calibrateAll = 0x7F;
  radio.command(sx126x::Calibrate, &calibrateAll, 1);
  delay(5);
  const uint8_t clearErrors[2] = {};
  radio.command(sx126x::ClearDeviceErrors, clearErrors, 2);  // the TCXO start-up error is expected
  const uint8_t on = 0x01;
  radio.command(sx126x::SetDio2AsRfSwitchCtrl, &on, 1);
  const uint8_t gfsk = 0x00;
  radio.command(sx126x::SetPacketType, &gfsk, 1);
  configureRx();
  return radioPresent();
}

// Without the radio there is no status: the host would take it as connected, then see no samples and ask again
// every second. The reason goes out at most every NO_RADIO_EVERY_MS instead.
#define NO_RADIO_EVERY_MS 10000

static void printStatus() {
  if (!radioOk) {
    static uint32_t lastNoRadio = 0;
    static bool told = false;
    if (told && millis() - lastNoRadio < NO_RADIO_EVERY_MS) return;
    told = true;
    lastNoRadio = millis();
    static const char kNoRadio[] = "E radio not found (is the Wio-SX1262 plugged in?)\n";
    writeLine(kNoRadio, sizeof(kNoRadio) - 1);
    return;
  }
  char s[128];
  writeLine(s, formatStatus(s, sizeof(s), settings, FW_VERSION, HW_NAME));
}

static void reply(TxResult r, uint32_t value = 0) {
  char s[32];
  size_t n = formatTxReply(s, sizeof(s), r, value);
  Serial.write((const uint8_t*)s, n);  // the host waits for it: never dropped
}

static uint8_t rssiInst() {
  uint8_t rssi = 0xFF;
  radio.read(sx126x::GetRssiInst, &rssi, 1);
  return rssi;
}

// Strongest instantaneous RSSI (raw) on hz during carrier sense, receiving with the streamer's wide filter
static uint8_t carrierSense(uint32_t hz) {
  configureRxAt(hz);
  delayMicroseconds(1000);  // RSSI settles after entering RX
  uint8_t strongest = 0xFF;
  for (uint32_t t0 = micros(); micros() - t0 < kCarrierSenseUs;) {
    uint8_t rssi = rssiInst();
    if (rssi < strongest) strongest = rssi;
    delayMicroseconds(50);
  }
  return strongest;
}

static void transmitDownlink(const char* args) {
  Downlink dl;
  if (!radioOk || !parseDownlink(args, dl)) return reply(TxResult::Err);
  TxResult verdict = txPolicy.check(settings.txEnabled, dl.hz, millis());
  if (verdict != TxResult::Ok) return reply(verdict);

  uint8_t cs = carrierSense(dl.hz);
  if (TxPolicy::busy(cs)) {
    configureRx();
    return reply(TxResult::Busy, cs);
  }

  standby();
  uint8_t mod[8], pkt[9], pa[4], txp[2], timeout[3];
  sx126x::txModulation(settings.bwCode, mod);
  radio.command(sx126x::SetModulationParams, mod, 8);
  sx126x::txPacket(pkt);
  radio.command(sx126x::SetPacketParams, pkt, 9);
  const uint8_t sync[2] = {0xB2, 0x27};
  radio.writeRegisters(sx126x::RegSyncWord, sync, 2);
  const uint8_t base[2] = {0x00, 0x00};
  radio.command(sx126x::SetBufferBaseAddress, base, 2);
  radio.writeBuffer(0, dl.frame, kDownlinkBytes);
  // Datasheet errata 15.2: better resistance of the TX to antenna mismatch
  uint8_t clamp = 0;
  radio.readRegisters(sx126x::RegTxClampConfig, &clamp, 1);
  clamp |= 0x1E;
  radio.writeRegisters(sx126x::RegTxClampConfig, &clamp, 1);
  sx126x::paConfig(pa);
  radio.command(sx126x::SetPaConfig, pa, 4);
  const uint8_t ocp = 0x38;  // 140 mA
  radio.writeRegisters(sx126x::RegOcp, &ocp, 1);
  sx126x::txParams(TxPolicy::clampDbm(dl.dbm), txp);
  radio.command(sx126x::SetTxParams, txp, 2);
  const uint16_t mask = sx126x::kIrqTxDone | sx126x::kIrqTimeout;
  const uint8_t irq[8] = {mask >> 8, mask & 0xFF, mask >> 8, mask & 0xFF, 0x00, 0x00, 0x00, 0x00};
  radio.command(sx126x::SetDioIrqParams, irq, 8);
  const uint8_t clear[2] = {0xFF, 0xFF};
  radio.command(sx126x::ClearIrqStatus, clear, 2);

  digitalWrite(PIN_RXEN, LOW);  // DIO2 switches the antenna to TX
  sx126x::steps(TX_TIMEOUT_STEPS, timeout);
  uint32_t start = millis();
  radio.command(sx126x::SetTx, timeout, 3);
  uint16_t status = 0;
  while (!(status & mask) && millis() - start < 1500) {
    uint8_t s[2] = {};
    radio.read(sx126x::GetIrqStatus, s, 2);
    status = (s[0] << 8) | s[1];
  }
  uint32_t onAir = millis() - start;
  radio.command(sx126x::ClearIrqStatus, clear, 2);
  txPolicy.sent(millis(), onAir);
  configureRx();  // antenna back to RX before anything else
  if (!(status & sx126x::kIrqTxDone)) return reply(TxResult::Err);
  reply(TxResult::Ok, onAir);
}

static void handleCommand(const char* line) {
  switch (applyCommand(line, settings)) {
    case Action::Transmit:
      return transmitDownlink(line + 1);
    case Action::Reset:
      Serial.flush();
      ESP.restart();
      return;
    case Action::Reconfigure:
      sampleClock.setRate(settings.sampleHz);
      if (radioOk) configureRx();
      printStatus();
      return;
    case Action::Status:
      printStatus();
      return;
    case Action::None:
      return;
  }
}

static void flushLine() {
  char line[kMaxLine];
  writeLine(line, formatSamples(line, lineSeq, samples, nSamples));
  if (settings.cfo) writeLine(line, formatCfo(line, lineSeq, cfo, nSamples / kCfoEvery));
  lineSeq++;
  nSamples = 0;
}

void setup() {
  pinMode(PIN_RXEN, OUTPUT);
  digitalWrite(PIN_RXEN, LOW);
  Serial.setTxBufferSize(4096);
  Serial.setTxTimeoutMs(0);
  Serial.begin(115200);
  radioOk = initRadio();
  printStatus();
  sampleClock.start(micros(), settings.sampleHz);
}

void loop() {
  while (Serial.available()) {
    if (const char* line = commands.push(Serial.read())) handleCommand(line);
  }
  if (!radioOk || !sampleClock.due(micros())) return;
  if (settings.cfo && nSamples % kCfoEvery == 0) {
    uint8_t r[2];
    radio.readRegisters(sx126x::RegCfo, r, 2);
    cfo[nSamples / kCfoEvery] = ((r[0] & 0x0F) << 8) | r[1];
  }
  samples[nSamples++] = rssiInst();
  if (nSamples == kLineSamples) flushLine();
}
