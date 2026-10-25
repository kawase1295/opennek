#include "Sx126xSpi.h"

#include "Sx126x.h"

bool Sx126xSpi::begin() {
  pinMode(pins_.nss, OUTPUT);
  digitalWrite(pins_.nss, HIGH);
  pinMode(pins_.busy, INPUT_PULLDOWN);  // without a module, reads as ready and the probe fails fast
  pinMode(pins_.reset, OUTPUT);
  SPI.begin(pins_.sck, pins_.miso, pins_.mosi, pins_.nss);
  digitalWrite(pins_.reset, LOW);
  delay(2);
  digitalWrite(pins_.reset, HIGH);
  delay(5);
  return waitBusy(100000);
}

bool Sx126xSpi::waitBusy(uint32_t timeoutUs) {
  for (uint32_t t0 = micros(); digitalRead(pins_.busy);) {
    if (micros() - t0 > timeoutUs) return false;
  }
  return true;
}

void Sx126xSpi::select() {
  waitBusy();
  SPI.beginTransaction(settings_);
  digitalWrite(pins_.nss, LOW);
}

void Sx126xSpi::deselect() {
  digitalWrite(pins_.nss, HIGH);
  SPI.endTransaction();
}

void Sx126xSpi::command(uint8_t op, const uint8_t* data, size_t n) {
  select();
  SPI.transfer(op);
  for (size_t i = 0; i < n; i++) SPI.transfer(data[i]);
  deselect();
}

void Sx126xSpi::read(uint8_t op, uint8_t* out, size_t n) {
  select();
  SPI.transfer(op);
  SPI.transfer(0x00);  // status
  for (size_t i = 0; i < n; i++) out[i] = SPI.transfer(0x00);
  deselect();
}

void Sx126xSpi::writeRegisters(uint16_t addr, const uint8_t* data, size_t n) {
  select();
  SPI.transfer(sx126x::WriteRegister);
  SPI.transfer(addr >> 8);
  SPI.transfer(addr & 0xFF);
  for (size_t i = 0; i < n; i++) SPI.transfer(data[i]);
  deselect();
}

void Sx126xSpi::readRegisters(uint16_t addr, uint8_t* out, size_t n) {
  select();
  SPI.transfer(sx126x::ReadRegister);
  SPI.transfer(addr >> 8);
  SPI.transfer(addr & 0xFF);
  SPI.transfer(0x00);  // status
  for (size_t i = 0; i < n; i++) out[i] = SPI.transfer(0x00);
  deselect();
}

void Sx126xSpi::writeBuffer(uint8_t offset, const uint8_t* data, size_t n) {
  select();
  SPI.transfer(sx126x::WriteBuffer);
  SPI.transfer(offset);
  for (size_t i = 0; i < n; i++) SPI.transfer(data[i]);
  deselect();
}
