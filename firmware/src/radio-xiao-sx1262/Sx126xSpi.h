// SPI access to an external SX126x: commands, registers and the data buffer, each waiting for BUSY to drop.
#pragma once

#include <Arduino.h>
#include <SPI.h>

class Sx126xSpi {
 public:
  struct Pins {
    int8_t sck, miso, mosi, nss, reset, busy;
  };

  explicit Sx126xSpi(const Pins& pins) : pins_(pins) {}

  // Sets up the pins and SPI, then resets the chip; false when it never becomes ready
  bool begin();
  void command(uint8_t op, const uint8_t* data, size_t n);
  // A get command: the status byte is skipped, n bytes are read into out
  void read(uint8_t op, uint8_t* out, size_t n);
  void writeRegisters(uint16_t addr, const uint8_t* data, size_t n);
  void readRegisters(uint16_t addr, uint8_t* out, size_t n);
  void writeBuffer(uint8_t offset, const uint8_t* data, size_t n);

 private:
  bool waitBusy(uint32_t timeoutUs = 20000);
  void select();
  void deselect();

  Pins pins_;
  SPISettings settings_{8000000, MSBFIRST, SPI_MODE0};
};
