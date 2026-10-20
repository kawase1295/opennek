import { expect, test } from 'bun:test'
import { DeviceInput } from '../src/db'
import { parseDownlinkTemplate, renderDownlink } from '../src/downlink'

const ctx = { time: 1_700_000_000_123, station: '0000', rssi: -100 }

test('plain 8-byte hex renders as itself', () => {
  expect(renderDownlink('0102030405060708', ctx)).toBe('0102030405060708')
})

test('{time} is the uplink UNIX time in seconds, big-endian', () => {
  // 1700000000 = 0x6553f100
  expect(renderDownlink('{time}01020304', ctx)).toBe('6553f10001020304')
})

test('{rssi} is a signed 16-bit big-endian value', () => {
  expect(renderDownlink('{time}0102{rssi}', ctx)).toBe('6553f1000102ff9c')
  expect(renderDownlink('{time}0102{rssi}', { ...ctx, rssi: 5 })).toBe('6553f10001020005')
})

test('{rssi} saturates to the int16 range', () => {
  expect(renderDownlink('000000000000{rssi}', { ...ctx, rssi: -40000 })).toBe('0000000000008000')
  expect(renderDownlink('000000000000{rssi}', { ...ctx, rssi: 40000 })).toBe('0000000000007fff')
})

test('{tapid} is the station parsed as hex and left-padded to 4 bytes', () => {
  expect(renderDownlink('{tapid}00000000', ctx)).toBe('0000000000000000')
  expect(renderDownlink('{tapid}00000000', { ...ctx, station: 'AABBCC' })).toBe('00aabbcc00000000')
  expect(renderDownlink('{tapid}00000000', { ...ctx, station: '11AABBCC' })).toBe('11aabbcc00000000')
})

test('{tapid} falls back to zero when the station is not hex', () => {
  expect(renderDownlink('{tapid}00000000', { ...ctx, station: 'sigfox' })).toBe('0000000000000000')
})

test('templates are rejected unless they render to exactly 8 bytes', () => {
  expect(() => parseDownlinkTemplate('{time}0102')).toThrow('8 bytes')
  expect(() => parseDownlinkTemplate('{time}{time}00')).toThrow('8 bytes')
  expect(() => parseDownlinkTemplate('')).toThrow('8 bytes')
})

test('unknown variables, odd hex digits and non-hex characters are rejected', () => {
  expect(() => parseDownlinkTemplate('{foo}01020304')).toThrow('{foo}')
  expect(() => parseDownlinkTemplate('{TIME}01020304')).toThrow('{TIME}')
  expect(() => parseDownlinkTemplate('{time}010{rssi}0')).toThrow('even number')
  expect(() => parseDownlinkTemplate('{time}0102zz{rssi}')).toThrow('not hex')
  expect(() => parseDownlinkTemplate('{time01020304')).toThrow('not hex')
})

const device = { id: '0a1b2c3d', downlinkMode: 'static' }

test('DeviceInput accepts templates and lower-cases the hex parts', () => {
  expect(DeviceInput.parse({ ...device, downlinkData: '{time}AABB{rssi}' }).downlinkData).toBe('{time}aabb{rssi}')
  expect(DeviceInput.parse({ ...device, downlinkData: 'AABBCCDDEEFF0011' }).downlinkData).toBe('aabbccddeeff0011')
  expect(DeviceInput.parse({ ...device, downlinkData: '' }).downlinkData).toBe('')
})

test('DeviceInput rejects invalid templates with the parser message', () => {
  const r = DeviceInput.safeParse({ ...device, downlinkData: '{foo}01020304' })
  expect(r.success).toBe(false)
  expect(r.error!.issues[0]!.message).toContain('{foo}')
  expect(DeviceInput.safeParse({ ...device, downlinkData: '0102' }).success).toBe(false)
})
