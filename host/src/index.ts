import { mkdirSync } from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'
import { parseArgs } from 'node:util'
import { Settings, Store } from './db'
import { Engine } from './engine'
import { SigfoxRadio } from './radio'
import { createApp } from './server'
import { VERSION } from './version'

const HELP = `OpenNEK ${VERSION} — a local network for real Sigfox devices

Usage: opennek [options]

  --port <n>          Port of the web UI / API (default 8085)
  --host <addr>       Address to listen on (default 127.0.0.1)
  --sigfox <path>     Port of the RSSI streamer that receives real Sigfox (RC3); off to disable (default: the saved setting)
  --sigfox-freq <hz>  Sigfox receive frequency (default 923200000)
  --data <dir>        Data directory (default $XDG_DATA_HOME/opennek)
  --open              Open the UI in a browser after starting
  -v, --version       Print the version
  -h, --help          Print this help
`

const { values: args } = parseArgs({
  options: {
    port: { type: 'string', default: '8085' },
    host: { type: 'string', default: '127.0.0.1' },
    sigfox: { type: 'string' },
    'sigfox-freq': { type: 'string' },
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
const store = new Store(join(dataDir, 'opennek.db'))
if (args.sigfox) store.saveSettings({ ...store.getSettings(), sigfoxPort: args.sigfox === 'off' ? '' : args.sigfox })
if (args['sigfox-freq']) store.saveSettings(Settings.parse({ ...store.getSettings(), sigfoxFreq: Number(args['sigfox-freq']) }))

// Following frame 1 of each message (for its downlink frequency) costs reception of other devices' first frames,
// so it is only on while Sigfox downlinks are enabled
const sigfoxSettings = () => {
  const s = store.getSettings()
  return { port: s.sigfoxPort, freq: s.sigfoxFreq, follow: s.sigfoxDownlink }
}
const sigfox = new SigfoxRadio(sigfoxSettings)
// The receiver also transmits the downlinks (firmware-lsm100a 0.3.0), while the sigfoxDownlink setting is on
const engine = new Engine(store, sigfox, sigfox)
const ts = () => new Date().toLocaleTimeString()
let lastRadioLine = ''
engine.on((e) => {
  if (e.type === 'console') console.log(`[${ts()}] ${e.line}`)
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
      console.log(`[${ts()}] uplink ${m.deviceId} seq=${m.seq} data=${m.data || '-'} rssi=${m.rssi} frames=${m.nbFrames} auth=${m.auth}${m.ack ? ' (downlink requested)' : ''}`)
    } else {
      console.log(`[${ts()}] downlink ${m.deviceId} seq=${m.seq} ${m.downlinkStatus}${m.downlink ? ` data=${m.downlink}` : ''}`)
    }
  }
})

const app = createApp(engine)
const server = Bun.serve({ port: Number(args.port), hostname: args.host, fetch: app.fetch, idleTimeout: 0 })
const url = `http://${args.host === '0.0.0.0' ? 'localhost' : args.host}:${server.port}/`
console.log(`OpenNEK ${VERSION} — ${url}  data: ${dataDir}`)
sigfox.start()

if (args.open) {
  Bun.spawn(['xdg-open', url], { stdout: 'ignore', stderr: 'ignore' }).exited.catch(() => {})
}

const shutdown = async () => {
  await sigfox.stop()
  server.stop(true)
  store.db.close()
  process.exit(0)
}
process.on('SIGINT', shutdown)
process.on('SIGTERM', shutdown)
