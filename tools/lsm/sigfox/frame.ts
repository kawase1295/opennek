// Sigfox uplink frame layout, as built by sigfox-ep-lib (github.com/sigfox-tech-radio/sigfox-ep-lib):
//   PR (0xAAAA) | FT (16 bit) | LI(2) BF(1) REP(1) MC(12) | EP ID (4, LSB first) | payload | auth | CRC16
// Repetitions 2 and 3 (ranks 1 and 2 here) convolve everything after FT.

export const PREAMBLE = 0xaaaa
// Frame types per repetition rank, indexed by payload class: empty/bit, 1 byte, 2-4, 5-8, 9-12 bytes
export const FRAME_TYPES = [
  [0xa06b, 0xa08d, 0xa35f, 0xa611, 0xa94c],
  [0xa6e0, 0xa0d2, 0xa598, 0xa6bf, 0xa971],
  [0xa034, 0xa302, 0xa5a3, 0xa72c, 0xa997],
] as const
// Control frames (keep-alive, downlink confirmation): 7 or 8 bytes, laid out like payload class 3
export const CONTROL_FRAME_TYPES = [0xaf67, 0xafc9, 0xb1be] as const
const CONTROL_CLASS = 3
const CONV_NEXT = [0, 0, 1, 1]
const CONV_OUT = [
  [0, 0, 0, 0],
  [0, 1, 1, 0],
  [0, 1, 0, 1],
]

export type UplinkFrame = {
  rank: number // 0, 1, 2 = first, second, third repetition
  deviceId: string // 8 hex digits, as printed by AT$ID
  counter: number
  payload: Uint8Array
  bit: 0 | 1 | null // single-bit messages (AT$SB)
  downlink: boolean // BF: the device waits for a downlink
  control: boolean // a control frame (see decodeControl), not application data
  auth: Uint8Array
  crcOk: boolean
  repaired: string | null // description of the bit-slip correction that made the CRC pass, if any
}

export function crc16(data: Uint8Array) {
  let c = 0
  for (const b of data) {
    c ^= b << 8
    for (let i = 0; i < 8; i++) c = c & 0x8000 ? ((c << 1) ^ 0x1021) & 0xffff : (c << 1) & 0xffff
  }
  return c ^ 0xffff
}

const bitsOf = (v: number, n: number) => v.toString(2).padStart(n, '0')
const toBytes = (bits: string) => {
  const out = new Uint8Array(bits.length >> 3)
  for (let i = 0; i < out.length; i++) out[i] = Number.parseInt(bits.slice(i * 8, i * 8 + 8), 2)
  return out
}

export function convolve(bits: string, rank: number) {
  let state = 0
  let out = ''
  for (const b of bits) {
    const input = b === '1' ? 1 : 0
    out += String(input ^ CONV_OUT[rank]![state]!)
    state = CONV_NEXT[state]! + (input ? 2 : 0)
  }
  return out
}

export function deconvolve(bits: string, rank: number) {
  let state = 0
  let out = ''
  for (const b of bits) {
    const input = (b === '1' ? 1 : 0) ^ CONV_OUT[rank]![state]!
    out += String(input)
    state = CONV_NEXT[state]! + (input ? 2 : 0)
  }
  return out
}

// Payload size, auth size and LI for a payload class and the LI read from the frame
function sizes(cls: number, li: number) {
  if (cls === 0) return { payload: 0, auth: 2 }
  if (cls === 1) return { payload: 1, auth: 2 }
  return { payload: 4 * (cls - 1) - li, auth: li + 2 }
}

// LI/BF/REP/MC, EP ID (LSB first) and payload: the part of an uplink covered by the authentication field
export function uplinkHeader(opts: { deviceId: string; counter: number; payload: Uint8Array; downlink?: boolean; bit?: 0 | 1 | null }) {
  const n = opts.payload.length
  const bit = opts.bit ?? undefined
  const cls = bit !== undefined || n === 0 ? 0 : n === 1 ? 1 : Math.floor((n - 1) / 4) + 2
  const li = bit !== undefined ? 2 + bit : cls >= 2 ? 3 - ((n - 1) % 4) : 0
  const id = Number.parseInt(opts.deviceId, 16)
  const out = new Uint8Array(2 + 4 + n)
  out[0] = (li << 6) | ((opts.downlink ? 1 : 0) << 5) | ((opts.counter >> 8) & 0x0f)
  out[1] = opts.counter & 0xff
  for (let i = 0; i < 4; i++) out[2 + i] = (id >>> (8 * i)) & 0xff
  out.set(opts.payload, 6)
  return { bytes: out, cls, authSize: cls >= 2 ? li + 2 : 2 }
}

// Builds the data bits of an uplink frame (used by tests)
export function encodeUplink(opts: {
  rank: number
  deviceId: string
  counter: number
  payload: Uint8Array
  auth: Uint8Array
  downlink?: boolean
  bit?: 0 | 1
  control?: boolean
}) {
  const header = uplinkHeader(opts)
  const body = new Uint8Array(header.bytes.length + opts.auth.length)
  body.set(header.bytes)
  body.set(opts.auth, header.bytes.length)
  const crc = crc16(body)
  let bits = [...body].map((b) => bitsOf(b, 8)).join('') + bitsOf(crc, 16)
  if (opts.rank > 0) bits = convolve(bits, opts.rank)
  const ft = opts.control ? CONTROL_FRAME_TYPES[opts.rank]! : FRAME_TYPES[opts.rank]![header.cls]!
  return bitsOf(PREAMBLE, 16) + bitsOf(ft, 16) + bits
}

function parseBody(bodyBits: string, rank: number, cls: number, control: boolean, repaired: string | null): UplinkFrame | null {
  const plain = rank > 0 ? deconvolve(bodyBits, rank) : bodyBits
  if (plain.length < 16) return null
  const li = Number.parseInt(plain.slice(0, 2), 2)
  const { payload, auth } = sizes(cls, li)
  if (payload < 0) return null
  const bodyBytes = 2 + 4 + payload + auth
  const total = (bodyBytes + 2) * 8
  if (plain.length < total) return null
  const bytes = toBytes(plain.slice(0, total))
  const body = bytes.subarray(0, bodyBytes)
  const crcOk = crc16(body) === ((bytes[bodyBytes]! << 8) | bytes[bodyBytes + 1]!)
  const id = (body[2]! | (body[3]! << 8) | (body[4]! << 16) | (body[5]! << 24)) >>> 0
  return {
    rank,
    deviceId: id.toString(16).toUpperCase().padStart(8, '0'),
    counter: ((body[0]! & 0x0f) << 8) | body[1]!,
    payload: body.slice(6, 6 + payload),
    bit: cls === 0 && li >= 2 ? ((li - 2) as 0 | 1) : null,
    downlink: (body[0]! & 0x20) !== 0,
    control,
    auth: body.slice(6 + payload, 6 + payload + auth),
    crcOk,
    repaired,
  }
}

// Decodes data bits that start somewhere in the preamble. The frame type is searched near where it should
// be, because the first preamble bits are not observable and the leading edge can add a spurious bit.
// When the CRC fails, single slipped, extra or flipped bits are tried (the receiver clock drifts).
export function decodeUplinkBits(bits: string): UplinkFrame | null {
  for (let start = 8; start <= 22 && start + 16 <= bits.length; start++) {
    const ft = Number.parseInt(bits.slice(start, start + 16), 2)
    for (let rank = 0; rank < 3; rank++) {
      const control = ft === CONTROL_FRAME_TYPES[rank]
      const cls = control ? CONTROL_CLASS : FRAME_TYPES[rank]!.indexOf(ft as never)
      if (cls < 0) continue
      const body = bits.slice(start + 16)
      const frame = parseBody(body, rank, cls, control, null)
      if (frame?.crcOk) return frame
      // One bit lost or gained somewhere (clock slip), or one bit wrong
      for (let i = 0; i < body.length; i++) {
        const candidates: [string, string][] = [
          [body.slice(0, i) + '1' + body.slice(i), `inserted 1 at ${i}`],
          [body.slice(0, i) + '0' + body.slice(i), `inserted 0 at ${i}`],
          [body.slice(0, i) + body.slice(i + 1), `deleted bit ${i}`],
          [body.slice(0, i) + (body[i] === '1' ? '0' : '1') + body.slice(i + 1), `flipped bit ${i}`],
        ]
        for (const [candidate, how] of candidates) {
          const fixed = parseBody(candidate, rank, cls, control, how)
          if (fixed?.crcOk) return fixed
        }
      }
      return frame
    }
  }
  return null
}

// Payload of a control frame (sigfox-ep-lib SIGFOX_EP_BITSTREAM_build_control_frame): a type byte, then little-endian
// voltages and temperature; a downlink confirmation adds the RSSI at which the device received the downlink
export type ControlMessage = {
  type: 'keepAlive' | 'downlinkConfirmation'
  voltageIdleMv: number
  voltageTxMv: number
  temperatureC: number
  downlinkRssiDbm?: number
}

export function decodeControl(payload: Uint8Array): ControlMessage | null {
  const type = payload[0] === 0x08 && payload.length === 7 ? 'keepAlive' : payload[0] === 0x09 && payload.length === 8 ? 'downlinkConfirmation' : null
  if (!type) return null
  const v = new DataView(payload.buffer, payload.byteOffset, payload.byteLength)
  const msg: ControlMessage = {
    type,
    voltageIdleMv: v.getUint16(1, true),
    voltageTxMv: v.getUint16(3, true),
    temperatureC: v.getInt16(5, true) / 10,
  }
  if (type === 'downlinkConfirmation') msg.downlinkRssiDbm = v.getInt8(7) - 100
  return msg
}
