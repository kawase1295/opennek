// UI の表示言語。設定 (GET /api/settings の language) に保存し、既定は英語
// en: app.js が組み立てる文字列だけ (静的な文字列の英語は index.html に直接書いてある)
// ja: index.html の data-i18n* キーと en のキーのすべて
const en = {
  'radio.hostDown': 'Cannot reach the host',
  'radio.off': 'Off',
  'radio.connected': 'Receiving',
  'radio.connecting': 'Waiting for reply',
  'radio.disconnected': 'Disconnected',
  'radio.offHint': 'Off (choose a receiver port in the settings below to enable it)',
  'radio.framesValue': '{frames} ({bad} CRC errors)',
  'kv.state': 'State',
  'kv.port': 'Port',
  'kv.firmware': 'Firmware',
  'kv.freq': 'Frequency',
  'kv.rate': 'Sample rate',
  'kv.frames': 'Frames received',
  'kv.lastFrame': 'Last received',
  'kv.lostLines': 'Lost lines',
  'kv.time': 'Time',
  'kv.device': 'Device',
  'kv.data': 'Data',
  'kv.decoded': 'Decoded',
  'kv.auth': 'Auth',
  'kv.downlink': 'Downlink',
  'kv.noDownlink': 'Not requested',
  'dl.pending': 'Pending',
  'dl.sent': 'Sent',
  'dl.acked': 'Acknowledged',
  'dl.failed': 'Failed',
  'dl.nodata': 'No data',
  'dl.unsupported': 'Not sent',
  'auth.bad': 'Key mismatch',
  'auth.public': 'Public key',
  'auth.unverified': 'Unverified',
  'msg.confirmClearDevice': 'Delete the messages of {device}?',
  'msg.confirmClearAll': 'Delete all messages?',
  'msg.noLogs': 'No callbacks were run.',
  'msg.allDevices': 'All devices',
  'dlMode.none': "Don't send",
  'dlMode.static': 'Static',
  'dlMode.callback': 'Callback',
  'dev.registered': 'Registered',
  'dev.unregistered': 'Not registered',
  'dev.confirmDelete': 'Delete {device}? (Its messages are kept.)',
  'common.edit': 'Edit',
  'common.delete': 'Delete',
  'common.saved': 'Saved',
  'cb.untitled': 'Callback #{id}',
  'cb.enabled': 'Enabled',
  'cb.disabled': 'Disabled',
  'cb.downlink': 'Downlink',
  'cb.target': 'Devices: {devices}',
  'cb.allDevices': 'all',
  'cb.test': 'Send test',
  'cb.confirmDelete': 'Delete this callback?',
  'cb.confirmTest': 'Send a test request to {method} {url}? The server receives it like a real uplink.',
  'cb.testOk': 'OK: HTTP {status} ({ms} ms)',
  'cb.testFailed': 'Failed: {error}',
  'set.notUsed': "Don't use",
}

const ja = {
  // index.html
  'nav.messages': 'メッセージ',
  'nav.devices': 'デバイス',
  'nav.callbacks': 'コールバック',
  'nav.log': 'ログ',
  'nav.settings': '設定',
  'pill.radio': 'Sigfox 受信機',
  'msg.clear': 'クリア',
  'msg.time': '時刻',
  'msg.device': 'デバイス',
  'msg.data': 'データ',
  'msg.frames': 'フレーム',
  'msg.auth': '認証',
  'msg.downlink': 'ダウンリンク',
  'msg.empty': 'まだメッセージがありません。デバイスから送信するとここにリアルタイムで表示されます。',
  'msg.more': 'さらに読み込む',
  'dev.intro': '受信したデバイスは自動で一覧に追加されます。デバイスの鍵を登録すると、アップリンクの検証とダウンリンクの送信ができます。',
  'dev.name': '名前',
  'dev.key': '鍵',
  'dev.downlink': 'ダウンリンク',
  'dev.lastSeen': '最終受信',
  'dev.lastSeq': '最終 Seq',
  'dev.empty': 'デバイスがありません。',
  'common.add': '追加',
  'common.save': '保存',
  'common.cancel': 'キャンセル',
  'common.close': '閉じる',
  'cb.intro': 'アップリンクを受けるたびに HTTP リクエストを送ります。変数は Sigfox のカスタムコールバックと互換です。',
  'cb.empty': 'コールバックがありません。',
  'cb.log': '実行ログ',
  'log.time': '時刻',
  'log.callback': 'コールバック',
  'log.message': 'メッセージ',
  'log.result': '結果',
  'log.duration': '時間',
  'log.response': 'レスポンス',
  'hostLog.empty': 'ログはまだありません。ホストのログ (アップリンク、ダウンリンク、受信機の状態、エラー) がリアルタイムに表示されます。',
  'set.radio': 'Sigfox 受信機',
  'set.host': 'ホスト',
  'set.timeout': 'ダウンリンク / コールバックのタイムアウト (ms)',
  'set.acceptUnknown': '鍵が未登録のデバイスもコールバックを実行する',
  'set.sigfox': 'Sigfox 受信機 (RC3)',
  'set.sigfoxPort': '受信機のポート',
  'set.sigfoxPortHint': 'RSSI ストリーマ (firmware-lsm100a) を書き込んだ受信機。本物の Sigfox デバイスのアップリンクを受信します。',
  'set.sigfoxFreq': '受信周波数 (Hz)',
  'set.sigfoxDownlink': '受信機からダウンリンクを送信する (電波を出します)',
  'set.sigfoxDownlinkHint':
    'firmware-lsm100a 0.3.0 以降。922.2 MHz 帯・10 mW・キャリアセンス付きで、デバイスの受信ウィンドウに 1 回だけ送信します。送信する機器が技適 (または無線局免許) の条件を満たしているときだけ有効にしてください。',
  'devForm.title': 'デバイス',
  'devForm.id': 'ID (8 桁 16 進)',
  'devForm.name': '名前',
  'devForm.key': '鍵 (32 桁 16 進)',
  'devForm.keyPlaceholder': '未登録',
  'devForm.payload': 'ペイロード定義',
  'devForm.payloadHint': '名前:位置:型:引数 を空白区切りで。型は uint / int / float / bool / char。デコード結果は {customData#名前} で使えます。',
  'devForm.downlink': 'ダウンリンク',
  'devForm.dlNone': '送らない',
  'devForm.dlStatic': '固定データ',
  'devForm.dlCallback': 'コールバックのレスポンス',
  'devForm.static': '固定データ (8 バイト 16 進)',
  'devForm.staticHint':
    '変数 <code>{time}</code> (UNIX 秒, 4 バイト) <code>{tapid}</code> (station, 4 バイト) <code>{rssi}</code> (符号付き, 2 バイト) を埋め込めます。合計 8 バイトにしてください。',
  'cbForm.title': 'コールバック',
  'cbForm.name': '名前',
  'cbForm.method': 'メソッド',
  'cbForm.devices': '対象デバイス',
  'cbForm.devicesPlaceholder': '空欄ですべて。カンマ区切りで ID を指定',
  'cbForm.headers': 'ヘッダ',
  'cbForm.body': 'ボディ',
  'cbForm.vars': '変数:',
  'cbForm.downlink': 'レスポンスからダウンリンクデータを取り出す (<code>{"&lt;device&gt;":{"downlinkData":"16進"}}</code>)',
  'cbForm.enabled': '有効',
  'detail.title': 'メッセージ詳細',
  'detail.callbacks': 'コールバック',
  // app.js
  'radio.hostDown': 'ホストに接続できません',
  'radio.off': '無効',
  'radio.connected': '受信中',
  'radio.connecting': '応答待ち',
  'radio.disconnected': '未接続',
  'radio.offHint': '無効 (下の設定で受信機のポートを選ぶと有効になります)',
  'radio.framesValue': '{frames} (CRC エラー {bad})',
  'kv.state': '状態',
  'kv.port': 'ポート',
  'kv.firmware': 'ファームウェア',
  'kv.freq': '周波数',
  'kv.rate': 'サンプリング',
  'kv.frames': '受信フレーム',
  'kv.lastFrame': '最終受信',
  'kv.lostLines': '欠落行',
  'kv.time': '時刻',
  'kv.device': 'デバイス',
  'kv.data': 'データ',
  'kv.decoded': 'デコード',
  'kv.auth': '認証',
  'kv.downlink': 'ダウンリンク',
  'kv.noDownlink': '要求なし',
  'dl.pending': '待機中',
  'dl.sent': '送信済',
  'dl.acked': '受信確認',
  'dl.failed': '失敗',
  'dl.nodata': 'データなし',
  'dl.unsupported': '送信なし',
  'auth.bad': '鍵不一致',
  'auth.public': '公開鍵',
  'auth.unverified': '未検証',
  'msg.confirmClearDevice': '{device} のメッセージを削除しますか?',
  'msg.confirmClearAll': 'すべてのメッセージを削除しますか?',
  'msg.noLogs': '実行されたコールバックはありません。',
  'msg.allDevices': 'すべてのデバイス',
  'dlMode.none': '送らない',
  'dlMode.static': '固定',
  'dlMode.callback': 'コールバック',
  'dev.registered': '登録済',
  'dev.unregistered': '未登録',
  'dev.confirmDelete': '{device} を削除しますか? (メッセージは残ります)',
  'common.edit': '編集',
  'common.delete': '削除',
  'common.saved': '保存しました',
  'cb.untitled': 'コールバック #{id}',
  'cb.enabled': '有効',
  'cb.disabled': '無効',
  'cb.downlink': 'ダウンリンク',
  'cb.target': '対象: {devices}',
  'cb.allDevices': 'すべて',
  'cb.test': 'テスト送信',
  'cb.confirmDelete': 'このコールバックを削除しますか?',
  'cb.confirmTest': '{method} {url} にテストのリクエストを送りますか? 送信先には本物のアップリンクと同じように届きます。',
  'cb.testOk': '成功: HTTP {status} ({ms} ms)',
  'cb.testFailed': '失敗: {error}',
  'set.notUsed': '使わない',
}

export const STRINGS = { en, ja }
export const LANGUAGES = { en: 'English', ja: '日本語' }

// 起動時に app.js が設定から決めるまでは英語
export let lang = 'en'

export function setLang(next) {
  lang = Object.hasOwn(STRINGS, next ?? '') ? next : 'en'
}

// 設定の言語、旧版が localStorage に覚えた言語 (legacy)、ブラウザの言語 (navigator.languages) から表示言語を決める
// 設定で選ばれていなければ (null) ブラウザの言語に従う。legacy があればそれを設定へ移す (migrate)
// 言語を未選択にできる前の版は既定の "en" を保存していたので、"en" でも legacy を優先する
export function chooseLang(saved, legacy, browser) {
  const known = (code) => Object.hasOwn(STRINGS, code ?? '')
  if ((saved === 'en' || !known(saved)) && legacy !== 'en' && known(legacy)) return { lang: legacy, migrate: true }
  if (known(saved)) return { lang: saved, migrate: false }
  const fromBrowser = browser.map((tag) => tag.split('-')[0].toLowerCase()).find(known)
  return { lang: fromBrowser ?? 'en', migrate: false }
}

// 旧版の localStorage の言語を取り出して消す。ポートが変わると読めなくなるので設定へ移した
const LEGACY_KEY = 'opennek.lang'
export function takeLegacyLang() {
  try {
    const v = localStorage.getItem(LEGACY_KEY)
    localStorage.removeItem(LEGACY_KEY)
    return v
  } catch {
    return null
  }
}

// {name} を vars で置換する。vars が無ければそのまま (ヒント文の {time} などを残すため)
export function t(key, vars) {
  const s = STRINGS[lang][key] ?? en[key] ?? key
  return vars ? s.replace(/\{(\w+)\}/g, (m, k) => (k in vars ? String(vars[k]) : m)) : s
}

// 英語は index.html に書いてあるので、それ以外の言語のときだけ差し替える
export function translatePage(root = document) {
  document.documentElement.lang = lang
  if (lang === 'en') return
  const dict = STRINGS[lang]
  for (const el of root.querySelectorAll('[data-i18n]')) if (dict[el.dataset.i18n]) el.textContent = dict[el.dataset.i18n]
  for (const el of root.querySelectorAll('[data-i18n-html]')) if (dict[el.dataset.i18nHtml]) el.innerHTML = dict[el.dataset.i18nHtml]
  for (const el of root.querySelectorAll('[data-i18n-placeholder]')) if (dict[el.dataset.i18nPlaceholder]) el.placeholder = dict[el.dataset.i18nPlaceholder]
  for (const el of root.querySelectorAll('[data-i18n-title]')) if (dict[el.dataset.i18nTitle]) el.title = dict[el.dataset.i18nTitle]
}
