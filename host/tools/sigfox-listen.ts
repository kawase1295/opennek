// Live Sigfox uplink receiver: reads the OpenNEK LSM100A RSSI streamer and prints decoded messages.
//   bun tools/sigfox-listen.ts /dev/ttyACM0 [--freq 923200000] [--save capture.json]
import { writeFileSync } from 'node:fs'
import { parseArgs } from 'node:util'
import { SigfoxReceiver } from '../src/sigfox/receiver'
import { openPort } from './serial'

const { values: args, positionals } = parseArgs({
  allowPositionals: true,
  options: { freq: { type: 'string' }, save: { type: 'string' } },
})
const path = positionals[0]
if (!path) {
  console.error('usage: bun tools/sigfox-listen.ts <serial port> [--freq hz] [--save capture.json]')
  process.exit(2)
}

const hex = (b: Uint8Array) => Buffer.from(b).toString('hex') || '-'
const stamp = () => new Date().toLocaleTimeString()
let rate = 2000
let rx: SigfoxReceiver | null = null
const saved: number[] = []
let lastSeq = -1
let dropped = 0

const start = (sampleRate: number) => {
  rate = sampleRate
  rx = new SigfoxReceiver(
    rate,
    (m) => {
      const kind = m.bit !== null ? `bit=${m.bit}` : `data=${hex(m.payload)}`
      console.log(
        `[${stamp()}] MESSAGE device=${m.deviceId} seq=${m.counter} ${kind}${m.downlink ? ' (downlink requested)' : ''} rssi=${m.rssi.toFixed(1)}dBm frames=${m.ranks.length}/3`,
      )
    },
    (f) => {
      const status = f.crcOk ? (f.repaired ? `CRC ok after repair (${f.repaired})` : 'CRC ok') : 'CRC error'
      console.log(`[${stamp()}]   frame ${f.rank + 1}/3 device=${f.deviceId} seq=${f.counter} ${status} rssi=${f.rssi.toFixed(1)} clock=${(f.clockErrorPpm / 1e4).toFixed(2)}%`)
    },
  )
}

const port = openPort(path, 115200)
port.onLine((line) => {
  if (line.startsWith('I ')) {
    const sr = Number(/sr=(\d+)/.exec(line)?.[1] ?? 2000)
    console.log(`[${stamp()}] receiver: ${line.slice(2)}`)
    if (!rx || sr !== rate) start(sr)
    return
  }
  if (!(line.startsWith('R') && line[5] === ':')) return
  const seq = Number.parseInt(line.slice(1, 5), 16)
  if (lastSeq >= 0 && seq !== ((lastSeq + 1) & 0xffff)) dropped += (seq - lastSeq - 1 + 0x10000) & 0xffff
  lastSeq = seq
  const hexs = line.slice(6)
  const raw = new Uint8Array(hexs.length >> 1)
  for (let i = 0; i < raw.length; i++) raw[i] = Number.parseInt(hexs.slice(i * 2, i * 2 + 2), 16)
  if (!rx) start(rate)
  rx!.pushRaw(raw)
  if (args.save) for (const v of raw) saved.push(v)
})
if (args.freq) port.write(`F${args.freq}\n`)
port.write('?\n')
console.log(`[${stamp()}] listening on ${path} (Ctrl+C to stop)`)

const stop = async () => {
  rx?.flush()
  await port.close()
  if (dropped) console.log(`${dropped} streamer lines were lost`)
  if (args.save) {
    writeFileSync(args.save, JSON.stringify({ rate, samples: saved.length, raw: Buffer.from(saved).toString('base64') }))
    console.log(`saved ${saved.length} samples to ${args.save}`)
  }
  process.exit(0)
}
process.on('SIGINT', stop)
process.on('SIGTERM', stop)
