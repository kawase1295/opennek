import { mkdirSync } from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'
import { parseArgs } from 'node:util'
import { Settings, Store } from './db'
import { Engine } from './engine'
import type { Gateway } from './gateway/gateway'
import { SerialGateway } from './gateway/serial'
import { SigfoxRadio } from './gateway/sigfox'
import { SimGateway } from './gateway/sim'
import { createApp } from './server'
import { VERSION } from './version'

const HELP = `OpenNEK ${VERSION} — ESP32 で作るオープンな LPWAN ネットワークエミュレータ

使い方: opennek [オプション]

  --port <n>        Web UI / API のポート (既定 8085)
  --host <addr>     待ち受けアドレス (既定 127.0.0.1)
  --serial <path>   ゲートウェイのシリアルポート (既定: 設定値。初期値は auto = 自動検出)
  --sigfox <path>   本物の Sigfox (RC3) を受ける RSSI ストリーマのポート。off で無効 (既定: 設定値)
  --sigfox-freq <hz>  Sigfox の受信周波数 (既定 923200000)
  --sim             ハードウェア無しのシミュレータで起動する
  --data <dir>      データ保存先 (既定 $XDG_DATA_HOME/opennek)
  --open            起動後にブラウザで UI を開く
  -v, --version     バージョンを表示
  -h, --help        このヘルプ
`

const { values: args } = parseArgs({
  options: {
    port: { type: 'string', default: '8085' },
    host: { type: 'string', default: '127.0.0.1' },
    serial: { type: 'string' },
    sigfox: { type: 'string' },
    'sigfox-freq': { type: 'string' },
    sim: { type: 'boolean', default: false },
    data: { type: 'string' },
    open: { type: 'boolean', default: false },
    version: { type: 'boolean', short: 'v', default: false },
    help: { type: 'boolean', short: 'h', default: false },
  },
})

if (args.help) {
  console.log(HELP)
  process.exit(0)
}
if (args.version) {
  console.log(VERSION)
  process.exit(0)
}

const dataDir = args.data ?? join(process.env.XDG_DATA_HOME || join(homedir(), '.local/share'), 'opennek')
mkdirSync(dataDir, { recursive: true })
const store = new Store(join(dataDir, args.sim ? 'opennek-sim.db' : 'opennek.db'))
if (args.serial) store.saveSettings({ ...store.getSettings(), serialPort: args.serial })
if (args.sigfox) store.saveSettings({ ...store.getSettings(), sigfoxPort: args.sigfox === 'off' ? '' : args.sigfox })
if (args['sigfox-freq']) store.saveSettings(Settings.parse({ ...store.getSettings(), sigfoxFreq: Number(args['sigfox-freq']) }))

const sigfoxSettings = () => ({ port: store.getSettings().sigfoxPort, freq: store.getSettings().sigfoxFreq })
const gateway: Gateway = args.sim ? new SimGateway() : new SerialGateway(() => store.getSettings().serialPort, () => [store.getSettings().sigfoxPort])
const sigfox = new SigfoxRadio(sigfoxSettings)
const engine = new Engine(store, gateway, sigfox)
const ts = () => new Date().toLocaleTimeString()
let lastRadioLine = ''
engine.on((e) => {
  if (e.type === 'console') console.log(`[${ts()}] ${e.line}`)
  if (e.type === 'gateway') {
    const s = e.status
    console.log(`[${ts()}] gateway: ${s.state}${s.port ? ` ${s.port}` : ''}${s.fw ? ` fw=${s.fw} ch=${s.channel}` : ''}${s.error ? ` (${s.error})` : ''}`)
    // 起動時・再接続時にホストの設定チャンネルへ合わせる
    const want = store.getSettings().channel
    if (s.state === 'connected' && s.channel !== want) gateway.setChannel(want)
  }
  if (e.type === 'radio') {
    const s = e.status
    // フレーム数の更新ごとには出さない
    const line = `sigfox: ${s.state}${s.port ? ` ${s.port}` : ''}${s.fw ? ` fw=${s.fw}` : ''}${s.freq ? ` ${(s.freq / 1e6).toFixed(3)} MHz` : ''}${s.error ? ` (${s.error})` : ''}`
    if (line !== lastRadioLine) console.log(`[${ts()}] ${line}`)
    lastRadioLine = line
  }
  if (e.type === 'message') {
    // メッセージは受信時と、ダウンリンクの結果が決まったときの 2 回通知される
    const m = e.message
    if (m.downlinkStatus === 'none' || m.downlinkStatus === 'pending') {
      console.log(`[${ts()}] uplink${m.radio === 'sigfox' ? ' (sigfox)' : ''} ${m.deviceId} seq=${m.seq} data=${m.data || '-'} rssi=${m.rssi} frames=${m.nbFrames} auth=${m.auth}${m.ack ? ' (downlink requested)' : ''}`)
    } else {
      console.log(`[${ts()}] downlink ${m.deviceId} seq=${m.seq} ${m.downlinkStatus}${m.downlink ? ` data=${m.downlink}` : ''}`)
    }
  }
})

const app = createApp(engine)
const server = Bun.serve({ port: Number(args.port), hostname: args.host, fetch: app.fetch, idleTimeout: 0 })
const url = `http://${args.host === '0.0.0.0' ? 'localhost' : args.host}:${server.port}/`
console.log(`OpenNEK ${VERSION} — ${url}${args.sim ? ' (simulator)' : ''}  data: ${dataDir}`)
gateway.start()
sigfox.start()

if (args.open) {
  Bun.spawn(['xdg-open', url], { stdout: 'ignore', stderr: 'ignore' }).exited.catch(() => {})
}

const shutdown = async () => {
  await Promise.all([gateway.stop(), sigfox.stop()])
  server.stop(true)
  store.db.close()
  process.exit(0)
}
process.on('SIGINT', shutdown)
process.on('SIGTERM', shutdown)
