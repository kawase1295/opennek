import { expect, test } from 'bun:test'
import { Store } from '../src/db'
import { Engine } from '../src/engine'
import { PUBLIC_KEY, uplinkAuth } from '../src/sigfox/auth'
import { decodeControl, decodeUplinkBits, encodeUplink } from '../src/sigfox/frame'

// Downlink confirmation as sigfox-ep-lib builds it: CT 0x09, idle 3300 mV, TX 3150 mV, 23.5 °C, received at -72 dBm
const confirmation = Uint8Array.of(0x09, 0xe4, 0x0c, 0x4e, 0x0c, 0xeb, 0x00, 28)

test('control frames are decoded with the control frame types', () => {
  for (const rank of [0, 1, 2]) {
    const f = { deviceId: '00763DA3', counter: 2031, payload: confirmation, bit: null, downlink: false }
    const bits = encodeUplink({ ...f, bit: undefined, rank, auth: uplinkAuth(PUBLIC_KEY, f), control: true })
    // the receiver starts somewhere in the preamble
    const frame = decodeUplinkBits(bits.slice(4))
    expect(frame).toMatchObject({ rank, deviceId: '00763DA3', counter: 2031, control: true, crcOk: true, downlink: false })
    expect(frame!.payload).toEqual(confirmation)
    expect(frame!.auth).toEqual(uplinkAuth(PUBLIC_KEY, f))
  }
})

test('data frames are not control frames', () => {
  const f = { deviceId: '00763DA3', counter: 5, payload: Uint8Array.of(1, 2, 3, 4, 5, 6, 7, 8), bit: null, downlink: false }
  const frame = decodeUplinkBits(encodeUplink({ ...f, bit: undefined, rank: 0, auth: uplinkAuth(PUBLIC_KEY, f) }).slice(4))
  expect(frame).toMatchObject({ control: false, crcOk: true })
})

test('control payloads', () => {
  expect(decodeControl(confirmation)).toEqual({
    type: 'downlinkConfirmation', voltageIdleMv: 3300, voltageTxMv: 3150, temperatureC: 23.5, downlinkRssiDbm: -72,
  })
  // keep-alive: no RSSI, negative temperature
  expect(decodeControl(Uint8Array.of(0x08, 0xe4, 0x0c, 0x4e, 0x0c, 0x9c, 0xff))).toEqual({
    type: 'keepAlive', voltageIdleMv: 3300, voltageTxMv: 3150, temperatureC: -10,
  })
  // received on air from an AX-SFJK after a downlink (seq 2046)
  expect(decodeControl(Uint8Array.from(Buffer.from('090c0d1b0d320116', 'hex')))).toEqual({
    type: 'downlinkConfirmation', voltageIdleMv: 3340, voltageTxMv: 3355, temperatureC: 30.6, downlinkRssiDbm: -78,
  })
  // a weak downlink: RSSI + 100 is negative
  expect(decodeControl(Uint8Array.of(0x09, 0, 0, 0, 0, 0, 0, 0xf6))!.downlinkRssiDbm).toBe(-110)
  expect(decodeControl(Uint8Array.of(0x07, 0, 0, 0, 0, 0, 0, 0))).toBeNull()
  expect(decodeControl(Uint8Array.of(0x09, 0, 0))).toBeNull()
})

test('a downlink confirmation is stored as OOB and marks the downlink as received', () => {
  const store = new Store(':memory:')
  const engine = new Engine(store)
  const message = (counter: number, payload: Uint8Array, control: boolean) => {
    const f = { deviceId: '00763DA3', counter, payload, bit: null, downlink: false }
    return {
      ...f, control, auth: uplinkAuth(PUBLIC_KEY, f), time: 0, rssi: -75, ranks: [0], frames: [], clockErrorPpm: 0,
      receivedAt: Date.now(), firstFrameEnd: null, firstFrameEndAt: null, uplinkFreqHz: null,
    }
  }
  // a downlink that was sent for 2030, and a newer one for another seq that must stay as it is
  for (const seq of [2030, 2029]) {
    const m = store.insertMessage({
      deviceId: '00763DA3', time: 0, seq, data: '61', ack: true, oob: false, rssi: -75, nbFrames: 3, auth: 'public',
      station: '0000', decoded: null, downlink: '0102030405060708', downlinkStatus: 'pending',
    })
    store.updateMessage(m.id, { downlinkStatus: 'sent' })
  }
  engine.handleSigfox(message(2031, confirmation, true))
  const [ack, other, dl] = store.listMessages()
  expect(ack).toMatchObject({ seq: 2031, oob: true, ack: false, auth: 'public', data: '09e40c4e0ceb001c' })
  expect(ack!.decoded).toMatchObject({ type: 'downlinkConfirmation', downlinkRssiDbm: -72 })
  expect(dl).toMatchObject({ seq: 2030, downlinkStatus: 'acked' })
  expect(other).toMatchObject({ seq: 2029, downlinkStatus: 'sent' })

  // a data message with the same bytes is not a confirmation
  engine.handleSigfox(message(31, confirmation, false))
  expect(store.listMessages()[0]).toMatchObject({ seq: 31, oob: false, decoded: null })
})
