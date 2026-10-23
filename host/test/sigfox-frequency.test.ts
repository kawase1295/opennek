import { describe, expect, test } from 'vitest'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { type RadioMessage, SigfoxRadio } from '../src/radio'
import { CFO_SCALE, cfoSigned, cfoStepHz, FrequencyTracker } from '../src/sigfox/frequency'
import { SigfoxReceiver } from '../src/sigfox/receiver'

test('CFO step follows SUBGRF_GetCFO for the streamer bandwidths', () => {
  // 234.3 kHz: demod sampling 500 kHz
  expect(cfoStepHz(0x0a)).toBeCloseTo(500_000 / 4096, 3)
  // 4.8 kHz: 10.4 kHz sampling, oversampling 17 at 600 bps; interpolated by 4 when the bit rate is above it
  expect(cfoStepHz(0x1f, 600)).toBeCloseTo(32e6 / (12 * 256) / 4096, 3)
  expect(cfoStepHz(0x1f, 50_000)).toBeCloseTo(((32e6 / (12 * 256)) * 4) / 4096, 3)
})

test('CFO register values are 12-bit signed', () => {
  expect(cfoSigned(0x7ff)).toBe(2047)
  expect(cfoSigned(0x800)).toBe(-2048)
  expect(cfoSigned(0xfff)).toBe(-1)
})

// Streamer lines of a cfo-capture recording: R and C lines, and a status line wherever the receiver was retuned
function* streamerLines(file: string) {
  const d = JSON.parse(readFileSync(join(import.meta.dirname, '../../fixtures', file), 'utf8'))
  const raw = Buffer.from(d.raw, 'base64')
  const status = (f: number) => `I fw=0.2.0 f=${f} bw=10 sr=2000 boost=1 cfo=1 pd=4`
  const tuning: { at: number; freq: number }[] = d.tuning.length ? d.tuning : [{ at: 0, freq: 923_200_000 }]
  for (const [n, seq] of (d.rssiSeq as number[]).entries()) {
    for (const t of tuning) if (t.at >= n * 64 && t.at < (n + 1) * 64) yield status(t.freq)
    const h = (seq & 0xffff).toString(16).padStart(4, '0').toUpperCase()
    yield `R${h}:${raw.subarray(n * 64, n * 64 + 64).toString('hex').toUpperCase()}`
    const c = d.cfo[seq] as number[] | undefined
    if (c) yield `C${h}:${c.map((v) => (v & 0xfff).toString(16).padStart(3, '0')).join('').toUpperCase()}`
  }
}

function replay(file: string, follow = false) {
  const radio = new SigfoxRadio(() => ({ port: '', freq: 923_200_000, follow }))
  const messages: RadioMessage[] = []
  const logs: string[] = []
  radio.onMessage((m) => messages.push(m))
  radio.onLog((l) => logs.push(l))
  for (const line of streamerLines(file)) radio.handleLine(line)
  radio.flush()
  return { messages, logs }
}

// Device 00763DA3, AT$SF=22,1 (downlink flag): frames 2 and 3 at frame 1 +/- 6 kHz
test('frame 1 is the midpoint of frames 2 and 3 after tuning to it', () => {
  const { messages } = replay('cfo-rc3-00763DA3-dl-retune.json')
  expect(messages).toHaveLength(1)
  const m = messages[0]!
  expect(m).toMatchObject({ deviceId: '00763DA3', counter: 2021, downlink: true, ranks: [0, 1, 2] })
  // the same as cfo-capture --retune printed for this recording; frame 1 read directly was 122 Hz lower
  expect(m.uplinkFreqHz).toBe(923_200_488)
})

test('frame 1 itself is read against the frequency it was retuned to', () => {
  // the tracker alone, fed like SigfoxRadio does, with the frame spans the receiver reported
  const { messages } = replay('cfo-rc3-00763DA3-dl-retune.json')
  const t = new FrequencyTracker(2000, 923_200_000)
  let lastSeq = -1
  for (const line of streamerLines('cfo-rc3-00763DA3-dl-retune.json')) {
    if (line[0] === 'I') t.tuned(Number(/f=(\d+)/.exec(line)![1]))
    if (line[0] === 'R') {
      lastSeq = Number.parseInt(line.slice(1, 5), 16)
      t.samples(Array.from(Buffer.from(line.slice(6), 'hex'), (v) => -v / 2))
    }
    if (line[0] === 'C' && Number.parseInt(line.slice(1, 5), 16) === lastSeq) {
      t.cfo(line.slice(6).match(/.{3}/g)!.map((h) => Number.parseInt(h, 16)))
    }
  }
  const frame1 = messages[0]!.frames.find((f) => f.rank === 0)!
  // cfo-capture --retune: 923.200488 MHz + CFO -122 Hz / CFO_SCALE
  const f = t.frameFrequency(frame1)!
  expect([f.lo, Math.round(f.hz)]).toEqual([923_200_488, 923_200_357])
  // without frames 2 and 3, frame 1 itself is used
  expect(t.frame1([frame1], true)).toBe(923_200_357)
})

test('without retuning, frame 1 is too far from the receiver frequency to be trusted', () => {
  // frame 1 read -43.8 kHz from 923.2 MHz: at a 7 % scale error that is ~3 kHz off
  const { messages } = replay('cfo-rc3-00763DA3-dl.json')
  expect(messages.map((m) => [m.counter, m.downlink, m.uplinkFreqHz])).toEqual([[2020, true, null]])
})

test('following the signal tunes to frame 1 while it is still on air', () => {
  const { logs } = replay('cfo-rc3-00763DA3-dl.json', true)
  const tune = logs.filter((l) => l.includes('tuning to'))
  expect(tune).toHaveLength(1)
  // the coarse frame 1 reading of that recording: 923.2 MHz - 43 823 Hz / CFO_SCALE
  expect(tune[0]).toBe('sigfox: tuning to 923.152878 MHz')
})

test('without frame 1, its end is estimated from frames 2 and 3', () => {
  const d = JSON.parse(readFileSync(join(import.meta.dirname, '../../fixtures', 'cfo-rc3-00763DA3-dl.json'), 'utf8'))
  const raw = Buffer.from(d.raw, 'base64')
  const run = (data: Uint8Array) => {
    const out: { ranks: number[]; firstFrameEnd: number | null }[] = []
    const rx = new SigfoxReceiver(d.rate, (m) => out.push(m))
    rx.pushRaw(data)
    rx.flush()
    return out
  }
  const [whole] = run(raw)
  expect(whole!.ranks).toEqual([0, 1, 2])
  // frame 1 (2.89-4.16 s into the recording) replaced by noise
  const lost = Uint8Array.from(raw)
  lost.fill(218, 2.7 * d.rate, 4.4 * d.rate)
  const [m] = run(lost)
  expect(m!.ranks).toEqual([1, 2])
  expect(Math.abs(m!.firstFrameEnd! - whole!.firstFrameEnd!)).toBeLessThan(0.02)
})

describe('FrequencyTracker', () => {
  const rate = 2000
  const line = (dbm: number) => new Array<number>(64).fill(dbm)
  const feed = (t: FrequencyTracker, seconds: number, dbm: number, cfo: number) => {
    const tunings: number[] = []
    for (let i = 0; i < (seconds * rate) / 64; i++) {
      t.samples(line(dbm))
      t.cfo(new Array<number>(8).fill(cfo & 0xfff))
      const f = t.nextTuning()
      if (f !== null) {
        tunings.push(f)
        t.tuned(f) // the streamer confirms with a status line
      }
    }
    return tunings
  }

  test('tunes to a frame once, then back to the base frequency after the message', () => {
    const t = new FrequencyTracker(rate, 923_200_000)
    t.tuned(923_200_000)
    expect(feed(t, 2, -110, 0)).toEqual([])
    // a frame 100 LSB (12.2 kHz) above the receiver frequency
    const step = cfoStepHz(0x0a)
    expect(feed(t, 1.3, -80, 100)).toEqual([923_200_000 + Math.round((100 * step) / CFO_SCALE)])
    // frames 2 and 3 do not move it again
    expect(feed(t, 2.4, -110, 0)).toEqual([])
    expect(feed(t, 1.3, -80, 50)).toEqual([])
    // 10 s of quiet ends the message
    expect(feed(t, 11, -110, 0)).toEqual([923_200_000])
  })

  test('short spikes are not frames', () => {
    const t = new FrequencyTracker(rate, 923_200_000)
    t.tuned(923_200_000)
    feed(t, 2, -110, 0)
    for (let i = 0; i < 20; i++) {
      expect(feed(t, 64 / rate, -80, 100)).toEqual([])
      expect(feed(t, (4 * 64) / rate, -110, 0)).toEqual([])
    }
  })
})
