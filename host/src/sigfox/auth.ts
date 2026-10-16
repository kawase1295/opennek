// Uplink authentication (UL-AUTH) as computed by sigfox-ep-lib (_aes_encrypt in sigfox_ep_bitstream.c):
// AES-128-CBC with a zero IV over the LI/BF/REP/MC, EP ID and payload bytes repeated to fill 16-byte blocks
// (two blocks for 11- and 12-byte payloads); the field is the first 2-5 bytes of the last cipher block.
import { createCipheriv } from 'node:crypto'
import { uplinkHeader } from './frame'

// SIGFOX_EP_PUBLIC_KEY, used by devices switched to public key mode (e.g. ATS410=1)
export const PUBLIC_KEY: Uint8Array = Uint8Array.from(Buffer.from('00112233445566778899aabbccddeeff', 'hex'))

export type AuthInput = { deviceId: string; counter: number; payload: Uint8Array; bit: 0 | 1 | null; downlink: boolean }

export function aesBlocks(key: Uint8Array, data: Uint8Array) {
  const c = createCipheriv('aes-128-cbc', key, new Uint8Array(16))
  c.setAutoPadding(false)
  return new Uint8Array(Buffer.concat([c.update(data), c.final()]))
}

export function uplinkAuth(key: Uint8Array, f: AuthInput) {
  const { bytes, authSize } = uplinkHeader(f)
  const blocks = f.payload.length >= 11 ? 2 : 1
  const input = Uint8Array.from({ length: 16 * blocks }, (_, i) => bytes[i % bytes.length]!)
  return aesBlocks(key, input).slice(16 * (blocks - 1), 16 * (blocks - 1) + authSize)
}

export function verifyUplink(key: Uint8Array, f: AuthInput & { auth: Uint8Array }) {
  const expected = uplinkAuth(key, f)
  return expected.length === f.auth.length && expected.every((b, i) => b === f.auth[i])
}
