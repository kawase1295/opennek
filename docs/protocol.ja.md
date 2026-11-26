# OpenNEK コールバック仕様

[English](protocol.md) | **日本語**

ホストは本物の Sigfox デバイスのアップリンクを受信機から受け取り、認証して HTTP コールバックを実行し、
受信機からダウンリンクを返す。

```
[Sigfox デバイス] --アップリンク (RC3)--> [受信機] --USB シリアル--> [ホスト] --HTTP コールバック--> [アプリ]
       ^                                                                   |
       +------------------------- ダウンリンク (8 バイト) -----------------+
```

---

## HTTP コールバック

Sigfox バックエンドのカスタムコールバックと互換の変数を使える。

| 変数 | 内容 |
|---|---|
| `{device}` | デバイス ID (8 桁 16 進) |
| `{deviceName}` | ホストに登録した名前 |
| `{time}` | 受信時刻 (UNIX 秒) |
| `{data}` | ペイロード (16 進小文字) |
| `{seqNumber}` | シーケンス番号 |
| `{ack}` | ダウンリンク要求 `true` / `false` |
| `{rssi}` | 最良 RSSI (dBm) |
| `{snr}` | 常に `0` (測っていない) |
| `{station}` | 常に `0000` (基地局が無いため) |
| `{nbFrames}` | 受信できた repeat 数 |
| `{oob}` | 制御フレーム (keep-alive・ダウンリンクの受信確認) なら `true` |
| `{customData#名前}` | カスタムペイロード設定でデコードした値 |

### カスタムペイロード設定

`名前:位置:型:引数` を空白区切りで並べる。`位置` を空にする (`名前::型`) と前のフィールドの直後から読む。

| 型 | 書式 | 例 |
|---|---|---|
| uint / int | `uint:ビット数[:little-endian]` | `temp::int:16:little-endian` |
| float | `float:32` または `float:64`（`:little-endian` 可） | `v::float:32` |
| bool | `bool:ビット位置` (0=LSB) — 位置のバイトを読み、前進しない | `flag:0:bool:7` |
| char | `char:バイト数` | `name::char:4` |

### ダウンリンク

デバイスごとに `none` / `static` (固定 8 バイト) / `callback` を選ぶ。

`static` のデータには 16 進の間に次の変数を埋め込める (Sigfox 系エミュレータ互換)。16 進は大文字小文字を問わず
バイト単位 (偶数桁) で書き、変数名は小文字。展開後がちょうど 8 バイトになる必要があり、デバイス保存時に検査する
(例: `{time}0102{rssi}`)。

| 変数 | バイト数 | 内容 |
|---|---|---|
| `{time}` | 4 | アップリンクの受信時刻 (UNIX 秒、ビッグエンディアン) |
| `{tapid}` | 4 | `{station}` を 16 進として読み、8 桁に左ゼロ埋めした値 (`0000` → `00000000`)。16 進でなければ `00000000` |
| `{rssi}` | 2 | アップリンクの最良 RSSI (dBm) を int16 の 2 の補数で (ビッグエンディアン、`-100` → `ff9c`) |

`callback` の場合、
「ダウンリンク用」にしたコールバックのレスポンスから次のどちらかの形式で 8 バイトを取り出す。

```json
{"0A1B2C3D": {"downlinkData": "0102030405060708"}}
{"downlinkData": "0102030405060708"}
```

レスポンスが `204 No Content` ならダウンリンクを送らない。

### SERVICE ACKNOWLEDGE

SERVICE ACKNOWLEDGE のコールバック (URL チャンネル) は、ダウンリンクを要求したアップリンク (`{ack}` が `true`) ごとに、
ホストがそのダウンリンクの結果を決めたあとで 1 回、対象デバイスについて送る。送り方 (Url pattern・メソッド・ヘッダー・本文・
Content type・SNI) と認証の確認は DATA と同じで、コールバックの履歴に残る。変数は Sigfox Backend の ACKNOWLEDGE
コールバックと同じものだけ。

| 変数 | 内容 |
|---|---|
| `{device}` | デバイス ID (8 桁 16 進) |
| `{time}` | アップリンクの受信時刻 (UNIX 秒) |
| `{infoCode}` | ダウンリンクの結果 (下の表) |
| `{infoMessage}` | 同じ内容の文 |
| `{downlinkAck}` | ダウンリンクを送信できたら `true` |
| `{downlinkOverusage}` | 常に `false` (OpenNEK にはダウンリンクの上限がない) |

ホストは、ダウンリンクのデータ、署名する鍵、送信機、タイミング、電波の順に結果を決める。そのため、ダウンリンクのデータが
ないアップリンクは、受信だけのホストでも `1` になる。

| 結果 | `{infoCode}` | `{infoMessage}` | `{downlinkAck}` | ダウンリンクの状態 |
|---|---|---|---|---|
| デバイスの受信ウィンドウ内に送信した | `0` | `Acked: the downlink was transmitted` | `true` | `sent` |
| ダウンリンクのデータがない: `none`、固定データが空、またはダウンリンク用の BIDIR が `downlinkData` を返さなかった (`204`・HTTP エラー・タイムアウトを含む) | `1` | `No answer: no downlink data was given` | `false` | `nodata` |
| `downlinkData` が 8 バイトの 16 進ではない | `2` | `Invalid payload: the downlink data is not 8 bytes of hex` | `false` | `nodata` |
| `callback` なのに、デバイスに当てはまる有効な DATA BIDIR がダウンリンク用に選ばれていない | `8` | `Invalid configuration: the device takes its downlink from a callback, but no DATA BIDIR callback is chosen for downlink` | `false` | `nodata` |
| 署名する鍵がない: アップリンクが認証されていない | `4` | `Network error: not transmitted, the uplink is not authenticated` | `false` | `nodata` |
| **受信機からダウンリンクを送信する** がオフ | `4` | `Network error: not transmitted, OpenNEK is receiving only` | `false` | `unsupported` |
| フレーム 1 を受けていない、またはその周波数が分からない | `4` | `Network error: not transmitted, the time or frequency of frame 1 is unknown` | `false` | `unsupported` |
| ダウンリンクの用意ができたときには、デバイスの受信ウィンドウが閉じていた | `4` | `Network error: not transmitted, the device's receive window has closed` | `false` | `failed` |
| 受信機が送信しなかった (RTL-SDR、ウィンドウが閉じるまでチャネルが埋まっていた、など) | `4` | `Network error: not transmitted, the transmitter did not send it` | `false` | `failed` |

コードは Backend と同じ: `0` ACKED、`1` NO_ANSWER、`2` INVALID_PAYLOAD、`4` NETWORK_ERROR、`8` INVALID_CONFIGURATION。
`3` OVERRUN_ERROR (1 日の上限がない)、`5` PENDING、`6` NO_DOWNLINK_CONTRACT、`7` TOO_MANY_REQUESTS は送らない。
正しいダウンリンクのデータが与えられたら、送れなかったときもメッセージに保存する。

Backend と同じく、このコールバックは基地局が何をしたかを 1 回だけ知らせる。あとでデバイスがダウンリンクの受信確認を送っても
(状態 `acked`)、2 回目の ACKNOWLEDGE コールバックは送らない。**テスト送信** では、直近のメッセージのダウンリンクを送信できた
ものとして送る (`{infoCode}` は `0`)。

## CSV エクスポート

**メッセージ** タブの **CSV** ボタンは `GET /api/messages.csv` をダウンロードする。ファイルには、ホストが保存している
すべてのメッセージが古い順に入る。ホストは最新の 10,000 件のメッセージを残す。同じタブのデバイス一覧でデバイスを
選ぶと、ファイルにはそのデバイスのメッセージだけが入る。
