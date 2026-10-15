# OpenNEK — Open Network Emulator Kit

[English](README.md) | **日本語**

ESP32 2 台と PC だけで動く、**LPWAN（Sigfox 風）ネットワークのエミュレータ**です。
デバイスから 12 バイトのアップリンクを送り、それを PC の Web UI で見たり、HTTP コールバックで
自分のアプリへ転送したり、8 バイトのダウンリンクを返したりできます。
クラウドの契約や電波の免許を用意しなくても、IoT アプリの開発とテストを手元で回せます。

```
┌────────────┐  ESP-NOW (2.4GHz)  ┌──────────────┐  USB シリアル  ┌───────────────────────┐  HTTP   ┌──────────┐
│ デバイス    │ ─ uplink ×3 ────▶ │ ゲートウェイ │ ─ JSON Lines ─▶│ opennek (Bun + Hono)  │ ──────▶ │ あなたの │
│ ESP32       │ ◀──── downlink ── │ ESP32 (USB)  │ ◀───────────── │ Web UI / SQLite       │ ◀────── │ アプリ   │
│ AT モデム   │                    └──────────────┘                └───────────────────────┘         └──────────┘
└────────────┘
```

| 部品 | 中身 |
|---|---|
| `host/` | ホスト。Bun + Hono + bun:sqlite。Web UI、コールバック、ダウンリンク、シミュレータ |
| `firmware/` | PlatformIO (Arduino)。`gateway` = USB ブリッジ、`device` = Sigfox 風 AT モデムエミュレータ |
| `docs/protocol.ja.md` | 無線フレーム・シリアル・コールバックの仕様 |
| `packaging/build-deb.sh` | Ubuntu 向けの `.deb` を作る |

> **Sigfox との関係**: 無線は ESP-NOW なので、**本物の Sigfox モジュールの電波は受信できません**。その代わり、AT コマンドとコールバックの形式を
> Sigfox に寄せてあるので、Sigfox 向けのアプリやツールをほぼそのまま試せます。

---

## とりあえず動かす（ハードウェア不要）

```bash
cd host
bun install
bun run sim
```

<http://127.0.0.1:8085/> を開くと、メッセージ画面にシミュレータの送信フォームが出ます。

## 実機で動かす

### 1. 用意するもの

- ESP32 の開発ボード 2 台（ESP32 DevKit や ESP32-S3 など。ESP-NOW が使えればなんでも可）
  - 1 台目: **ゲートウェイ**（PC に USB で挿したまま）
  - 2 台目: **デバイス**（モバイルバッテリーなどで給電）

### 2. ファームウェアを書き込む

```bash
cd firmware
pio run -e gateway -t upload --upload-port /dev/ttyUSB0
pio run -e device  -t upload --upload-port /dev/ttyUSB1
```

ESP32-S3 は `gateway-s3` / `device-s3` を使います。PlatformIO を使わない方法（ブラウザからの書き込みなど）は
[docs/flashing.ja.md](docs/flashing.ja.md) を見てください。

### 3. ホストを起動する

```bash
cd host && bun run start          # または .deb を入れて: opennek --open
```

ゲートウェイは自動で見つかります（右上が「接続中 · ch 1」になれば OK）。
シリアルポートを開く権限がないと言われたら `sudo usermod -aG dialout $USER` を実行し、再ログインしてください。

### 4. デバイスを登録する

デバイスの USB シリアル（115200 bps）で:

```
AT$ID      → 0A1B2C3D                          (デバイス ID)
AT$PAC     → 3F2A...（32 桁）                   (デバイス鍵)
AT$SF=0102030405
OK
```

1 回送るとデバイス一覧に「未登録」として出てきます。**編集** を押して鍵に `AT$PAC` の値を入れると、
認証が OK になり、ダウンリンクが使えるようになります。

---

## デバイス（AT モデムエミュレータ）

USB シリアル（115200 bps）と UART（**GPIO16 = RX、GPIO17 = TX、9600 bps**）の両方でコマンドを受け付けます。
既存の Sigfox AT 端末の UART をこの ESP32 につなぎ替えると、そのまま送信できます。
**BOOT ボタン**を押すと、起動してからの秒数（4 バイト）をデモとして送ります。

| コマンド | 応答 | 説明 |
|---|---|---|
| `AT` / `ATZ` | `OK` | |
| `AT$SF=<hex>` | `OK` | 0〜12 バイトを送る |
| `AT$SF=<hex>,1` | `OK` `RX=01 02 03 04 05 06 07 08` | ダウンリンクを要求する（来なければ `ERROR: downlink timeout`） |
| `AT$SB=<0\|1>[,1]` | `OK` | 1 ビット送信（1 バイト `00` / `01` として送る） |
| `AT$SO` | `OK` | OOB（keep-alive）フレーム |
| `AT$ID` / `AT$I=10` | `0A1B2C3D` | デバイス ID（MAC の下位 4 バイト） |
| `AT$PAC` / `AT$I=11` | 32 桁 16 進 | デバイス鍵（ホストに登録する） |
| `AT$CH?` / `AT$CH=<1-13>` | | ESP-NOW のチャンネル（ゲートウェイと揃える） |
| `AT$KEY=<32 hex>` | `OK` | 鍵を書き換える |
| `AT$DLW?` / `AT$DLW=<ms>` | | ダウンリンクを待つ時間（既定 8000 ms） |
| `AT$SEQ?` | | 次に使うシーケンス番号 |
| `AT$T?` / `AT$V?` | | 温度（0.1 ℃単位）/ 電圧（固定値 3300） |
| `AT$RC…` `AT$P=…` `AT+MODE…` `ATS3…` | `OK` | 実モデム用の設定コマンド。何もせず受け流す |

自分のスケッチから使うときは、ライブラリ `firmware/lib/OpenNEK` をそのまま使えます:

```cpp
#include <OpenNEK.h>
onek::Device dev;
void setup() { dev.begin(); }
void loop() {
  uint8_t data[] = {0x12, 0x34}, dl[8];
  if (dev.send(data, sizeof(data), dl) == onek::SendResult::Ok) { /* dl に 8 バイト */ }
  delay(60000);
}
```

## コールバック

**コールバック** タブで URL・ヘッダ・ボディを設定します。ボディには Sigfox カスタムコールバックと同じ変数を使えます
（`{device}` `{time}` `{data}` `{seqNumber}` `{ack}` `{rssi}` `{snr}` `{station}` `{customData#名前}` など。
一覧は [docs/protocol.ja.md](docs/protocol.ja.md)）。

```json
{"device":"{device}","time":{time},"data":"{data}","seqNumber":{seqNumber},"rssi":{rssi}}
```

- **ペイロード定義**（デバイスごと）: `temp::int:16:little-endian hum::uint:8 door:0:bool:7` のように書くと、
  UI にデコードした値が出て、コールバックでは `{customData#temp}` として使えます。
- **ダウンリンク**: デバイスの設定を「コールバックのレスポンス」にし、コールバックの
  「レスポンスからダウンリンクデータを取り出す」にチェックを入れます。レスポンスで
  `{"0A1B2C3D":{"downlinkData":"0102030405060708"}}` を返すと、それがデバイスへ届きます。
- **テスト送信** ボタンを押すと、直近のメッセージを使ってコールバックを試せます（ログには残りません）。

## WSL2 で使う場合

WSL には USB がそのままでは見えません。Windows 側で [usbipd-win](https://github.com/dorssel/usbipd-win) を使い、
ゲートウェイを WSL に渡してください（PowerShell を管理者で開いて実行）:

```powershell
usbipd list
usbipd bind --busid <BUSID>
usbipd attach --wsl --busid <BUSID>
```

Web UI は Windows のブラウザから <http://localhost:8085/> で開けます。

## .deb を作る

```bash
./packaging/build-deb.sh     # → dist/opennek_0.1.0_amd64.deb
sudo apt install ./dist/opennek_0.1.0_amd64.deb
opennek --open               # または: systemctl --user enable --now opennek
```

ホストは `bun build --compile` で単一バイナリにしてあるので、Bun や Node がなくても動きます。
ファームの統合イメージは `/usr/share/opennek/firmware/` に入ります。データは `~/.local/share/opennek/` に保存されます。

## 開発

```bash
(cd host && bun test && bun run typecheck)
(cd firmware && pio run)
(cd firmware/test && uvx --from ziglang python -m ziglang c++ -std=c++17 -I ../lib/OpenNEK/src \
  frame_native_test.cpp ../lib/OpenNEK/src/OpenNekFrame.cpp -o frame_test && ./frame_test)
```

ホストのテストには、シミュレータを使った E2E テスト（アップリンク → コールバック → ダウンリンク）と、
socat の仮想シリアルを使ったゲートウェイ通信のテストが含まれます。
ファームとホストのフレーム実装は、同じゴールデンベクタで一致を確認しています。

## ライセンス

MIT
