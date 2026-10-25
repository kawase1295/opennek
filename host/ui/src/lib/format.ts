// 表示用の書式

export const fmtTime = (ms: number | null | undefined): string => (ms ? new Date(ms).toLocaleString() : '—')
export const fmtClock = (ms: number): string => new Date(ms).toLocaleTimeString()

// 16 進を 1 バイトずつ区切る
export const spaced = (hex: string | null | undefined): string => (hex ? (hex.match(/../g) ?? []).join(' ') : '—')

export const decodedText = (decoded: Record<string, unknown> | null): string =>
  decoded
    ? Object.entries(decoded)
        .map(([k, v]) => `${k}=${String(v)}`)
        .join('  ')
    : ''

export const mhz = (hz: number, digits: number): string => `${(hz / 1e6).toFixed(digits)} MHz`
