// Records RSSI and the GFSK carrier frequency offset (CFO) register from the LSM100A streamer (fw >= 0.2.0)
// to see whether the CFO follows a Sigfox uplink (#6).
//   npx tsx tools/cfo-capture.ts /dev/ttyACM0 [--pd 0] [--bw 10] [--freq 923200000] [--retune] [--out cfo]
// Stop with Ctrl+C: prints the CFO inside every burst and writes <out>.json.
// --retune: once the CFO of the first frame of a message has settled, tune to it, so that frames 2 and 3 of a
// message with the downlink flag (frame 1 +/- 6 kHz) land close to the receiver frequency, where an error in
// the CFO scale hardly matters. Their midpoint is then frame 1. Back to --freq after 10 s without a signal.
import { writeFileSync } from 'node:fs'
import { setTimeout as sleep } from 'node:timers/promises'
import { parseArgs } from 'node:util'
import { findBursts } from '../src/sigfox/demod'
import { CFO_EVERY, cfoSigned, cfoStepHz, RC3_DELTA_F_MF_HZ } from '../src/sigfox/frequency'
import { openPort } from './serial'
import { isMain } from './main'

const DEFAULT_FREQ = 923_200_000
const ABOVE_DB = 15 // a sample this far above the noise floor counts as signal
const SETTLE_MS = 300 // wait this long into a frame before trusting the CFO
const QUIET_MS = 10_000 // a gap this long ends a message

const median = (xs: number[]) => {
  const s = [...xs].sort((a, b) => a - b)
  return s.length ? s[Math.floor(s.length / 2)]! : Number.NaN
}

if (isMain(import.meta.url)) {
  const { values: args, positionals } = parseArgs({
    allowPositionals: true,
    options: {
      pd: { type: 'string' },
      bw: { type: 'string', default: '10' },
      freq: { type: 'string' },
      retune: { type: 'boolean', default: false },
      out: { type: 'string', default: `cfo-${new Date().toISOString().replace(/[:.]/g, '-')}` },
    },
  })
  const path = positionals[0]
  if (!path) {
    console.error('usage: npx tsx tools/cfo-capture.ts <serial port> [--pd code] [--bw code] [--freq hz] [--retune] [--out name]')
    process.exit(2)
  }
  const bw = Number(args.bw)
  const step = cfoStepHz(bw)
  const port = await openPort(path, 115200)
  const raw: number[] = []
  const cfo = new Map<number, number[]>() // R/C line seq -> raw CFO values
  const rssiSeq: number[] = [] // seq of each R line, in order
  let status = ''
  let rate = 2000
  const baseFreq = Number(args.freq ?? DEFAULT_FREQ)
  // Receiver frequency from sample `at` on, from the streamer's status lines
  const tuning: { at: number; freq: number }[] = []
  port.onLine((line) => {
    if (line.startsWith('I ')) {
      status = line
      rate = Number(/sr=(\d+)/.exec(line)?.[1] ?? rate)
      const f = Number(/\bf=(\d+)/.exec(line)?.[1])
      if (f && tuning.at(-1)?.freq !== f) tuning.push({ at: raw.length, freq: f })
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
  port.write(`F${baseFreq}\n`)
  port.write(`W${bw}\n`)
  if (args.pd !== undefined) port.write(`P${args.pd}\n`)
  port.write('C1\n')
  console.log(`CFO step ${step.toFixed(1)} Hz/LSB (bw code ${bw}). Send a message from the device, then Ctrl+C.`)

  // CFO value for sample index i (one per CFO_EVERY samples of each 64-sample line)
  const cfoAt = (i: number) => cfo.get(rssiSeq[Math.floor(i / 64)]!)?.[Math.floor((i % 64) / CFO_EVERY)]

  const freqAt = (i: number) => tuning.findLast((t) => t.at <= i)?.freq ?? baseFreq

  // --retune: follows the signal sample by sample
  let floor = Number.NaN
  let signalFrom = -1 // first sample of the current frame
  let lastSignal = -Infinity
  let retuned = false
  let scanned = 0
  const retune = () => {
    const quiet = Math.round((QUIET_MS / 1000) * rate)
    for (; scanned < raw.length; scanned++) {
      const dbm = -raw[scanned]! / 2
      if (Number.isNaN(floor)) floor = dbm
      const above = dbm > floor + ABOVE_DB
      if (!above) floor += (dbm - floor) / 2000 // slow noise floor tracker
      if (above) {
        if (scanned - lastSignal > quiet) retuned = false // a new message
        if (scanned - lastSignal > rate / 10) signalFrom = scanned
        lastSignal = scanned
      }
    }
    const now = raw.length
    if (now - lastSignal > quiet && freqAt(now) !== baseFreq && !tuning.some((t) => t.at > lastSignal && t.freq === baseFreq)) {
      console.log('  retune: back to the base frequency')
      port.write(`F${baseFreq}\n`)
      tuning.push({ at: now, freq: baseFreq }) // until the status line confirms it
    }
    if (retuned || signalFrom < 0 || now - lastSignal > rate / 10) return
    if (now - signalFrom < (SETTLE_MS / 1000) * rate) return
    const vs: number[] = []
    for (let i = signalFrom + Math.round(rate / 10); i < now; i += CFO_EVERY) {
      const v = cfoAt(i)
      if (v !== undefined) vs.push(v)
    }
    if (vs.length < 5) return
    const target = freqAt(now) + Math.round(median(vs) * step)
    retuned = true
    console.log(`  retune: frame 1 at ${(target / 1e6).toFixed(6)} MHz (coarse), tuning there`)
    port.write(`F${target}\n`)
  }

  let shown = 0
  if (args.retune) setInterval(retune, 50)
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
    await sleep(100)
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
    const absolute: { freq: number; cfo: number }[] = []
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
      // Settled CFO against the frequency the receiver was tuned to, per tuning: after a retune the register
      // needs a moment, so skip SETTLE_MS after the start of the frame and after every change
      const settle = Math.round((SETTLE_MS / 1000) * rate)
      const parts = new Map<number, number[]>()
      for (let i = b.start + settle; i < b.end; i += CFO_EVERY) {
        const v = cfoAt(i)
        if (v === undefined || tuning.some((t) => t.at <= i && i - t.at < settle)) continue
        const lo = freqAt(i)
        parts.set(lo, [...(parts.get(lo) ?? []), v * step])
      }
      for (const [lo, vs] of parts) {
        const c = median(vs)
        console.log(`  receiver ${(lo / 1e6).toFixed(6)} MHz + CFO ${c.toFixed(0)} Hz = ${((lo + c) / 1e6).toFixed(6)} MHz  (${vs.length} values)`)
      }
      // the last tuning is the closest one (after a retune)
      const [lo, last] = [...parts].at(-1) ?? [freqAt(b.start), [Number.NaN]]
      absolute.push({ freq: lo, cfo: median(last) })
    }
    // Groups of three frames of a bidirectional message: frame 1, frame 1 + 6 kHz, frame 1 - 6 kHz
    for (let n = 0; n + 2 < absolute.length; n += 3) {
      const [f1, f2, f3] = absolute.slice(n, n + 3) as [typeof absolute[0], typeof absolute[0], typeof absolute[0]]
      const abs = (f: typeof f1) => f.freq + f.cfo
      console.log(`\nmessage (bursts ${n + 1}-${n + 3}):`)
      console.log(`  frame 2 - frame 1 = ${(abs(f2) - abs(f1)).toFixed(0)} Hz, frame 3 - frame 1 = ${(abs(f3) - abs(f1)).toFixed(0)} Hz (expected +/-${RC3_DELTA_F_MF_HZ})`)
      if (f2.freq === f3.freq) {
        const scale = (f2.cfo - f3.cfo) / (2 * RC3_DELTA_F_MF_HZ)
        const mid = f2.freq + (f2.cfo + f3.cfo) / 2
        console.log(`  CFO scale from frames 2/3: ${scale.toFixed(3)}`)
        console.log(`  frame 1 = midpoint of frames 2/3 = ${(mid / 1e6).toFixed(6)} MHz (frame 1 measured directly: ${(abs(f1) / 1e6).toFixed(6)} MHz, difference ${(abs(f1) - mid).toFixed(0)} Hz)`)
        console.log(`  downlink frequency = ${((mid - 1_000_000) / 1e6).toFixed(6)} MHz`)
      }
    }
    const seqs = [...cfo.keys()]
    writeFileSync(`${args.out}.json`, JSON.stringify({ status, rate, bw, stepHz: step, tuning, raw: Buffer.from(raw).toString('base64'), rssiSeq, cfo: Object.fromEntries(seqs.map((k) => [k, cfo.get(k)])) }))
    console.log(`saved ${args.out}.json`)
    process.exit(0)
  }
  process.on('SIGINT', finish)
  process.on('SIGTERM', finish)
}
