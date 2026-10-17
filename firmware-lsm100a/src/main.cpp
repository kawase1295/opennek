// OpenNEK RSSI streamer for the SEONG JI LSM100A (STM32WLE5).
//
// Keeps the sub-GHz radio in wide-band continuous RX and streams the instantaneous RSSI over
// LPUART1 (PA2/PA3, 115200 bps). Sigfox DBPSK is pulse shaped, so every phase reversal shows up
// as a dip in the envelope; the host turns those dips back into bits.
//
// Output lines:
//   I fw=0.1.0 f=923200000 bw=10 sr=2000 boost=1     status (on boot and in reply to "?")
//   R<seq:4 hex>:<64 samples as 2 hex digits>         raw RSSI, dBm = -value / 2
//   C<seq:4 hex>:<8 values as 3 hex digits>           GFSK carrier frequency offset (only with C1), one value
//                                                     per 8 RSSI samples of the R line with the same seq
//   D <result>                                        reply to T: ok ms=<on air> | busy rssi=<dBm> | off | range |
//                                                     limit | err
// Commands (one per line):
//   F<hz>   RX frequency            W<n>  RX bandwidth code (SX126x RX_BW, 10 = 234.3 kHz)
//   S<hz>   sample rate (100-4000)  B<0|1> boosted LNA gain
//   C<0|1>  stream the CFO register P<n>  preamble detector code (0 = off, 4 = 8 bits, ...)
//   ?       status                  X     reset (send '1' right after to enter the IAP bootloader)
//   E<0|1>  allow T (off after every reset)
//   T<hz>,<dBm>,<30 hex>  send one Sigfox downlink frame (15 bytes of DL-PHY content): carrier sense, then GFSK
//           600 bps / 800 Hz deviation / BT 1, preamble AAAA, sync word B227 (as SJI's RF_Tx_Local_loop), then
//           back to streaming. Only in 922.1-922.3 MHz, at most TX_MAX_DBM.
#include <Arduino.h>

#define FW_VERSION "0.3.0"

// LSM100A radio board configuration (LSM_RF_CHANNEL_SWITCH = 1 in the SDK)
#define RF_CTRL1 PB12
#define RF_CTRL2 PC13
#define TCXO_1V7 0x01
#define XTAL_HZ 32000000ULL

#define LINE_SAMPLES 64
#define CFO_EVERY 8
// GFSK carrier frequency offset, 12 bit signed (STM32WL RM0461 GCFORH/GCFORL; not in the SX126x datasheet)
#define REG_GCFORH 0x06B0
#define REG_SYNCWORD 0x06C0
#define REG_OCP 0x08E7

// Downlink transmission. These limits are enforced here, whatever the host asks for.
#define TX_MIN_HZ 922100000UL  // the 922.2 MHz unit channel (ARIB STD-T108): downlink = uplink frame 1 - 1 MHz
#define TX_MAX_HZ 922300000UL
#define TX_MAX_DBM 10  // 10 mW from the low-power PA (the LSM100A has no high-power path)
// Carrier sense as sigfox-ep-lib's RC3 LBT (SIGFOX_SPECTRUM_ACCESS_LBT_M80): -80 dBm for 5 ms in ~200 kHz
#define CS_BUSY_RAW 160  // raw RSSI = -2 * dBm, so a lower value is a stronger signal
#define CS_US 5000
#define TX_PAUSE_MS 100            // between two transmissions (ARIB STD-T108: at least 50 ms)
#define TX_BUDGET_MS 360000UL      // on-air time per hour
#define TX_TIMEOUT_STEPS 0x00FA00  // 1 s in 15.625 us steps; a frame takes 280 ms
#define DL_BYTES 15

static Uart link(PA3, PA2);
static SUBGHZ_HandleTypeDef hsubghz;

static uint32_t freqHz = 923200000;  // Sigfox RC3 uplink centre
static uint8_t bwCode = 0x0A;        // 234.3 kHz covers the 192 kHz uplink band
static uint32_t sampleHz = 2000;     // 20 samples per bit at 100 bps
static bool boost = true;
static bool cfoOn = false;
static uint8_t preambleDetector = 0x04;
static bool txEnabled = false;
static bool txDone = false;  // at least one transmission since reset
static uint32_t lastTxEnd = 0;
static uint32_t budgetStart = 0;
static uint32_t budgetUsedMs = 0;
static uint16_t cfo[LINE_SAMPLES / CFO_EVERY];

static uint8_t samples[LINE_SAMPLES];
static uint8_t nSamples = 0;
static uint16_t lineSeq = 0;
static uint32_t nextSampleUs = 0;
static char cmd[64];
static uint8_t cmdLen = 0;

extern "C" void HAL_SUBGHZ_MspInit(SUBGHZ_HandleTypeDef*) { __HAL_RCC_SUBGHZSPI_CLK_ENABLE(); }

static void radioCmd(SUBGHZ_RadioSetCmd_t op, const uint8_t* data, uint16_t len) {
  HAL_SUBGHZ_ExecSetCmd(&hsubghz, op, const_cast<uint8_t*>(data), len);
}

static void put24(uint8_t* p, uint32_t v) {
  p[0] = v >> 16;
  p[1] = v >> 8;
  p[2] = v;
}

static void standby() {
  const uint8_t standbyRc = 0x00;
  radioCmd(RADIO_SET_STANDBY, &standbyRc, 1);
}

static void setFrequency(uint32_t hz) {
  uint8_t freq[4];
  uint32_t frf = (uint32_t)(((uint64_t)hz << 25) / XTAL_HZ);
  freq[0] = frf >> 24;
  freq[1] = frf >> 16;
  freq[2] = frf >> 8;
  freq[3] = frf;
  // Image calibration for the 902-928 MHz band
  const uint8_t image[2] = {0xE1, 0xE9};
  radioCmd(RADIO_CALIBRATEIMAGE, image, 2);
  radioCmd(RADIO_SET_RFFREQUENCY, freq, 4);
}

static void configureRxAt(uint32_t hz) {
  standby();
  setFrequency(hz);

  // GFSK RX is only used as a wide-band receiver; the bit rate and sync word are irrelevant
  uint8_t mod[8];
  put24(mod, (uint32_t)(32 * XTAL_HZ / 50000));  // 50 kbps
  mod[3] = 0x00;                                 // no pulse shaping
  mod[4] = bwCode;
  put24(mod + 5, (uint32_t)((25000ULL << 25) / XTAL_HZ));  // 25 kHz deviation
  radioCmd(RADIO_SET_MODULATIONPARAMS, mod, 8);

  const uint8_t pkt[9] = {0x00, 0x08, preambleDetector, 0x40, 0x00, 0x00, 0xFF, 0x01, 0x00};
  radioCmd(RADIO_SET_PACKETPARAMS, pkt, 9);
  const uint8_t sync[8] = {0x9C, 0x3B, 0x51, 0xE7, 0x2A, 0xD4, 0x68, 0x0F};
  HAL_SUBGHZ_WriteRegisters(&hsubghz, REG_SYNCWORD, const_cast<uint8_t*>(sync), 8);
  uint8_t gain = boost ? 0x96 : 0x94;
  HAL_SUBGHZ_WriteRegisters(&hsubghz, 0x08AC, &gain, 1);

  const uint8_t noIrq[8] = {};
  radioCmd(RADIO_CFG_DIOIRQ, noIrq, 8);

  digitalWrite(RF_CTRL1, HIGH);  // RF switch: RX
  digitalWrite(RF_CTRL2, LOW);
  const uint8_t continuous[3] = {0xFF, 0xFF, 0xFF};
  radioCmd(RADIO_SET_RX, continuous, 3);
}

static void configureRx() { configureRxAt(freqHz); }

static bool initRadio() {
  hsubghz.Init.BaudratePrescaler = SUBGHZSPI_BAUDRATEPRESCALER_4;
  if (HAL_SUBGHZ_Init(&hsubghz) != HAL_OK) return false;
  const uint8_t standbyRc = 0x00;
  radioCmd(RADIO_SET_STANDBY, &standbyRc, 1);
  const uint8_t ldo = 0x00;
  radioCmd(RADIO_SET_REGULATORMODE, &ldo, 1);
  uint8_t tcxo[4] = {TCXO_1V7};
  put24(tcxo + 1, 320);  // 5 ms start-up, in 15.625 us steps
  radioCmd(RADIO_SET_TCXOMODE, tcxo, 4);
  const uint8_t calibrateAll = 0x7F;
  radioCmd(RADIO_CALIBRATE, &calibrateAll, 1);
  delay(5);
  const uint8_t clearErrors[2] = {};
  radioCmd(RADIO_CLR_ERROR, clearErrors, 2);  // the TCXO start-up error is expected
  const uint8_t gfsk = 0x00;
  radioCmd(RADIO_SET_PACKETTYPE, &gfsk, 1);
  configureRx();
  return true;
}

static void printStatus() {
  char s[112];
  int n = snprintf(s, sizeof(s), "I fw=%s f=%lu bw=%u sr=%lu boost=%u cfo=%u pd=%u tx=%u\n", FW_VERSION, (unsigned long)freqHz,
                   bwCode, (unsigned long)sampleHz, boost ? 1 : 0, cfoOn ? 1 : 0, preambleDetector, txEnabled ? 1 : 0);
  link.write(s, n);
}

static void reply(const char* text) {
  link.write("D ", 2);
  link.write(text, strlen(text));
  link.write("\n", 1);
}

static int hexDigit(char c) {
  if (c >= '0' && c <= '9') return c - '0';
  if (c >= 'A' && c <= 'F') return c - 'A' + 10;
  if (c >= 'a' && c <= 'f') return c - 'a' + 10;
  return -1;
}

// Strongest instantaneous RSSI (raw) on hz during CS_US, receiving with the streamer's wide filter
static uint8_t carrierSense(uint32_t hz) {
  configureRxAt(hz);
  delayMicroseconds(1000);  // RSSI settles after entering RX
  uint8_t strongest = 0xFF;
  for (uint32_t t0 = micros(); micros() - t0 < CS_US;) {
    uint8_t rssi = 0xFF;
    HAL_SUBGHZ_ExecGetCmd(&hsubghz, RADIO_GET_RSSIINST, &rssi, 1);
    if (rssi < strongest) strongest = rssi;
    delayMicroseconds(50);
  }
  return strongest;
}

// T<hz>,<dBm>,<30 hex>
static void transmitDownlink(const char* args) {
  char* p;
  uint32_t hz = strtoul(args, &p, 10);
  if (*p++ != ',') return reply("err");
  long dbm = strtol(p, &p, 10);
  if (*p++ != ',') return reply("err");
  uint8_t frame[DL_BYTES];
  for (int i = 0; i < DL_BYTES; i++) {
    int hi = hexDigit(p[2 * i]), lo = hi < 0 ? -1 : hexDigit(p[2 * i + 1]);
    if (lo < 0) return reply("err");
    frame[i] = (hi << 4) | lo;
  }
  if (p[2 * DL_BYTES]) return reply("err");
  if (!txEnabled) return reply("off");
  if (hz < TX_MIN_HZ || hz > TX_MAX_HZ) return reply("range");
  uint32_t now = millis();
  if (now - budgetStart >= 3600000UL) {
    budgetStart = now;
    budgetUsedMs = 0;
  }
  if ((txDone && now - lastTxEnd < TX_PAUSE_MS) || budgetUsedMs + 1000 > TX_BUDGET_MS) return reply("limit");
  if (dbm > TX_MAX_DBM) dbm = TX_MAX_DBM;
  if (dbm < -17) dbm = -17;

  uint8_t cs = carrierSense(hz);
  if (cs <= CS_BUSY_RAW) {
    configureRx();
    char s[32];
    snprintf(s, sizeof(s), "busy rssi=-%u.%u", cs / 2, (cs & 1) * 5);
    return reply(s);
  }

  standby();
  uint8_t mod[8];
  put24(mod, (uint32_t)(32 * XTAL_HZ / 600));  // 600 bps
  mod[3] = 0x0B;                              // Gaussian BT 1
  mod[4] = bwCode;                            // RX only
  put24(mod + 5, (uint32_t)((800ULL << 25) / XTAL_HZ));  // 800 Hz deviation
  radioCmd(RADIO_SET_MODULATIONPARAMS, mod, 8);
  // 16-bit preamble, 16-bit sync word, fixed length, no CRC, no whitening
  const uint8_t pkt[9] = {0x00, 0x10, 0x00, 0x10, 0x00, 0x00, DL_BYTES, 0x01, 0x00};
  radioCmd(RADIO_SET_PACKETPARAMS, pkt, 9);
  const uint8_t sync[2] = {0xB2, 0x27};
  HAL_SUBGHZ_WriteRegisters(&hsubghz, REG_SYNCWORD, const_cast<uint8_t*>(sync), 2);
  const uint8_t base[2] = {0x00, 0x00};
  radioCmd(RADIO_SET_BUFFERBASEADDRESS, base, 2);
  HAL_SUBGHZ_WriteBuffer(&hsubghz, 0, frame, DL_BYTES);
  // Low-power PA set for 10 dBm max, as SUBGRF_SetTxParams does when the board's LP maximum is 10 dBm
  const uint8_t pa[4] = {0x01, 0x00, 0x01, 0x01};
  radioCmd(RADIO_SET_PACONFIG, pa, 4);
  uint8_t ocp = 0x18;
  HAL_SUBGHZ_WriteRegisters(&hsubghz, REG_OCP, &ocp, 1);
  const uint8_t txParams[2] = {(uint8_t)(int8_t)(0x0D - (TX_MAX_DBM - dbm)), 0x02};  // 40 us ramp
  radioCmd(RADIO_SET_TXPARAMS, txParams, 2);
  const uint8_t irq[8] = {0x02, 0x01, 0x02, 0x01, 0x00, 0x00, 0x00, 0x00};  // TxDone | Timeout
  radioCmd(RADIO_CFG_DIOIRQ, irq, 8);
  const uint8_t clear[2] = {0xFF, 0xFF};
  radioCmd(RADIO_CLR_IRQSTATUS, clear, 2);

  digitalWrite(RF_CTRL1, HIGH);  // RF switch: low-power TX
  digitalWrite(RF_CTRL2, HIGH);
  const uint8_t timeout[3] = {TX_TIMEOUT_STEPS >> 16, (TX_TIMEOUT_STEPS >> 8) & 0xFF, TX_TIMEOUT_STEPS & 0xFF};
  uint32_t start = millis();
  radioCmd(RADIO_SET_TX, timeout, 3);
  uint16_t status = 0;
  while (!(status & 0x0201) && millis() - start < 1500) {
    uint8_t s[2] = {};
    HAL_SUBGHZ_ExecGetCmd(&hsubghz, RADIO_GET_IRQSTATUS, s, 2);
    status = (s[0] << 8) | s[1];
  }
  uint32_t onAir = millis() - start;
  radioCmd(RADIO_CLR_IRQSTATUS, clear, 2);
  txDone = true;
  lastTxEnd = millis();
  budgetUsedMs += onAir;
  configureRx();  // RF switch back to RX before anything else
  if (!(status & 0x0001)) return reply("err");
  char s[24];
  snprintf(s, sizeof(s), "ok ms=%lu", (unsigned long)onAir);
  reply(s);
}

static void handleCommand(const char* c) {
  if (c[0] == 'T') return transmitDownlink(c + 1);
  long v = atol(c + 1);
  switch (c[0]) {
    case 'F':
      if (v >= 150000000 && v <= 960000000) freqHz = v;
      break;
    case 'W':
      if (v >= 0 && v <= 0x1F) bwCode = v;
      break;
    case 'S':
      if (v >= 100 && v <= 4000) sampleHz = v;
      break;
    case 'B':
      boost = v != 0;
      break;
    case 'C':
      cfoOn = v != 0;
      break;
    case 'P':
      if (v >= 0 && v <= 7) preambleDetector = v;
      break;
    case 'E':
      txEnabled = v != 0;
      break;
    case 'X':
      link.flush();
      NVIC_SystemReset();
      break;
    case '?':
      break;
    default:
      return;
  }
  if (c[0] != '?') configureRx();
  printStatus();
}

static void readCommands() {
  while (link.available()) {
    char ch = link.read();
    if (ch == '\r' || ch == '\n') {
      if (cmdLen) {
        cmd[cmdLen] = 0;
        cmdLen = 0;
        handleCommand(cmd);
      }
    } else if (cmdLen < sizeof(cmd) - 1) {
      cmd[cmdLen++] = ch;
    }
  }
}

static void flushLine() {
  static const char digits[] = "0123456789ABCDEF";
  char line[1 + 4 + 1 + LINE_SAMPLES * 2 + 2];
  char* p = line;
  *p++ = 'R';
  for (int s = 12; s >= 0; s -= 4) *p++ = digits[(lineSeq >> s) & 0xF];
  *p++ = ':';
  for (uint8_t i = 0; i < nSamples; i++) {
    *p++ = digits[samples[i] >> 4];
    *p++ = digits[samples[i] & 0xF];
  }
  *p++ = '\n';
  link.write(line, p - line);
  if (cfoOn) {
    p = line;
    *p++ = 'C';
    for (int s = 12; s >= 0; s -= 4) *p++ = digits[(lineSeq >> s) & 0xF];
    *p++ = ':';
    for (uint8_t i = 0; i < nSamples / CFO_EVERY; i++)
      for (int s = 8; s >= 0; s -= 4) *p++ = digits[(cfo[i] >> s) & 0xF];
    *p++ = '\n';
    link.write(line, p - line);
  }
  lineSeq++;
  nSamples = 0;
}

void setup() {
  pinMode(RF_CTRL1, OUTPUT);
  pinMode(RF_CTRL2, OUTPUT);
  link.begin(115200);
  if (!initRadio()) link.println("E radio init failed");
  printStatus();
  nextSampleUs = micros();
}

void loop() {
  readCommands();
  uint32_t now = micros();
  if ((int32_t)(now - nextSampleUs) < 0) return;
  nextSampleUs += 1000000UL / sampleHz;
  // Fell behind (e.g. while reconfiguring): resynchronise instead of bursting
  if ((int32_t)(now - nextSampleUs) > 100000) nextSampleUs = now;
  uint8_t rssi = 0;
  HAL_SUBGHZ_ExecGetCmd(&hsubghz, RADIO_GET_RSSIINST, &rssi, 1);
  if (cfoOn && nSamples % CFO_EVERY == 0) {
    uint8_t r[2];
    HAL_SUBGHZ_ReadRegisters(&hsubghz, REG_GCFORH, r, 2);
    cfo[nSamples / CFO_EVERY] = ((r[0] & 0x0F) << 8) | r[1];
  }
  samples[nSamples++] = rssi;
  if (nSamples == LINE_SAMPLES) flushLine();
}
