import { expect, test } from 'bun:test'
import { PUBLIC_KEY } from '../src/sigfox/auth'
import { crc8, encodeDownlink, whiten } from '../src/sigfox/downlink'

const hex = (b: Uint8Array) => Buffer.from(b).toString('hex')
const bytes = (h: string) => Uint8Array.from(Buffer.from(h, 'hex'))
const privateKey = bytes('3f2a10990102030405060708090a0b0c')

// DL-PHY contents accepted by sigfox-ep-lib's SIGFOX_EP_BITSTREAM_decode_downlink_frame (tools/sigfox-ep-oracle),
// which returned the payload for each: [device ID, counter, key, payload, frame]
const VECTORS: [string, number, Uint8Array, string, string][] = [
  ['0A1B2C3D', 1234, PUBLIC_KEY, '0102030405060708', 'd1232ec88a6a34718ab5d1508837e4'],
  ['0A1B2C3D', 0, PUBLIC_KEY, 'deadbeefcafebabe', 'fc5e5202028572a720c3ce39884f08'],
  ['00763DA3', 1984, privateKey, '0000000000000000', 'ba00921dd6b681f678cb1d452a7d68'],
  ['FFFFFFFF', 4095, PUBLIC_KEY, 'ffffffffffffffff', '061dbfd49b8eaf26c1b862764dc409'],
  ['00000200', 1, PUBLIC_KEY, '1122334455667788', '4d22530dcd0aff0cbf5b030f7cdce8'],
]

for (const [deviceId, counter, key, payload, frame] of VECTORS) {
  test(`downlink frame for ${deviceId} counter ${counter} is the one sigfox-ep-lib accepts`, () => {
    expect(hex(encodeDownlink({ deviceId, counter, key, payload: bytes(payload) }))).toBe(frame)
  })
}

test('the frame depends on the key and the counter of the uplink', () => {
  const base = { deviceId: '0A1B2C3D', counter: 1234, payload: bytes('0102030405060708') }
  // sigfox-ep-lib rejects the first vector with the private key or with counter 1235
  expect(hex(encodeDownlink({ ...base, key: privateKey }))).not.toBe(VECTORS[0]![4])
  expect(hex(encodeDownlink({ ...base, counter: 1235, key: PUBLIC_KEY }))).not.toBe(VECTORS[0]![4])
})

test('whitening is its own inverse', () => {
  const phy = bytes('00112233445566778899aabbccddee')
  expect(hex(whiten(whiten(phy, '0A1B2C3D', 7), '0A1B2C3D', 7))).toBe(hex(phy))
})

test('crc8 uses polynomial 0x2F with a zero start value', () => {
  expect(crc8(new Uint8Array(10))).toBe(0)
  expect(crc8(Uint8Array.of(1))).toBe(0x2f)
})

test('the payload must be 8 bytes', () => {
  expect(() => encodeDownlink({ deviceId: '0A1B2C3D', counter: 1, key: PUBLIC_KEY, payload: bytes('01') })).toThrow()
})
