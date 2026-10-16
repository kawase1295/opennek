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

## OpenNEK ホストで受信する

ホストは ESP-NOW ゲートウェイと並べて同じ解読器を動かせます。本物の Sigfox のメッセージが Web UI
（「Sigfox」の印付き）とデバイス一覧に表示され、Sigfox 互換のコールバックも実行されます。

```bash
cd host && bun src/index.ts --sigfox /dev/ttyACM0 [--sigfox-freq 923200000]
```

ポートは保存されます。設定 → Sigfox 受信機 でも選べます（「使わない」で無効）。自動検出の ESP-NOW
ゲートウェイは受信機のポートを使いません。現時点の制限: 受信専用なのでダウンリンク要求は「受信専用」と
表示します。

### アップリンクの認証

アップリンクの認証フィールドは、デバイスが署名に使った鍵でしか検証できません。

| デバイス | 表示 | コールバック |
|---|---|---|
| 秘密鍵をホストに登録済み（デバイス画面） | OK、合わなければ「鍵不一致」 | OK のときだけ実行する |
| 公開鍵モード（下記）で鍵が未登録 | 公開鍵 | 「鍵が未登録のデバイスもコールバックを実行する」が有効なら実行する |
| 秘密鍵で未登録（通常の状態） | 未検証 | 「鍵が未登録のデバイスもコールバックを実行する」が有効なら実行する |

公式の SNEK と同じく、本物のデバイスを検証するときは公開鍵（sigfox-ep-lib が定める
`00112233445566778899AABBCCDDEEFF`）に切り替えます。LSM100A / AX モジュールでは次のとおりです。

```
ATS410=1    → 公開鍵: OpenNEK で検証できる。本物のネットワーク向けではない
ATS410=0    → 秘密鍵に戻す。本物のネットワークで使う前に必ず戻す
```

公開鍵は誰でも署名できるので、フレームの整合性を示すだけで、送信者を保証しません。そのため未登録の
デバイスと同じ扱いにし、鍵が登録されていればそちらを優先します。

## 単体テスター (M5Stack Cardputer)

PC なしで Cardputer だけで解読し、テスト対象のデバイスを検査できます。

```bash
cd firmware && pio run -e tester-cardputer -t upload    # 中継用ファームを置き換えます。戻すときは bridge-s3 を書き込みます
```

LSM100A は上と同じく Grove の G1/G2 につなぎます。`L` を押してデバイスから 1 回送信すると登録されます
（`I` で ID を手入力することもできます）。登録したデバイスのメッセージごとに検査して、PASS / WARN / FAIL を表示します。

| 検査 | 結果 |
|---|---|
| 3 回の送信がすべて CRC 正常で届いたか | 1〜2 回欠けたら WARN |
| 連番が前回 + 1 か | 飛び・重複・逆戻りは WARN |
| ペイロードが期待値と同じか（`P` で設定、任意） | 違えば FAIL |
| 雑音レベルより 25dB 以上強いか | 弱ければ WARN |

ほかのキー: `D` 登録済みデバイス（`1`〜`8` で削除）、`H` 履歴、`M` 消音、`R` 集計のリセット、`` ` `` 戻る。
microSD カードを入れると、結果を `/opennek/tester.csv` に追記し、WARN/FAIL のたびに直前 30 秒の RSSI を
`/opennek/raw-*.rssi`（`host/test/fixtures` と同じ形式）に保存します。USB シリアルには結果ごとに JSON を 1 行、
10 秒ごとに動作状態を 1 行出力します。

このテスターは受信だけを行います。LSM100A のファームは送信せず、Cardputer のファームも Wi-Fi や Bluetooth を
起動しません。登録していないデバイスのメッセージは表示も記録もしません（学習モードで、次に受信したデバイスの
ID を登録候補として示すときだけ例外です）。

解読処理は `host/src/sigfox` を C++ に移植したもの（`firmware/lib/SigfoxRx`）で、`firmware/test/sigfox_native_test.cpp`
で録音済みデータに対して同じ結果になることを確認しています。

## シリアルプロトコル (115200bps)

| 向き | 行 | 意味 |
|---|---|---|
| 出力 | `I fw=0.1.0 f=923200000 bw=10 sr=2000 boost=1` | 状態 |
| 出力 | `R<seq>:<64 × 2 桁 16 進>` | RSSI 64 サンプル。dBm = -値 / 2 |
| 入力 | `F<hz>` `W<code>` `S<hz>` `B<0\|1>` | 周波数・帯域・サンプルレート・LNA ブースト |
| 入力 | `?` / `X` | 状態表示 / リセット（IAP ブートローダへ） |
