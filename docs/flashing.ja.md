# ファームウェアの書き込み

[English](flashing.md) | **日本語**

`opennek-<env>-<version>.bin` は bootloader・パーティション・アプリをまとめた統合イメージで、
**アドレス 0x0** に書き込むだけで動きます。

| ファイル | 用途 | ボード |
|---|---|---|
| `opennek-gateway-*.bin` | ゲートウェイ (ホストに USB で挿す) | ESP32 DevKit など (CP210x / CH340 経由の USB) |
| `opennek-device-*.bin` | デバイス (AT モデムエミュレータ) | 同上 |
| `opennek-gateway-s3-*.bin` | ゲートウェイ | ESP32-S3 (ネイティブ USB) |
| `opennek-device-s3-*.bin` | デバイス | ESP32-S3 |

## ブラウザから (Windows / WSL でもおすすめ)

1. Chrome / Edge で <https://espressif.github.io/esptool-js/> を開く
2. **Connect** → ESP32 の COM ポートを選ぶ
3. Flash Address を `0x0`、File に上の `.bin` を指定して **Program**

WSL に USB を渡さずに Windows 側から直接書き込めます。

## esptool から

```bash
esptool.py --chip esp32 write_flash 0x0 opennek-gateway-0.1.0.bin
```

ESP32-S3 は `--chip esp32s3`。

## ソースから (PlatformIO)

```bash
cd firmware
pio run -e gateway -t upload
pio run -e device -t upload
```
