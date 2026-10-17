// Sigfox downlink frame, the inverse of SIGFOX_EP_BITSTREAM_decode_downlink_frame in sigfox-ep-lib:
//   DL-PHY content (15 bytes) = ECC (4) | payload (8) | auth (2) | CRC8 (1), BCH(15,11)-coded column-wise
//   and whitened with a PN sequence seeded by device ID * message counter.
// On air: GFSK 600 bps, deviation 800 Hz, preamble then the sync word 0xB227, then the 15 bytes.
import { aesBlocks } from './auth'

export const DL_SYNC_WORD = 0xb227
export const DL_BIT_RATE = 600
export const DL_DEVIATION_HZ = 800
// RC3: the downlink is sent on the frequency of the uplink's first frame + (f_dl - f_ul)
export const RC3_UL_HZ = 923_200_000
export const RC3_DL_HZ = 922_200_000
// RC3: the device listens from T_W after the end of uplink frame 1 for T_RX (sigfox_ep_api.c starts the T_W
// timer when frame rank 1 has been sent)
export const RC3_DL_T_W_MS = 20_000
export const RC3_DL_T_RX_MS = 25_000
// Where to transmit inside that window: late enough for the error of the device's timer and of our estimate of
// the end of frame 1, and early enough that carrier sense can postpone the transmission
const TX_DELAY_MS = 2000
const TX_END_MARGIN_MS = 1000

export function downlinkFrequency(uplinkFreqHz: number) {
  return uplinkFreqHz + RC3_DL_HZ - RC3_UL_HZ
}

// at: when to transmit; deadline: the latest start that still ends inside the device's window (wall-clock ms)
export function downlinkWindow(firstFrameEndAt: number) {
  const opens = firstFrameEndAt + RC3_DL_T_W_MS
  return { at: opens + TX_DELAY_MS, deadline: opens + RC3_DL_T_RX_MS - TX_END_MARGIN_MS }
}

const POW_ALPHA = [1, 2, 4, 8, 3, 6, 12, 11, 5, 10, 7, 14, 15, 13, 9]
const PAYLOAD = 4
const AUTH = 12
const CRC = 14

export function crc8(data: Uint8Array, poly = 0x2f) {
  let c = 0
  for (const b of data) {
    c ^= b
    for (let i = 0; i < 8; i++) c = c & 0x80 ? ((c << 1) ^ poly) & 0xff : (c << 1) & 0xff
  }
  return c
}

export function downlinkAuth(key: Uint8Array, deviceId: string, counter: number, payload: Uint8Array) {
  const id = Number.parseInt(deviceId, 16)
  const input = new Uint8Array(16)
  for (let i = 0; i < 4; i++) input[i] = (id >>> (8 * i)) & 0xff
  input[4] = counter & 0xff
  input[5] = (counter >> 8) & 0x0f
  input.set(payload, 6)
  input[14] = id & 0xff
  input[15] = (id >>> 8) & 0xff
  return aesBlocks(key, input).slice(0, 2)
}

function pnStep(pn: number) {
  for (let roll = 0; roll < 8; roll++) {
    const msb = pn & 0x21
    pn = (pn >> 1) | (msb === 0x20 || msb === 0x01 ? 0x100 : 0)
  }
  return pn
}

// XOR with the PN sequence: whitening and de-whitening are the same operation
export function whiten(phy: Uint8Array, deviceId: string, counter: number) {
  const buf = new Uint8Array(16)
  buf.set(phy.subarray(0, 15))
  let pn = Number((BigInt(Number.parseInt(deviceId, 16)) * BigInt(counter)) & 0x1ffn)
  if (pn === 0) pn = 0x1ff
  let idx = 0
  for (; idx < 8; idx++) {
    pn = pnStep(pn)
    buf[idx]! ^= (pn >> (idx + 1)) & 0xff
    buf[idx + 1]! ^= ((pn & ((1 << (idx + 1)) - 1)) << (7 - idx)) & 0xff
  }
  for (idx = 0; idx < 5; idx++) {
    pn = pnStep(pn)
    buf[idx + 9]! ^= (pn >> (idx + 1)) & 0xff
    buf[idx + 10]! ^= ((pn & ((1 << (idx + 1)) - 1)) << (7 - idx)) & 0xff
  }
  pn = pnStep(pn)
  buf[14]! ^= (pn >> (idx + 1)) & 0xff
  return buf.slice(0, 15)
}

// Fills bytes 0-3 so that every bit column (bit i of bytes 0..14) is a BCH(15,11) codeword: its syndrome,
// the XOR of alpha^j over the set bits, is zero. alpha^0..alpha^3 are 1, 2, 4, 8, so the parity of
// column i is simply the syndrome of its data bits.
function addBch(phy: Uint8Array) {
  phy.fill(0, 0, PAYLOAD)
  for (let i = 0; i < 8; i++) {
    let s = 0
    for (let j = PAYLOAD; j < 15; j++) if ((phy[j]! >> i) & 1) s ^= POW_ALPHA[j]!
    for (let j = 0; j < PAYLOAD; j++) if ((s >> j) & 1) phy[j]! |= 1 << i
  }
}

export function encodeDownlink(opts: { deviceId: string; counter: number; payload: Uint8Array; key: Uint8Array }) {
  if (opts.payload.length !== 8) throw new Error('downlink payload must be 8 bytes')
  const phy = new Uint8Array(15)
  phy.set(opts.payload, PAYLOAD)
  phy.set(downlinkAuth(opts.key, opts.deviceId, opts.counter, opts.payload), AUTH)
  phy[CRC] = crc8(phy.subarray(PAYLOAD, CRC))
  addBch(phy)
  return whiten(phy, opts.deviceId, opts.counter)
}
