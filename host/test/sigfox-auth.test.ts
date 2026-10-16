import { describe, expect, test } from 'bun:test'
import { decodeUplinkBits } from '../src/sigfox/frame'
import { PUBLIC_KEY, uplinkAuth, verifyUplink } from '../src/sigfox/auth'

const bitsOf = (hex: string) => [...Buffer.from(hex, 'hex')].map((b) => b.toString(2).padStart(8, '0')).join('')
const decode = (hex: string) => decodeUplinkBits(bitsOf(hex))!

// Bitstreams built by sigfox-ep-lib (SIGFOX_EP_BITSTREAM_build_application_frame) for device 0A1B2C3D,
// counter 1234, downlink flag set and the public key: [payload, rank 1, rank 3]
const PUBLIC_VECTORS: [string, string, string][] = [
  ['empty', 'aaaaa06b24d23d2c1b0a8cb34313', 'aaaaa0342de6b2671dc82f9f93d7'],
  ['bit0', 'aaaaa06ba4d23d2c1b0afa4d12bc', 'aaaaa0348de6b2671dc844de5613'],
  ['bit1', 'aaaaa06be4d23d2c1b0ad1d60ca2', 'aaaaa034dde6b2671dc865a38f8a'],
  ['a5', 'aaaaa08d24d23d2c1b0aa573b0dc70', 'aaaaa3022de6b2671dc80c2f5ceb6c'],
  ['0102', 'aaaaa35fa4d23d2c1b0a01025b17b66eb4f5', 'aaaaa5a38de6b2671dc88142cdd25bf519c8'],
  ['010203', 'aaaaa35f64d23d2c1b0a010203ad562e52a4', 'aaaaa5a37de6b2671dc88142834603a5c60d'],
  ['01020304', 'aaaaa35f24d23d2c1b0a01020304089d16e1', 'aaaaa5a32de6b2671dc8814283c50aba5359'],
  ['0102030405', 'aaaaa611e4d23d2c1b0a0102030405e9b4e027fdb448', 'aaaaa72cdde6b2671dc8814283c50493d9d82e02d95a'],
  ['010203040506', 'aaaaa611a4d23d2c1b0a010203040506960aaccb4ab4', 'aaaaa72c8de6b2671dc8814283c50447338807f99819'],
  ['01020304050607', 'aaaaa61164d23d2c1b0a01020304050607009ebfe4e3', 'aaaaa72c7de6b2671dc8814283c5044786c0b9101ddb'],
  ['0102030405060708', 'aaaaa61124d23d2c1b0a010203040506070808736be7', 'aaaaa72c2de6b2671dc8814283c5044786ca0a6fb11e'],
  ['010203040506070809', 'aaaaa94ce4d23d2c1b0a01020304050607080960cff3ae92212f', 'aaaaa997dde6b2671dc8814283c5044786ca0b38fc0f4536a964'],
  ['0102030405060708090a', 'aaaaa94ca4d23d2c1b0a0102030405060708090a984e18499535', 'aaaaa9978de6b2671dc8814283c5044786ca0b483e5d9e5bf078'],
  ['0102030405060708090a0b', 'aaaaa94c64d23d2c1b0a0102030405060708090a0bd5a4ca6da3', 'aaaaa9977de6b2671dc8814283c5044786ca0b488920cdf8f6cb'],
  ['0102030405060708090a0b0c', 'aaaaa94c24d23d2c1b0a0102030405060708090a0b0c25f6f4bd', 'aaaaa9972de6b2671dc8814283c5044786ca0b4889cf2c8b4992'],
]

describe('uplink authentication', () => {
  test('the public key is the one defined by sigfox-ep-lib', () => {
    expect(Buffer.from(PUBLIC_KEY).toString('hex')).toBe('00112233445566778899aabbccddeeff')
  })

  for (const [payload, rank1, rank3] of PUBLIC_VECTORS) {
    test(`public key frames match sigfox-ep-lib for payload ${payload}`, () => {
      for (const hex of [rank1, rank3]) {
        const f = decode(hex)
        expect(f.crcOk).toBe(true)
        expect(Buffer.from(uplinkAuth(PUBLIC_KEY, f)).toString('hex')).toBe(Buffer.from(f.auth).toString('hex'))
        expect(verifyUplink(PUBLIC_KEY, f)).toBe(true)
      }
    })
  }

  test('a frame recorded from a device in private key mode does not verify with the public key', () => {
    // counter 1979 from device 00763DA3, payload 11, auth ab6d (see sigfox.test.ts)
    const f = { deviceId: '00763DA3', counter: 1979, payload: Uint8Array.of(0x11), bit: null, downlink: false, auth: Uint8Array.of(0xab, 0x6d) }
    expect(verifyUplink(PUBLIC_KEY, f)).toBe(false)
  })

  test('the device key verifies frames built with it', () => {
    // sigfox-ep-lib with key 3f2a1099 0102030405060708090a0b0c, device 00763DA3, counter 1979, payload 11
    const f = decode('aaaaa08d07bba33d7600111ebbcd78')
    expect(verifyUplink(Buffer.from('3f2a10990102030405060708090a0b0c', 'hex'), f)).toBe(true)
    expect(verifyUplink(PUBLIC_KEY, f)).toBe(false)
  })
})
