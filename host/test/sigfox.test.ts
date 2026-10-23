import { describe, expect, test } from 'vitest'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { burstBits, decodeCapture } from '../src/sigfox/demod'
import { convolve, crc16, deconvolve, decodeUplinkBits, encodeUplink } from '../src/sigfox/frame'
import { type SigfoxMessage, SigfoxReceiver } from '../src/sigfox/receiver'

const hex = (b: Uint8Array) => Buffer.from(b).toString('hex')

describe('frame', () => {
  test('convolution is invertible for every rank', () => {
    const bits = '1011001110001011010011110000101'
    for (const rank of [0, 1, 2]) expect(deconvolve(convolve(bits, rank), rank)).toBe(bits)
  })

  test('round trip for every payload size and repetition', () => {
    for (let n = 0; n <= 12; n++) {
      for (const rank of [0, 1, 2]) {
        const payload = Uint8Array.from({ length: n }, (_, i) => (i * 37 + n) & 0xff)
        const li = n >= 2 ? 3 - ((n - 1) % 4) : 0
        const auth = Uint8Array.from({ length: li + 2 }, (_, i) => 0xa0 + i)
        const bits = encodeUplink({ rank, deviceId: '00763DA3', counter: 1979 + n, payload, auth, downlink: n % 2 === 1 })
        // the receiver never sees the first preamble bit
        const f = decodeUplinkBits(bits.slice(1).padStart(bits.length, '1'))!
        expect(f).toMatchObject({ rank, deviceId: '00763DA3', counter: 1979 + n, downlink: n % 2 === 1, crcOk: true, repaired: null })
        expect(hex(f.payload)).toBe(hex(payload))
        expect(hex(f.auth)).toBe(hex(auth))
      }
    }
  })

  test('single-bit messages', () => {
    const f = decodeUplinkBits(encodeUplink({ rank: 0, deviceId: '1', counter: 5, payload: new Uint8Array(), auth: new Uint8Array(2), bit: 1 }))!
    expect(f).toMatchObject({ bit: 1, crcOk: true })
  })

  test('a slipped bit is repaired using the CRC', () => {
    const bits = encodeUplink({ rank: 0, deviceId: '00763DA3', counter: 1979, payload: Uint8Array.of(0x11), auth: Uint8Array.of(0xab, 0x6d) })
    const slipped = bits.slice(0, 70) + bits.slice(71)
    const f = decodeUplinkBits(slipped)!
    expect(f.crcOk).toBe(true)
    expect(f.repaired).toContain('inserted')
    expect(hex(f.payload)).toBe('11')
  })

  test('crc16 matches the frame recorded from a real device', () => {
    // LI/BF/MC, ID (LSB first), payload, auth of counter 1979 from device 00763DA3
    expect(crc16(Uint8Array.from([0x07, 0xbb, 0xa3, 0x3d, 0x76, 0x00, 0x11, 0xab, 0x6d]))).toBe(0x873b)
  })
})

// Pulse-shaped DBPSK envelope sampled by a receiver whose clock runs `clockError` off
function envelope(dataBits: string, rate: number, clockError: number, peakDbm = -75, floorDbm = -122) {
  const perBit = (rate / 100) * (1 + clockError)
  const lead = Math.round(rate * 0.3)
  const n = lead * 2 + Math.ceil(dataBits.length * perBit)
  const out = new Float32Array(n)
  for (let i = 0; i < n; i++) {
    const t = (i - lead) / perBit // in bits
    let a = 0
    if (t >= 0 && t < dataBits.length) {
      // a phase reversal on the boundary before bit k when the data bit is 0 (the first bit has no
      // reference); the amplitude passes close to zero there
      const k = Math.round(t)
      const reversal = k > 0 && k < dataBits.length && dataBits[k] === '0'
      a = reversal ? Math.max(0.05, Math.sin((Math.PI / 2) * Math.min(1, Math.abs(t - k) / 0.3))) : 1
    }
    const p = 10 * Math.log10(10 ** (peakDbm / 10) * a * a + 10 ** (floorDbm / 10))
    out[i] = Math.round((p + (Math.random() - 0.5)) * 2) / 2
  }
  return out
}

describe('demodulator', () => {
  for (const clockError of [-0.015, 0, 0.015]) {
    test(`decodes a synthetic frame with ${clockError * 100}% receiver clock error`, () => {
      const bits = encodeUplink({ rank: 1, deviceId: '01F9419F', counter: 42, payload: Uint8Array.of(1, 2, 3, 4, 5), auth: Uint8Array.of(9, 9, 9, 9, 9) })
      const [f] = decodeCapture(envelope(bits, 2000, clockError), 2000)
      expect(f).toMatchObject({ deviceId: '01F9419F', counter: 42, rank: 1, crcOk: true, repaired: null })
      expect(hex(f!.payload)).toBe('0102030405')
      expect(Math.abs(f!.clockErrorPpm - clockError * 1e6)).toBeLessThan(3000)
    })
  }

  test('rejects noise', () => {
    const noise = Float32Array.from({ length: 20000 }, () => -122 + Math.random() * 3)
    expect(decodeCapture(noise, 2000)).toEqual([])
    expect(burstBits(noise.subarray(0, 3000), 2000)).toBeNull()
  })
})

describe('real capture (LSM100A, RC3, device 00763DA3)', () => {
  const raw = readFileSync(join(import.meta.dirname, 'fixtures/rc3-00763DA3.rssi'))

  test('every repetition of the three messages decodes with a valid CRC', () => {
    const frames = decodeCapture(Float32Array.from(raw, (r) => -r / 2), 2000)
    expect(frames.map((f) => [f.counter, f.rank, f.crcOk])).toEqual([
      [1979, 0, true], [1979, 1, true], [1979, 2, true],
      [1980, 0, true], [1980, 1, true], [1980, 2, true],
      [1981, 0, true], [1981, 1, true], [1981, 2, true],
    ])
    for (const f of frames) {
      expect(f.deviceId).toBe('00763DA3')
      expect(hex(f.payload)).toBe('11')
    }
  })

  test('the streaming receiver merges repetitions into messages', () => {
    const messages: SigfoxMessage[] = []
    const rx = new SigfoxReceiver(2000, (m) => messages.push(m))
    for (let i = 0; i < raw.length; i += 64) rx.pushRaw(raw.subarray(i, i + 64))
    rx.flush()
    expect(messages.map((m) => [m.deviceId, m.counter, hex(m.payload), m.ranks])).toEqual([
      ['00763DA3', 1979, '11', [0, 1, 2]],
      ['00763DA3', 1980, '11', [0, 1, 2]],
      ['00763DA3', 1981, '11', [0, 1, 2]],
    ])
  })
})

describe('live session (5-byte payload and downlink request)', () => {
  const raw = readFileSync(join(import.meta.dirname, 'fixtures/rc3-00763DA3-live.rssi'))

  test('repetitions about 4 s apart still merge into one message each', () => {
    const messages: SigfoxMessage[] = []
    const rx = new SigfoxReceiver(2000, (m) => messages.push(m))
    for (let i = 0; i < raw.length; i += 64) rx.pushRaw(raw.subarray(i, i + 64))
    rx.flush()
    expect(messages.map((m) => [m.counter, hex(m.payload), m.downlink, m.ranks])).toEqual([
      [1982, '1234567890', false, [0, 1, 2]],
      [1983, '11', false, [0, 1, 2]],
      [1984, '11', true, [0, 1, 2]],
    ])
  })
})
