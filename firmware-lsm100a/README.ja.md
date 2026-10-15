# LSM100A RSSI ストリーマ

[English](README.md) | **日本語**

SEONG JI **LSM100A**（STM32WLE5、Sigfox + LoRa モジュール）を、実機の Sigfox アップリンク
（RC3、923.2MHz）を受ける広帯域の包絡線受信機にする実験用ファームです。

Sigfox のアップリンクは波形整形した DBPSK なので、位相が反転するたびに振幅がゼロ付近まで落ち込みます。
このファームは無線部を 234kHz 幅（アップリンク帯 192kHz より広い）で受信させ続け、
瞬時 RSSI を 2kHz で流します。落ち込みをビットに戻すのはホスト側です。

> 書き込むと AT ファームが消えます。モジュールの Sigfox 認証情報（ID / PAC / 鍵）も失われる可能性があるので、
> 受信専用にしてよいモジュールを使ってください。

## 配線

```
LSM100A (受信機)              M5Stack Cardputer (または任意の ESP32-S3)      PC
UART2_TX ──────────────────▶ G1 (GPIO1)
UART2_RX ◀────────────────── G2 (GPIO2)       USB ─────────────────▶ /dev/ttyACM0
VCC 3.3V / GND ───────────── 3.3V LDO / GND
```

3.3V の USB-UART 変換器でもかまいません。Cardputer を使う場合は、先に中継ファームを書き込みます。
PC が開いたボーレートに UART 側も自動で合わせます。

```bash
cd firmware && pio run -e bridge-s3 -t upload
```

## ビルドと書き込み

```bash
cd firmware-lsm100a && pio run                       # .pio/build/lsm100a/firmware.bin
cd host && bun tools/lsm-flash.ts /dev/ttyACM0 ../firmware-lsm100a/.pio/build/lsm100a/firmware.bin
```

`lsm-flash.ts` はモジュールをリセットし（工場出荷の AT ファームなら ATZ、このファームなら `X`）、
IAP ブートローダと Ymodem で通信します。自動でリセットされない場合は、
「Waiting for the bootloader」と出ている間に RST を一瞬 GND に落とすか、電源を入れ直してください。

工場出荷の AT ファームに戻すときは、[LSM100A SDK](https://github.com/Support-SJI/LSM100A) の
`FW/LSM100A_SDK_V104_240129.bin` を同じ手順で書き込み、`AT$RFS` を実行します。

## キャプチャ

```bash
cd host && bun tools/rssi-capture.ts /dev/ttyACM0 --out capture
```

別の Sigfox デバイス（2 台目の LSM100A で `AT$SF=0102030405` など）から何回か送信し、Ctrl+C で止めます。
`capture.html` に各バーストのグラフ、`capture.json` に全サンプルが保存されます。

オプション: `--freq <hz>`（既定 923200000）、`--bw <code>`（SX126x の受信帯域コード。10 = 234.3kHz、
25 = 312kHz、9 = 467kHz）、`--rate <hz>`（100〜4000）、`--boost 0|1`。

## Sigfox アップリンクをリアルタイムで解読する

```bash
cd host && bun tools/sigfox-listen.ts /dev/ttyACM0 [--save session.json]
```

フレームごと（何回目の繰り返しか、CRC、RSSI、受信機の時計のずれ）と、繰り返しをまとめたメッセージごとに
1 行ずつ表示します。実機の RC3 デバイスで、1 バイト・5 バイトのペイロードとダウンリンク要求フラグが
CRC 込みで正しく解読できることを確認済みです。LSM100A の RC 発振器は約 1.3% 遅れていますが、自動で追従します。

## シリアルプロトコル (115200bps)

| 向き | 行 | 意味 |
|---|---|---|
| 出力 | `I fw=0.1.0 f=923200000 bw=10 sr=2000 boost=1` | 状態 |
| 出力 | `R<seq>:<64 × 2 桁 16 進>` | RSSI 64 サンプル。dBm = -値 / 2 |
| 入力 | `F<hz>` `W<code>` `S<hz>` `B<0\|1>` | 周波数・帯域・サンプルレート・LNA ブースト |
| 入力 | `?` / `X` | 状態表示 / リセット（IAP ブートローダへ） |
