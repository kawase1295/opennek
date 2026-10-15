// OpenNEK RSSI streamer for the SEONG JI LSM100A (STM32WLE5).
//
// Keeps the sub-GHz radio in wide-band continuous RX and streams the instantaneous RSSI over
// LPUART1 (PA2/PA3, 115200 bps). Sigfox DBPSK is pulse shaped, so every phase reversal shows up
// as a dip in the envelope; the host turns those dips back into bits.
//
// Output lines:
//   I fw=0.1.0 f=923200000 bw=10 sr=2000 boost=1     status (on boot and in reply to "?")
//   R<seq:4 hex>:<64 samples as 2 hex digits>         raw RSSI, dBm = -value / 2
// Commands (one per line):
//   F<hz>   RX frequency            W<n>  RX bandwidth code (SX126x RX_BW, 10 = 234.3 kHz)
//   S<hz>   sample rate (100-4000)  B<0|1> boosted LNA gain
//   ?       status                  X     reset (send '1' right after to enter the IAP bootloader)
#include <Arduino.h>

#define FW_VERSION "0.1.0"

// LSM100A radio board configuration (LSM_RF_CHANNEL_SWITCH = 1 in the SDK)
#define RF_CTRL1 PB12
#define RF_CTRL2 PC13
#define TCXO_1V7 0x01
#define XTAL_HZ 32000000ULL

#define LINE_SAMPLES 64

static Uart link(PA3, PA2);
static SUBGHZ_HandleTypeDef hsubghz;

static uint32_t freqHz = 923200000;  // Sigfox RC3 uplink centre
static uint8_t bwCode = 0x0A;        // 234.3 kHz covers the 192 kHz uplink band
static uint32_t sampleHz = 2000;     // 20 samples per bit at 100 bps
static bool boost = true;

static uint8_t samples[LINE_SAMPLES];
static uint8_t nSamples = 0;
static uint16_t lineSeq = 0;
static uint32_t nextSampleUs = 0;
static char cmd[32];
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

static void configureRx() {
  const uint8_t standbyRc = 0x00;
  radioCmd(RADIO_SET_STANDBY, &standbyRc, 1);

  uint8_t freq[4];
  uint32_t frf = (uint32_t)(((uint64_t)freqHz << 25) / XTAL_HZ);
  freq[0] = frf >> 24;
  freq[1] = frf >> 16;
  freq[2] = frf >> 8;
  freq[3] = frf;
  // Image calibration for the 902-928 MHz band
  const uint8_t image[2] = {0xE1, 0xE9};
  radioCmd(RADIO_CALIBRATEIMAGE, image, 2);
  radioCmd(RADIO_SET_RFFREQUENCY, freq, 4);

  // GFSK RX is only used as a wide-band receiver; the bit rate and sync word are irrelevant
  uint8_t mod[8];
  put24(mod, (uint32_t)(32 * XTAL_HZ / 50000));  // 50 kbps
  mod[3] = 0x00;                                 // no pulse shaping
  mod[4] = bwCode;
  put24(mod + 5, (uint32_t)((25000ULL << 25) / XTAL_HZ));  // 25 kHz deviation
  radioCmd(RADIO_SET_MODULATIONPARAMS, mod, 8);

  const uint8_t pkt[9] = {0x00, 0x08, 0x04, 0x40, 0x00, 0x00, 0xFF, 0x01, 0x00};
  radioCmd(RADIO_SET_PACKETPARAMS, pkt, 9);
  const uint8_t sync[8] = {0x9C, 0x3B, 0x51, 0xE7, 0x2A, 0xD4, 0x68, 0x0F};
  HAL_SUBGHZ_WriteRegisters(&hsubghz, 0x06C0, const_cast<uint8_t*>(sync), 8);
  uint8_t gain = boost ? 0x96 : 0x94;
  HAL_SUBGHZ_WriteRegisters(&hsubghz, 0x08AC, &gain, 1);

  const uint8_t noIrq[8] = {};
  radioCmd(RADIO_CFG_DIOIRQ, noIrq, 8);

  digitalWrite(RF_CTRL1, HIGH);  // RF switch: RX
  digitalWrite(RF_CTRL2, LOW);
  const uint8_t continuous[3] = {0xFF, 0xFF, 0xFF};
  radioCmd(RADIO_SET_RX, continuous, 3);
}

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
  char s[80];
  int n = snprintf(s, sizeof(s), "I fw=%s f=%lu bw=%u sr=%lu boost=%u\n", FW_VERSION, (unsigned long)freqHz, bwCode,
                   (unsigned long)sampleHz, boost ? 1 : 0);
  link.write(s, n);
}

static void handleCommand(const char* c) {
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
  samples[nSamples++] = rssi;
  if (nSamples == LINE_SAMPLES) flushLine();
}
