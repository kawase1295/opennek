import { expect, test } from 'bun:test'
import { fromHex } from '../src/hex'
import { decodePayload, parsePayloadConfig } from '../src/payload'

test('sequential fields with endianness, bits and bool', () => {
  // temp = -12.3 (int16 LE x10 = 0xff85), hum = 0x40, flags byte 0b1000_0001, 12-bit counter 0xabc
  const data = fromHex('85ff4081abc0')
  const v = decodePayload('temp::int:16:little-endian hum::uint:8 door:3:bool:7 low:3:bool:0 flags::uint:8 cnt::uint:12', data)
  expect(v).toEqual({ temp: -123, hum: 0x40, door: true, low: true, flags: 0x81, cnt: 0xabc })
})

test('float, char and explicit offsets', () => {
  const buf = new Uint8Array(8)
  new DataView(buf.buffer).setFloat32(0, 1.5)
  buf.set(new TextEncoder().encode('OK'), 4)
  expect(decodePayload('v::float:32 s::char:2 again:0:uint:8', buf)).toEqual({ v: 1.5, s: 'OK', again: 0x3f })
})

test('errors', () => {
  expect(() => parsePayloadConfig('x::nope:8')).toThrow('unknown type')
  expect(() => parsePayloadConfig('x::bool:1')).toThrow('explicit byte offset')
  expect(() => decodePayload('x::uint:16', fromHex('01'))).toThrow('exceeds payload')
  expect(parsePayloadConfig('  ')).toEqual([])
})
