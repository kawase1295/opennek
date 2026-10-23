# OpenNEK — Open Network Emulator Kit

[English](README.md) | **日本語**

PC と USB の受信機だけで動く、**本物の Sigfox デバイス（RC3、日本）のためのローカルネットワーク**です。
デバイスのアップリンクを Web UI で見たり、Sigfox 互換の HTTP コールバックで自分のアプリへ転送したり、
8 バイトのダウンリンクを返したりできます。
バックエンドの契約を用意しなくても、Sigfox アプリの開発とテストを手元で回せます。

```
┌──────────────┐  Sigfox RC3 (923.2 MHz)   ┌──────────┐  USB シリアル ┌───────────────────────┐  HTTP   ┌──────────┐
│ Sigfox       │ ─ uplink ×3 ───────────▶ │ 受信機   │ ───────────▶ │ opennek (Bun + Hono)  │ ──────▶ │ あなたの │
│ デバイス      │ ◀──── downlink (922 MHz) │          │ ◀─────────── │ Web UI / SQLite       │ ◀────── │ アプリ   │
└──────────────┘                            └──────────┘              └───────────────────────┘         └──────────┘
```

| 部品 | 中身 |
|---|---|
| `host/` | ホスト。Bun + Hono + bun:sqlite。Sigfox の解読器、Web UI、コールバック、ダウンリンク |
| `firmware/` | 受信機の ESP32-S3 側の PlatformIO (Arduino) ファーム。USB ブリッジとスタンドアロンのテスター |
| `docs/protocol.ja.md` | コールバックとダウンリンクの仕様 |
| `packaging/build-deb.sh` | Ubuntu 向けの `.deb` を作る |

---

## 使い始める

```bash
cd host
bun install
bun run start -- --sigfox /dev/ttyACM0    # 受信機のシリアルポート
```

<http://127.0.0.1:8085/> を開きます。ポートは保存されます。**設定 → Sigfox 受信機 (RC3)** でも選べます。
Sigfox デバイスからメッセージを送ると（`AT$SF=0102`）、メッセージ一覧に表示されます。
UI は英語で表示されます。右上の言語メニューで「日本語」を選ぶと日本語になります（ブラウザに保存されます）。

シリアルポートの権限エラーが出たら、`sudo usermod -aG dialout $USER` を実行してログインし直してください。

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
デバイスの受信ウィンドウ内で受信機から応答します。デバイス側には `RX=…` が出ます。
その後にデバイスが送る受信確認フレームで、ダウンリンクが「受信確認」になります。

> **電波法**: ダウンリンクは 920 MHz 帯で電波を出します。受信機が技適の条件を満たし、その範囲内で使うときだけオンにしてください。

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
受信機を WSL に渡してください（PowerShell を管理者で開いて実行）:

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
opennek --open --sigfox /dev/ttyACM0     # または: systemctl --user enable --now opennek
```

ホストは `bun build --compile` で単一バイナリにしてあるので、Bun や Node がなくても動きます。
データは `~/.local/share/opennek/` に保存されます。

## 開発

```bash
(cd host && bun test && bun run typecheck)
(cd firmware && pio run)
(cd firmware/test && uvx --from ziglang python -m ziglang c++ -std=c++17 -O2 -I ../lib/SigfoxRx/src \
  sigfox_native_test.cpp ../lib/SigfoxRx/src/*.cpp -o sigfox_test && ./sigfox_test ../../host/test/fixtures)
```

ホストのテストは、本物の Sigfox デバイスの録音を通しで解読し（アップリンク → コールバック → ダウンリンク）、
socat の仮想シリアルで受信機とのやりとりも確認します。ファームの C++ 解読器も同じ録音で一致を確認しています。

## ライセンス

MIT
