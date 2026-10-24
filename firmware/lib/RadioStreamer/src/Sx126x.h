// SX126x command opcodes and parameter bytes (Semtech SX1261/2 datasheet), as the streamer uses them. Pure
// functions: the radio driver only sends what these build.
#pragma once

#include <stdint.h>

namespace sx126x {

enum Opcode : uint8_t {
  ClearIrqStatus = 0x02,
  ClearDeviceErrors = 0x07,
  SetDioIrqParams = 0x08,
  WriteRegister = 0x0D,
  WriteBuffer = 0x0E,
  GetIrqStatus = 0x12,
  GetRssiInst = 0x15,
  ReadRegister = 0x1D,
  SetStandby = 0x80,
  SetRx = 0x82,
  SetTx = 0x83,
  SetRfFrequency = 0x86,
  Calibrate = 0x89,
  SetPacketType = 0x8A,
  SetModulationParams = 0x8B,
  SetPacketParams = 0x8C,
  SetTxParams = 0x8E,
  SetBufferBaseAddress = 0x8F,
  SetPaConfig = 0x95,
  SetRegulatorMode = 0x96,
  SetDio3AsTcxoCtrl = 0x97,
  CalibrateImage = 0x98,
  SetDio2AsRfSwitchCtrl = 0x9D,
};

enum Register : uint16_t {
  RegCfo = 0x06B0,  // GFSK carrier frequency offset, 12 bit signed (STM32WL RM0461 GCFORH/GCFORL)
  RegSyncWord = 0x06C0,
  RegRxGain = 0x08AC,
  RegTxClampConfig = 0x08D8,
  RegOcp = 0x08E7,
};

constexpr uint32_t kXtalHz = 32000000;
constexpr uint8_t kRxGainBoosted = 0x96;
constexpr uint8_t kRxGainPowerSaving = 0x94;
constexpr uint16_t kIrqTxDone = 0x0001;
constexpr uint16_t kIrqTimeout = 0x0200;

// SetRfFrequency
void frequency(uint32_t hz, uint8_t out[4]);
// CalibrateImage for the band that contains hz (as Semtech's reference driver picks it)
void imageCalibration(uint32_t hz, uint8_t out[2]);
// GFSK used as a wide-band receiver: 50 kbps, no pulse shaping, 25 kHz deviation
void rxModulation(uint8_t bwCode, uint8_t out[8]);
void rxPacket(uint8_t preambleDetector, uint8_t out[9]);
// Sigfox downlink: GFSK 600 bps, BT 1, 800 Hz deviation
void txModulation(uint8_t bwCode, uint8_t out[8]);
// 16-bit preamble, 16-bit sync word, fixed length of the downlink frame, no CRC, no whitening
void txPacket(uint8_t out[9]);
// High-power PA set up as the datasheet's +22 dBm table, so the TX power byte is the output in dBm
void paConfig(uint8_t out[4]);
// SetTxParams with a 40 us ramp
void txParams(int32_t dbm, uint8_t out[2]);
// A 24-bit timeout in 15.625 us steps (SetRx, SetTx, SetDio3AsTcxoCtrl)
void steps(uint32_t n, uint8_t out[3]);

}  // namespace sx126x
