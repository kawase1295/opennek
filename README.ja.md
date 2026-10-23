# OpenNEK — Open Network Emulator Kit

[English](README.md) | **日本語**

PC で動く、**本物の Sigfox デバイス（RC3、日本）のためのローカルネットワーク**です。SDR ドングルの代わりに、
Seeed Studio の **XIAO ESP32S3 & Wio-SX1262 キット**を無線機として使います。
デバイスのアップリンクを Web UI で見たり、Sigfox 互換の HTTP コールバックで自分のアプリへ転送したり、
8 バイトのダウンリンクを返したりできます。
バックエンドの契約を用意しなくても、Sigfox アプリの開発とテストを手元で回せます。

```
┌──────────────┐  Sigfox RC3 (923.2 MHz)   ┌──────────────┐  USB シリアル ┌───────────────────────┐  HTTP   ┌──────────┐
│ Sigfox       │ ─ uplink ×3 ───────────▶ │ XIAO ESP32S3 │ ─ RSSI/CFO ─▶ │ opennek (Node + Hono) │ ──────▶ │ あなたの │
│ デバイス      │ ◀──── downlink (922 MHz) │ + Wio-SX1262 │ ◀──── TX ──── │ Web UI / SQLite       │ ◀────── │ アプリ   │
└──────────────┘                            └──────────────┘               └───────────────────────┘         └──────────┘
```

| 部品 | 中身 |
|---|---|
| `host/` | ホスト。Node.js + Hono + node:sqlite。Sigfox の解読器、Web UI、コールバック、ダウンリンク |
| `firmware/` | PlatformIO (Arduino) ファーム。`radio-xiao-sx1262` = 無線機。Wio-SX1262 の RSSI をホストへ流し、ダウンリンクを送信する |
| `docs/protocol.ja.md` | コールバックとダウンリンクの仕様 |

---

## 使い始める

1. XIAO ESP32S3 を Wio-SX1262 に挿し、キット付属のアンテナをつなぎます。
2. 無線機のファームウェアを書き込みます。
   ```bash
   cd firmware && pio run -e radio-xiao-sx1262 -t upload
   ```
3. キットのシリアルポートを指定してホストを起動します（Node.js 22.13 以降）。
   ```bash
   cd host
   npm install
   npm run build
   npm start -- --sigfox /dev/ttyACM0
   ```

<http://127.0.0.1:8085/> を開きます。キットは 923.2 MHz で受信します（`--sigfox-freq <hz>` で変更）。ポートは保存されます。**設定 → Sigfox 受信機 (RC3)** でも選べます。
Sigfox デバイスからメッセージを送ると（`AT$SF=0102`）、メッセージ一覧に表示されます。
UI は英語で表示されます。右上の言語メニューで「日本語」を選ぶと日本語になります（ブラウザに保存されます）。

シリアルポートの権限エラーが出たら、`sudo usermod -aG dialout $USER` を実行してログインし直してください。

データは `~/.local/share/opennek/` に保存されます。

### アップリンクの認証

アップリンクは、デバイスが署名に使った鍵でしか検証できません。Sigfox が書き込んだ秘密鍵は通常モジュールから読み出せないので、
本物のデバイスを検証するときは公開鍵（sigfox-ep-lib が定める `00112233445566778899AABBCCDDEEFF`）に切り替えます。
AX-SFJK などのモジュールでは次のとおりです。

```
ATS410=1    → 公開鍵: OpenNEK で検証できる。本番ネットワークでは使えない
ATS410=0    → 秘密鍵に戻す。本番ネットワークで使う前に必ず戻す
```

| デバイス | 表示 | コールバック |
|---|---|---|
| デバイス画面で鍵を登録済み | OK、または鍵不一致 | OK のときだけ実行 |
| 公開鍵モード、鍵は未登録 | 公開鍵 | 「鍵が未登録のデバイスもコールバックを実行する」がオンなら実行 |
| 秘密鍵、未登録 | 未検証 | 「鍵が未登録のデバイスもコールバックを実行する」がオンなら実行 |

公開鍵は誰でも署名に使えるので、フレームに矛盾がないことは分かっても、送信者までは保証しません。

### ダウンリンク

**設定 → 受信機からダウンリンクを送信する** をオンにすると、ダウンリンクを要求する（`AT$SF=…,1`）認証済みのアップリンクに、
デバイスの固定データかコールバックの `downlinkData` を返します。デバイス側には `RX=…` が出ます。

- キットはアップリンクのフレーム 1 の周波数を測り、フレーム 1 − 1 MHz（922.1〜922.3 MHz）で、フレーム 1 の終わりから 21 秒後、
  デバイスの RC3 受信ウィンドウ内に送信します。
- フレームは GFSK 600 bps です。認証値は登録済みの鍵か公開鍵で計算します。
- `RX=` の後にデバイスが送る受信確認フレームで、ダウンリンクが「受信確認」になります。

ホストが何を指示しても、ファームウェアは次の範囲を超える送信を拒否します。

| 制限 | 値 |
|---|---|
| 送信 | ホストが許可している間だけ。リセットのたびにオフ |
| 周波数 | 922.1〜922.3 MHz（922.2 MHz の単位チャネル） |
| 出力 | 10 dBm（10 mW） |
| キャリアセンス | 毎フレームの前に −80 dBm で 5 ms |
| 休止 / 1 時間の総送信時間 | 送信の間に 100 ms、1 時間あたり 360 秒 |

> **電波法**: ダウンリンクは 920 MHz 帯で電波を出します。キットが技適の条件（認証を受けたアンテナ・条件の範囲内）を
> 満たしているときだけオンにしてください。上の制限値は認証の条件と照らし合わせて確認してください。

## コールバック

**コールバック** タブで URL・ヘッダ・ボディを設定します。ボディには Sigfox カスタムコールバックと同じ変数を使えます
（`{device}` `{time}` `{data}` `{seqNumber}` `{ack}` `{rssi}` `{snr}` `{station}` `{customData#名前}` など。
一覧は [docs/protocol.ja.md](docs/protocol.ja.md)）。

```json
{"device":"{device}","time":{time},"data":"{data}","seqNumber":{seqNumber},"rssi":{rssi}}
```

- **ペイロード定義**（デバイスごと）: `temp::int:16:little-endian hum::uint:8 door:0:bool:7` のように書くと、
  UI にデコードした値が出て、コールバックでは `{customData#temp}` として使えます。
- **ダウンリンクのデータ**: デバイスの設定を「固定データ」（例: `{time}0102{rssi}`）か「コールバックのレスポンス」にします。
  後者ではコールバックの「レスポンスからダウンリンクデータを取り出す」にチェックを入れます。レスポンスで
  `{"0A1B2C3D":{"downlinkData":"0102030405060708"}}` を返すと、それがデバイスへ届きます。
- **テスト送信** ボタンを押すと、直近のメッセージを使ってコールバックを試せます（ログには残りません）。

## WSL2 で使う場合

WSL には USB がそのままでは見えません。Windows 側で [usbipd-win](https://github.com/dorssel/usbipd-win) を使い、
キットを WSL に渡してください（PowerShell を管理者で開いて実行）:

```powershell
usbipd list
usbipd bind --busid <BUSID>
usbipd attach --wsl --busid <BUSID>
```

Web UI は Windows のブラウザから <http://localhost:8085/> で開けます。

## 開発

```bash
(cd host && npm test && npm run typecheck)
(cd firmware && pio run)
(cd firmware/test && uvx --from ziglang python -m ziglang c++ -std=c++17 -O2 -I ../lib/SigfoxRx/src \
  sigfox_native_test.cpp ../lib/SigfoxRx/src/*.cpp -o sigfox_test && ./sigfox_test ../../host/test/fixtures)
```

ホストのテストは、本物の Sigfox デバイスの録音を通しで解読し（アップリンク → コールバック → ダウンリンク）、
socat の仮想シリアルで受信機とのやりとりも確認します。ファームの C++ 解読器も同じ録音で一致を確認しています。

## ライセンス

MIT
