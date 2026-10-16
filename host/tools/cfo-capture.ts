// Records RSSI and the GFSK carrier frequency offset (CFO) register from the LSM100A streamer (fw >= 0.2.0)
// to see whether the CFO follows a Sigfox uplink (#6).
//   bun tools/cfo-capture.ts /dev/ttyACM0 [--pd 0] [--bw 10] [--freq 923200000] [--out cfo]
// Stop with Ctrl+C: prints the CFO inside every burst and writes <out>.json.
import { writeFileSync } from 'node:fs'
import { parseArgs } from 'node:util'
import { findBursts } from '../src/sigfox/demod'
import { openPort } from './serial'

const XTAL_HZ = 32_000_000
const STREAMER_BIT_RATE = 50_000 // the streamer's GFSK setting (firmware-lsm100a configureRx)
const CFO_EVERY = 8

// Hz per CFO LSB, as in SUBGRF_GetCFO (STM32CubeWL): the RX_BW code has the BWSELR layout
export function cfoStepHz(bwCode: number, bitRate = STREAMER_BIT_RATE) {
  const mant = [4, 8, 10, 12][(bwCode >> 3) & 3]!
  const cfFs = XTAL_HZ / (mant * (1 << ((bwCode & 7) + 1)))
  const osr = Math.floor(cfFs / bitRate)
  const interp = osr < 4 ? 4 : osr < 8 ? 2 : 1
  return (cfFs * interp) / 4096
}

export const cfoSigned = (v: number) => (v & 0x800 ? v - 0x1000 : v)

const median = (xs: number[]) => {
  const s = [...xs].sort((a, b) => a - b)
  return s.length ? s[Math.floor(s.length / 2)]! : Number.NaN
}

if (import.meta.main) {
  const { values: args, positionals } = parseArgs({
    allowPositionals: true,
    options: {
      pd: { type: 'string' },
      bw: { type: 'string', default: '10' },
      freq: { type: 'string' },
      out: { type: 'string', default: `cfo-${new Date().toISOString().replace(/[:.]/g, '-')}` },
    },
  })
  const path = positionals[0]
  if (!path) {
    console.error('usage: bun tools/cfo-capture.ts <serial port> [--pd code] [--bw code] [--freq hz] [--out name]')
    process.exit(2)
  }
  const bw = Number(args.bw)
  const step = cfoStepHz(bw)
  const port = openPort(path, 115200)
  const raw: number[] = []
  const cfo = new Map<number, number[]>() // R/C line seq -> raw CFO values
  const rssiSeq: number[] = [] // seq of each R line, in order
  let status = ''
  let rate = 2000
  port.onLine((line) => {
    if (line.startsWith('I ')) {
      status = line
      rate = Number(/sr=(\d+)/.exec(line)?.[1] ?? rate)
      console.log(`streamer: ${line.slice(2)}`)
      if (!/cfo=1/.test(line)) console.log('  (CFO not streaming yet: needs firmware-lsm100a 0.2.0)')
    } else if (line[5] === ':' && (line[0] === 'R' || line[0] === 'C')) {
      const seq = Number.parseInt(line.slice(1, 5), 16)
      const hex = line.slice(6)
      if (line[0] === 'R') {
        rssiSeq.push(seq)
        for (let i = 0; i + 1 < hex.length; i += 2) raw.push(Number.parseInt(hex.slice(i, i + 2), 16))
      } else {
        const vs: number[] = []
        for (let i = 0; i + 2 < hex.length; i += 3) vs.push(cfoSigned(Number.parseInt(hex.slice(i, i + 3), 16)))
        cfo.set(seq, vs)
      }
    } else if (line.trim()) {
      console.log(`streamer: ${line}`)
    }
  })
  if (args.freq) port.write(`F${args.freq}\n`)
  port.write(`W${bw}\n`)
  if (args.pd !== undefined) port.write(`P${args.pd}\n`)
  port.write('C1\n')
  console.log(`CFO step ${step.toFixed(1)} Hz/LSB (bw code ${bw}). Send a message from the device, then Ctrl+C.`)

  // CFO value for sample index i (one per CFO_EVERY samples of each 64-sample line)
  const cfoAt = (i: number) => cfo.get(rssiSeq[Math.floor(i / 64)]!)?.[Math.floor((i % 64) / CFO_EVERY)]

  let shown = 0
  const timer = setInterval(() => {
    const from = Math.max(shown, raw.length - rate)
    shown = raw.length
    const peak = Math.max(...raw.slice(from).map((r) => -r / 2))
    const vs: number[] = []
    for (let i = from; i < raw.length; i += CFO_EVERY) {
      const v = cfoAt(i)
      if (v !== undefined) vs.push(v * step)
    }
    console.log(`${(raw.length / rate).toFixed(1).padStart(7)} s  peak ${peak.toFixed(1)} dBm  CFO median ${median(vs).toFixed(0)} Hz  (${new Set(vs).size} distinct)`)
  }, 1000)

  const finish = async () => {
    clearInterval(timer)
    port.write('C0\n')
    await Bun.sleep(100)
    await port.close()
    const dbm = Float32Array.from(raw, (r) => -r / 2)
    const bursts = findBursts(dbm, rate)
    const noise: number[] = []
    const inside = new Set<number>()
    for (const b of bursts) for (let i = b.start; i < b.end; i++) inside.add(i)
    for (let i = 0; i < raw.length; i += CFO_EVERY) {
      const v = cfoAt(i)
      if (v !== undefined && !inside.has(i)) noise.push(v * step)
    }
    console.log(`\n${(raw.length / rate).toFixed(1)} s, ${bursts.length} bursts. CFO outside bursts: median ${median(noise).toFixed(0)} Hz, ${new Set(noise).size} distinct values`)
    for (const [n, b] of bursts.entries()) {
      const vs: number[] = []
      for (let i = b.start; i < b.end; i += CFO_EVERY) {
        const v = cfoAt(i)
        if (v !== undefined) vs.push(v * step)
      }
      const s = [...vs].sort((a, c) => a - c)
      const q = (p: number) => s[Math.floor((s.length - 1) * p)] ?? Number.NaN
      // CFO every 100 ms through the burst
      const trace: string[] = []
      for (let t = b.start; t < b.end; t += Math.round(rate / 10)) trace.push(String(Math.round((cfoAt(t) ?? Number.NaN) * step)))
      console.log(`burst ${n + 1}: t=${(b.start / rate).toFixed(2)} s ${(((b.end - b.start) / rate) * 1000).toFixed(0)} ms peak ${b.peakDbm.toFixed(1)} dBm`)
      console.log(`  CFO median ${q(0.5).toFixed(0)} Hz, IQR ${q(0.25).toFixed(0)}..${q(0.75).toFixed(0)} Hz, ${new Set(vs).size} distinct`)
      console.log(`  every 100 ms: ${trace.join(' ')}`)
    }
    const seqs = [...cfo.keys()]
    writeFileSync(`${args.out}.json`, JSON.stringify({ status, rate, bw, stepHz: step, raw: Buffer.from(raw).toString('base64'), rssiSeq, cfo: Object.fromEntries(seqs.map((k) => [k, cfo.get(k)])) }))
    console.log(`saved ${args.out}.json`)
    process.exit(0)
  }
  process.on('SIGINT', finish)
  process.on('SIGTERM', finish)
}
