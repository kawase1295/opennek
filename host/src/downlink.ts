// 固定ダウンリンクのテンプレート。Sigfox 系エミュレータと同じ変数を 16 進の間に埋め込める
export const DOWNLINK_BYTES = 8

export type DownlinkContext = { time: number; station: string; rssi: number }

const VARIABLES = {
  time: { bytes: 4, render: (c: DownlinkContext) => u32(Math.floor(c.time / 1000)) },
  tapid: { bytes: 4, render: (c: DownlinkContext) => u32(stationId(c.station)) },
  rssi: { bytes: 2, render: (c: DownlinkContext) => i16(c.rssi) },
} as const
type Variable = keyof typeof VARIABLES

export type DownlinkPart = { hex: string } | { variable: Variable }

const u32 = (n: number) => (n >>> 0).toString(16).padStart(8, '0')
const i16 = (n: number) => (Math.max(-0x8000, Math.min(0x7fff, Math.round(n))) & 0xffff).toString(16).padStart(4, '0')
// station は 16 進の文字列 (Sigfox は '0000'、ESP-NOW はゲートウェイ MAC の下位 4 バイト)。読めなければ 0
const stationId = (s: string) => (/^[0-9a-fA-F]{1,8}$/.test(s) ? parseInt(s, 16) : 0)

export function parseDownlinkTemplate(template: string): DownlinkPart[] {
  const parts: DownlinkPart[] = []
  let bytes = 0
  for (const [, hex, name] of template.matchAll(/([^{]+)|\{([^}]*)\}/g)) {
    if (hex !== undefined) {
      if (!/^[0-9a-fA-F]*$/.test(hex)) throw new Error(`not hex: ${hex}`)
      if (hex.length % 2) throw new Error(`hex needs an even number of digits: ${hex}`)
      parts.push({ hex: hex.toLowerCase() })
      bytes += hex.length / 2
    } else {
      if (!Object.hasOwn(VARIABLES, name!)) throw new Error(`unknown variable {${name}} (use {time}, {tapid} or {rssi})`)
      parts.push({ variable: name as Variable })
      bytes += VARIABLES[name as Variable].bytes
    }
  }
  // 閉じていない { は matchAll で拾われずに消えるので、元の文字列と突き合わせる
  const consumed = parts.map((p) => ('hex' in p ? p.hex : `{${p.variable}}`)).join('')
  if (consumed.length !== template.length) throw new Error(`not hex: ${template}`)
  if (bytes !== DOWNLINK_BYTES) throw new Error(`must be ${DOWNLINK_BYTES} bytes (got ${bytes})`)
  return parts
}

export function renderDownlink(template: string, ctx: DownlinkContext): string {
  return parseDownlinkTemplate(template)
    .map((p) => ('hex' in p ? p.hex : VARIABLES[p.variable].render(ctx)))
    .join('')
}
