# Flashing the firmware

**English** | [日本語](flashing.ja.md)

`opennek-<env>-<version>.bin` is a merged image containing the bootloader, partition table and application.
Just write it to **address 0x0**.

| File | Role | Board |
|---|---|---|
| `opennek-gateway-*.bin` | Gateway (plugged into the host over USB) | ESP32 DevKit etc. (USB via CP210x / CH340) |
| `opennek-device-*.bin` | Device (AT modem emulator) | same as above |
| `opennek-gateway-s3-*.bin` | Gateway | ESP32-S3 (native USB) |
| `opennek-device-s3-*.bin` | Device | ESP32-S3 |

## From a browser (recommended on Windows / WSL)

1. Open <https://espressif.github.io/esptool-js/> in Chrome or Edge
2. Click **Connect** and select the ESP32's COM port
3. Set Flash Address to `0x0`, choose one of the `.bin` files above, and click **Program**

This flashes directly from Windows without attaching the USB device to WSL.

## With esptool

```bash
esptool.py --chip esp32 write_flash 0x0 opennek-gateway-0.1.0.bin
```

Use `--chip esp32s3` for ESP32-S3.

## From source (PlatformIO)

```bash
cd firmware
pio run -e gateway -t upload
pio run -e device -t upload
```
