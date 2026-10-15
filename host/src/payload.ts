// カスタムペイロード設定 ("名前:位置:型:引数" を空白区切り) でバイト列をデコードする
export type FieldValue = number | boolean | string

type Field =
  | { name: string; offset?: number; type: 'uint' | 'int'; bits: number; little: boolean }
  | { name: string; offset?: number; type: 'float'; bits: 32 | 64; little: boolean }
  | { name: string; offset: number; type: 'bool'; bit: number }
  | { name: string; offset?: number; type: 'char'; length: number }

export function parsePayloadConfig(config: string): Field[] {
  return config
    .trim()
    .split(/\s+/)
    .filter(Boolean)
    .map((token) => {
      const [name, offsetStr, type, ...args] = token.split(':')
      if (!name || !/^[A-Za-z_][A-Za-z0-9_]*$/.test(name)) throw new Error(`invalid field name in "${token}"`)
      const offset = offsetStr ? Number(offsetStr) : undefined
      if (offset !== undefined && !(Number.isInteger(offset) && offset >= 0)) throw new Error(`invalid offset in "${token}"`)
      const little = args[1] === 'little-endian'
      if (args[1] && !little && args[1] !== 'big-endian') throw new Error(`invalid endianness in "${token}"`)
      const n = Number(args[0])
      switch (type) {
        case 'uint':
        case 'int':
          if (!Number.isInteger(n) || n < 1 || n > 32) throw new Error(`bits must be 1..32 in "${token}"`)
          if (little && n % 8 !== 0) throw new Error(`little-endian needs whole bytes in "${token}"`)
          return { name, offset, type, bits: n, little }
        case 'float':
          if (n !== 32 && n !== 64) throw new Error(`float must be 32 or 64 in "${token}"`)
          return { name, offset, type, bits: n, little }
        case 'bool':
          if (offset === undefined) throw new Error(`bool needs an explicit byte offset in "${token}"`)
          if (!Number.isInteger(n) || n < 0 || n > 7) throw new Error(`bool bit must be 0..7 in "${token}"`)
          return { name, offset, type, bit: n }
        case 'char':
          if (!Number.isInteger(n) || n < 1) throw new Error(`char length must be >= 1 in "${token}"`)
          return { name, offset, type, length: n }
        default:
          throw new Error(`unknown type "${type}" in "${token}"`)
      }
    })
}

export function decodePayload(config: string, data: Uint8Array): Record<string, FieldValue> {
  const out: Record<string, FieldValue> = {}
  const dv = new DataView(data.buffer, data.byteOffset, data.byteLength)
  // ビット単位のカーソル (uint/int は 8 の倍数でなくてもよい)
  let cursor = 0
  for (const f of parsePayloadConfig(config)) {
    const start = f.offset !== undefined ? f.offset * 8 : cursor
    const need = (bits: number) => {
      if (start + bits > data.length * 8) throw new Error(`field "${f.name}" exceeds payload`)
    }
    switch (f.type) {
      case 'bool':
        need(8)
        out[f.name] = ((data[f.offset]! >> f.bit) & 1) === 1
        continue
      case 'char': {
        if (start % 8) throw new Error(`field "${f.name}" is not byte aligned`)
        need(f.length * 8)
        out[f.name] = new TextDecoder().decode(data.subarray(start / 8, start / 8 + f.length))
        cursor = start + f.length * 8
        continue
      }
      case 'float': {
        if (start % 8) throw new Error(`field "${f.name}" is not byte aligned`)
        need(f.bits)
        out[f.name] = f.bits === 32 ? dv.getFloat32(start / 8, f.little) : dv.getFloat64(start / 8, f.little)
        cursor = start + f.bits
        continue
      }
      default: {
        need(f.bits)
        if (f.little && start % 8) throw new Error(`field "${f.name}" is not byte aligned`)
        let v = 0n
        if (f.little) {
          for (let i = f.bits / 8 - 1; i >= 0; i--) v = (v << 8n) | BigInt(data[start / 8 + i]!)
        } else {
          for (let b = start; b < start + f.bits; b++) v = (v << 1n) | BigInt((data[b >> 3]! >> (7 - (b & 7))) & 1)
        }
        if (f.type === 'int' && v >> BigInt(f.bits - 1)) v -= 1n << BigInt(f.bits)
        out[f.name] = Number(v)
        cursor = start + f.bits
      }
    }
  }
  return out
}
