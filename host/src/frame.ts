import { createHmac, timingSafeEqual } from 'node:crypto'

// 無線フレーム (docs/protocol.md の (1))
export const MAGIC = [0x4e, 0x4b] as const
export const VERSION = 1
export const TYPE_UPLINK = 0x01
export const TYPE_DOWNLINK = 0x02
export const FLAG_ACK = 0x01
export const FLAG_OOB = 0x08
export const HEADER_LEN = 12
export const MAC_LEN = 4
export const MAX_UPLINK_PAYLOAD = 12
export const DOWNLINK_PAYLOAD = 8
export const SEQ_MOD = 4096

export type Frame = {
  type: number
  ack: boolean
  oob: boolean
  repeat: number
  deviceId: string
  seq: number
  payload: Uint8Array
  mac: Uint8Array
  // MAC 計算対象 (repeat ビットを 0 にしたヘッダ + ペイロード)
  signed: Uint8Array
}

export const formatDeviceId = (id: number) => (id >>> 0).toString(16).toUpperCase().padStart(8, '0')
export const parseDeviceId = (s: string) => {
  if (!/^[0-9a-fA-F]{1,8}$/.test(s)) throw new Error(`invalid device id: ${s}`)
  return Number.parseInt(s, 16) >>> 0
}

export const toHex = (b: Uint8Array) => Buffer.from(b).toString('hex')
export const fromHex = (s: string) => {
  if (!/^([0-9a-fA-F]{2})*$/.test(s)) throw new Error(`invalid hex: ${s}`)
  return new Uint8Array(Buffer.from(s, 'hex'))
}

export const computeMac = (key: Uint8Array, signed: Uint8Array) =>
  new Uint8Array(createHmac('sha256', key).update(signed).digest().subarray(0, MAC_LEN))

export const verifyMac = (key: Uint8Array, frame: Frame) =>
  timingSafeEqual(computeMac(key, frame.signed), frame.mac)

export function decodeFrame(buf: Uint8Array): Frame {
  if (buf.length < HEADER_LEN + MAC_LEN) throw new Error('frame too short')
  if (buf[0] !== MAGIC[0] || buf[1] !== MAGIC[1]) throw new Error('bad magic')
  if (buf[2] !== VERSION) throw new Error(`unsupported version ${buf[2]}`)
  const type = buf[3]!
  if (type !== TYPE_UPLINK && type !== TYPE_DOWNLINK) throw new Error(`unknown type ${type}`)
  const len = buf[11]!
  const max = type === TYPE_UPLINK ? MAX_UPLINK_PAYLOAD : DOWNLINK_PAYLOAD
  if (len > max) throw new Error(`payload too long (${len})`)
  if (buf.length !== HEADER_LEN + len + MAC_LEN) throw new Error('length mismatch')
  const dv = new DataView(buf.buffer, buf.byteOffset, buf.byteLength)
  const flags = buf[4]!
  const signed = buf.slice(0, HEADER_LEN + len)
  signed[4] = flags & ~0x06
  return {
    type,
    ack: (flags & FLAG_ACK) !== 0,
    oob: (flags & FLAG_OOB) !== 0,
    repeat: (flags >> 1) & 0x03,
    deviceId: formatDeviceId(dv.getUint32(5, true)),
    seq: dv.getUint16(9, true) % SEQ_MOD,
    payload: buf.slice(HEADER_LEN, HEADER_LEN + len),
    mac: buf.slice(HEADER_LEN + len),
    signed,
  }
}

export function encodeFrame(opts: {
  type: number
  deviceId: string
  seq: number
  payload: Uint8Array
  key: Uint8Array
  ack?: boolean
  oob?: boolean
  repeat?: number
}): Uint8Array {
  const len = opts.payload.length
  const max = opts.type === TYPE_UPLINK ? MAX_UPLINK_PAYLOAD : DOWNLINK_PAYLOAD
  if (len > max) throw new Error(`payload too long (${len} > ${max})`)
  const buf = new Uint8Array(HEADER_LEN + len + MAC_LEN)
  const dv = new DataView(buf.buffer)
  buf.set(MAGIC, 0)
  buf[2] = VERSION
  buf[3] = opts.type
  buf[4] = (opts.ack ? FLAG_ACK : 0) | (opts.oob ? FLAG_OOB : 0)
  dv.setUint32(5, parseDeviceId(opts.deviceId), true)
  dv.setUint16(9, opts.seq % SEQ_MOD, true)
  buf[11] = len
  buf.set(opts.payload, HEADER_LEN)
  buf.set(computeMac(opts.key, buf.subarray(0, HEADER_LEN + len)), HEADER_LEN + len)
  buf[4] = buf[4]! | (((opts.repeat ?? 0) & 0x03) << 1)
  return buf
}
