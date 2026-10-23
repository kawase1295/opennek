// Record the RSSI stream of the OpenNEK LSM100A streamer and report the bursts it sees.
//   npx tsx tools/rssi-capture.ts /dev/ttyACM0 [--freq 923200000] [--bw 10] [--rate 2000] [--out capture]
// Writes <out>.json (all samples) and <out>.html (a plot of every burst) when stopped with Ctrl+C.
import { writeFileSync } from 'node:fs'
import { parseArgs } from 'node:util'
import { openPort } from './serial'

// Same detector as the Sigfox decoder: well above the noise floor, at least 300 ms long
export { type Burst, findBursts } from './sigfox/demod'
import { type Burst, findBursts } from './sigfox/demod'
import { isMain } from './main'

export function plotHtml(dbm: Float32Array, rate: number, bursts: Burst[], meta: Record<string, unknown>) {
  const W = 1200
  const H = 220
  const svgs = bursts.map((b, n) => {
    const margin = Math.round(rate * 0.1)
    const from = Math.max(0, b.start - margin)
    const to = Math.min(dbm.length, b.end + margin)
    const lo = b.floorDbm - 5
    const hi = b.peakDbm + 5
    const pts: string[] = []
    for (let i = from; i < to; i++) {
      const x = ((i - from) / Math.max(1, to - from - 1)) * W
      const y = H - ((dbm[i]! - lo) / (hi - lo)) * H
      pts.push(`${x.toFixed(1)},${y.toFixed(1)}`)
    }
    const ms = (((b.end - b.start) / rate) * 1000).toFixed(0)
    return `<h2>Burst ${n + 1}: t=${(b.start / rate).toFixed(3)} s, ${ms} ms, peak ${b.peakDbm.toFixed(1)} dBm, floor ${b.floorDbm.toFixed(1)} dBm</h2>
<svg viewBox="0 0 ${W} ${H}" preserveAspectRatio="none"><polyline points="${pts.join(' ')}"/></svg>`
  })
  return `<!doctype html><html><head><meta charset="utf-8"><title>RSSI capture</title>
<style>body{font:14px system-ui;margin:16px;background:#fff;color:#111}svg{width:100%;height:220px;background:#f4f6f8;border:1px solid #ddd}
polyline{fill:none;stroke:#2277aa;stroke-width:1;vector-effect:non-scaling-stroke}h2{font-size:14px;margin:18px 0 6px}
@media (prefers-color-scheme:dark){body{background:#14171b;color:#e3e6ea}svg{background:#1c2026;border-color:#2e343c}polyline{stroke:#4aa3d8}}</style></head>
<body><h1>RSSI capture</h1><pre>${JSON.stringify(meta, null, 2)}</pre>${svgs.join('\n') || '<p>No bursts found.</p>'}</body></html>`
}

if (isMain(import.meta.url)) {
  const { values: args, positionals } = parseArgs({
    allowPositionals: true,
    options: {
      freq: { type: 'string' },
      bw: { type: 'string' },
      rate: { type: 'string' },
      boost: { type: 'string' },
      out: { type: 'string', default: `rssi-${new Date().toISOString().replace(/[:.]/g, '-')}` },
    },
  })
  const path = positionals[0]
  if (!path) {
    console.error('usage: npx tsx tools/rssi-capture.ts <serial port> [--freq hz] [--bw code] [--rate hz] [--boost 0|1] [--out name]')
    process.exit(2)
  }
  const port = await openPort(path, 115200)
  const raw: number[] = []
  let status = ''
  let rate = Number(args.rate ?? 2000)
  let lastSeq = -1
  let dropped = 0
  port.onLine((line) => {
    if (line.startsWith('I ')) {
      status = line
      rate = Number(/sr=(\d+)/.exec(line)?.[1] ?? rate)
      console.log(`streamer: ${line.slice(2)}`)
    } else if (line.startsWith('R') && line[5] === ':') {
      const seq = Number.parseInt(line.slice(1, 5), 16)
      if (lastSeq >= 0 && seq !== ((lastSeq + 1) & 0xffff)) dropped += (seq - lastSeq - 1 + 0x10000) & 0xffff
      lastSeq = seq
      const hex = line.slice(6)
      for (let i = 0; i + 1 < hex.length; i += 2) raw.push(Number.parseInt(hex.slice(i, i + 2), 16))
    } else if (line.trim()) {
      console.log(`streamer: ${line}`)
    }
  })
  for (const [flag, cmd] of [[args.freq, 'F'], [args.bw, 'W'], [args.rate, 'S'], [args.boost, 'B']] as const) {
    if (flag !== undefined) port.write(`${cmd}${flag}\n`)
  }
  port.write('?\n')

  // Live view: noise floor and peak of the last second
  let shown = 0
  const timer = setInterval(() => {
    const recent = raw.slice(Math.max(shown, raw.length - rate))
    shown = raw.length
    if (!recent.length) return console.log('no samples yet (is the RSSI streamer running at 115200 bps?)')
    const dbm = recent.map((r) => -r / 2).sort((a, b) => a - b)
    const floor = dbm[Math.floor(dbm.length * 0.2)]!
    const peak = dbm.at(-1)!
    const bar = '#'.repeat(Math.max(0, Math.min(60, Math.round((peak - floor) / 1))))
    console.log(`${(raw.length / rate).toFixed(1).padStart(7)} s  floor ${floor.toFixed(1)} dBm  peak ${peak.toFixed(1)} dBm  ${bar}`)
  }, 1000)

  const finish = async () => {
    clearInterval(timer)
    await port.close()
    const dbm = Float32Array.from(raw, (r) => -r / 2)
    const bursts = findBursts(dbm, rate)
    const meta = { status, rate, samples: raw.length, seconds: raw.length / rate, droppedLines: dropped, bursts: bursts.length }
    writeFileSync(`${args.out}.json`, JSON.stringify({ ...meta, raw: Buffer.from(raw).toString('base64') }))
    writeFileSync(`${args.out}.html`, plotHtml(dbm, rate, bursts, meta))
    console.log(`\n${raw.length} samples (${meta.seconds.toFixed(1)} s, ${dropped} lines dropped), ${bursts.length} bursts`)
    for (const b of bursts) {
      console.log(`  t=${(b.start / rate).toFixed(3)} s  ${(((b.end - b.start) / rate) * 1000).toFixed(0)} ms  peak ${b.peakDbm.toFixed(1)} dBm`)
    }
    console.log(`saved ${args.out}.json and ${args.out}.html`)
    process.exit(0)
  }
  process.on('SIGINT', finish)
  process.on('SIGTERM', finish)
}
