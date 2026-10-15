// Envelope (RSSI) demodulator for Sigfox DBPSK uplinks.
// Pulse-shaped DBPSK makes the amplitude dip towards zero at every phase reversal, so the dips alone give
// the differentially decoded bits: a dip on a bit boundary means data bit 0, no dip means 1.
import { decodeUplinkBits, type UplinkFrame } from './frame'

export const BIT_RATE = 100

export type Burst = { start: number; end: number; peakDbm: number; floorDbm: number }

export function noiseFloor(dbm: Float32Array) {
  const sorted = Float32Array.from(dbm).sort()
  return sorted[Math.floor(sorted.length * 0.2)] ?? -120
}

// Frames last over a second and sit well above the noise; the dips inside a frame are short gaps
export function findBursts(dbm: Float32Array, rate: number, opts = { aboveDb: 15, minMs: 300, gapMs: 40 }, floor = noiseFloor(dbm)): Burst[] {
  const threshold = floor + opts.aboveDb
  const gap = Math.round((opts.gapMs / 1000) * rate)
  const bursts: Burst[] = []
  let start = -1
  let lastAbove = -1
  for (let i = 0; i <= dbm.length; i++) {
    const above = i < dbm.length && dbm[i]! > threshold
    if (above) {
      if (start < 0) start = i
      lastAbove = i
    } else if (start >= 0 && (i === dbm.length || i - lastAbove > gap)) {
      if ((lastAbove - start) / rate >= opts.minMs / 1000) {
        let peak = -200
        for (let k = start; k <= lastAbove; k++) peak = Math.max(peak, dbm[k]!)
        bursts.push({ start, end: lastAbove + 1, peakDbm: peak, floorDbm: floor })
      }
      start = -1
    }
  }
  return bursts
}

export type Bits = { bits: string; periodSamples: number; clockErrorPpm: number; levelDbm: number; dips: number }

// Finds the dips of one burst, recovers the bit clock from their positions and returns the data bits
export function burstBits(seg: Float32Array, rate: number): Bits | null {
  const nominal = rate / BIT_RATE
  const sorted = Float32Array.from(seg).sort()
  const level = sorted[Math.floor(sorted.length * 0.9)]!
  // Dips are measured against the local envelope on each side (max over 1.5 bits). A real dip has a high
  // level on both sides; this rejects the rising and falling edges of the frame, and copes with the
  // transmit power ramping during the first part of a frame.
  const win = Math.round(nominal * 1.5)
  const sideMax = (from: number, to: number) => {
    let m = -200
    for (let k = Math.max(0, from); k <= Math.min(seg.length - 1, to); k++) m = Math.max(m, seg[k]!)
    return m
  }
  // Dip centres: local minima at least 6 dB below both sides, half a bit apart, refined to sub-sample
  const dips: number[] = []
  const half = Math.round(nominal * 0.4)
  for (let i = 1; i < seg.length - 1; i++) {
    const v = seg[i]!
    const left = sideMax(i - win, i - 1)
    const right = sideMax(i + 1, i + win)
    if (v >= Math.min(left, right) - 6 || Math.max(left, right) < level - 25) continue
    let isMin = true
    for (let k = Math.max(0, i - half); k <= Math.min(seg.length - 1, i + half); k++) {
      if (seg[k]! < v || (seg[k] === v && k < i)) {
        isMin = false
        break
      }
    }
    if (!isMin) continue
    const a = seg[i - 1]!
    const c = seg[i + 1]!
    const denom = a - 2 * v + c
    dips.push(i + (denom !== 0 ? (0.5 * (a - c)) / denom : 0))
  }
  if (dips.length < 8) return null

  // Bit clock: assign each dip to a bit index with the current estimate, then refit period and phase by
  // least squares. Tracking dip by dip keeps a 1 % clock error from slipping a whole bit over a frame.
  let period = nominal
  // Anchor the grid on the mean phase of the first dips (circular mean modulo one bit)
  let c = 0
  let s = 0
  for (const d of dips.slice(0, 12)) {
    const a = (2 * Math.PI * (d - dips[0]!)) / nominal
    c += Math.cos(a)
    s += Math.sin(a)
  }
  let phase = dips[0]! + (Math.atan2(s, c) / (2 * Math.PI)) * nominal
  const points: [number, number][] = []
  for (let j = 0; j < dips.length; j++) {
    const k = Math.round((dips[j]! - phase) / period)
    const predicted = phase + k * period
    if (Math.abs(dips[j]! - predicted) > 0.35 * period || (points.length && k <= points.at(-1)![0])) continue
    points.push([k, dips[j]!])
    if (points.length >= 4) {
      let sk = 0, sd = 0, skk = 0, skd = 0
      for (const [pk, pd] of points) {
        sk += pk
        sd += pd
        skk += pk * pk
        skd += pk * pd
      }
      const n = points.length
      const det = n * skk - sk * sk
      if (det > 0) {
        period = (n * skd - sk * sd) / det
        phase = (sd - period * sk) / n
      }
    }
  }
  if (points.length < 8) return null
  const first = points[0]![0]
  const last = points.at(-1)![0]
  const onGrid = new Set(points.map(([k]) => k))
  let bits = '1' // the first preamble bit has no phase reference
  // Bits after the last dip are 1s (no reversal); count them up to where the signal ends
  let lastHigh = seg.length - 1
  while (lastHigh > 0 && seg[lastHigh]! < level - 15) lastHigh--
  const end = Math.max(last, Math.floor((lastHigh - phase) / period))
  for (let k = first; k <= end; k++) bits += onGrid.has(k) ? '0' : '1'
  return { bits, periodSamples: period, clockErrorPpm: Math.round((period / nominal - 1) * 1e6), levelDbm: level, dips: points.length }
}

export type DecodedFrame = UplinkFrame & { time: number; rssi: number; clockErrorPpm: number }

export function decodeBurst(dbm: Float32Array, rate: number, b: Burst, t0 = 0): DecodedFrame | null {
  const pad = Math.round(rate * 0.02)
  const seg = dbm.subarray(Math.max(0, b.start - pad), Math.min(dbm.length, b.end + pad))
  const bits = burstBits(seg, rate)
  if (!bits) return null
  const frame = decodeUplinkBits(bits.bits)
  if (!frame) return null
  return { ...frame, time: t0 + b.start / rate, rssi: b.peakDbm, clockErrorPpm: bits.clockErrorPpm }
}

// Decodes every frame found in a block of RSSI samples (dBm). `t0` is the time of the first sample in seconds.
export function decodeCapture(dbm: Float32Array, rate: number, t0 = 0, floor?: number): DecodedFrame[] {
  const out: DecodedFrame[] = []
  for (const b of findBursts(dbm, rate, undefined, floor)) {
    const f = decodeBurst(dbm, rate, b, t0)
    if (f) out.push(f)
  }
  return out
}
