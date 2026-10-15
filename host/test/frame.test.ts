import { describe, expect, test } from 'bun:test'
import { decodeFrame, encodeFrame, fromHex, toHex, TYPE_DOWNLINK, TYPE_UPLINK, verifyMac } from '../src/frame'

const key = fromHex('000102030405060708090a0b0c0d0e0f')

describe('frame', () => {
  test('uplink round trip', () => {
    const buf = encodeFrame({ type: TYPE_UPLINK, deviceId: '0A1B2C3D', seq: 4097, payload: fromHex('cafe'), key, ack: true, repeat: 2 })
    const f = decodeFrame(buf)
    expect(f).toMatchObject({ type: TYPE_UPLINK, deviceId: '0A1B2C3D', seq: 1, ack: true, oob: false, repeat: 2 })
    expect(toHex(f.payload)).toBe('cafe')
    expect(verifyMac(key, f)).toBe(true)
  })

  test('repeats share the same MAC', () => {
    const macs = [0, 1, 2].map((repeat) =>
      toHex(decodeFrame(encodeFrame({ type: TYPE_UPLINK, deviceId: '1', seq: 7, payload: new Uint8Array(), key, repeat })).mac),
    )
    expect(new Set(macs).size).toBe(1)
  })

  test('tampered payload or wrong key fails verification', () => {
    const buf = encodeFrame({ type: TYPE_UPLINK, deviceId: '0A1B2C3D', seq: 1, payload: fromHex('01'), key })
    buf[12] = 0x02
    expect(verifyMac(key, decodeFrame(buf))).toBe(false)
    const ok = encodeFrame({ type: TYPE_UPLINK, deviceId: '0A1B2C3D', seq: 1, payload: fromHex('01'), key })
    expect(verifyMac(fromHex('ff'.repeat(16)), decodeFrame(ok))).toBe(false)
  })

  test('layout matches docs/protocol.md (golden vector shared with firmware)', () => {
    const buf = encodeFrame({ type: TYPE_DOWNLINK, deviceId: '0A1B2C3D', seq: 0x123, payload: fromHex('0102030405060708'), key })
    // magic ver type flags | id (LE) | seq (LE) | len | payload | mac
    expect(toHex(buf.subarray(0, 12))).toBe('4e4b0102003d2c1b0a230108')
    expect(toHex(buf)).toMatchSnapshot()
  })

  test('rejects malformed frames', () => {
    expect(() => decodeFrame(fromHex('4e4b'))).toThrow()
    const buf = encodeFrame({ type: TYPE_UPLINK, deviceId: '1', seq: 1, payload: fromHex('01'), key })
    expect(() => decodeFrame(buf.subarray(0, buf.length - 1))).toThrow('length mismatch')
    expect(() => encodeFrame({ type: TYPE_UPLINK, deviceId: '1', seq: 1, payload: new Uint8Array(13), key })).toThrow()
  })
})
