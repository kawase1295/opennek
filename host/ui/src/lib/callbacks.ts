// コールバックの編集フォームで使う値
import type { CallbackInput } from './api.gen'

export const DEFAULT_BODY =
  '{"device":"{device}","time":{time},"data":"{data}","seqNumber":{seqNumber},"rssi":{rssi},"ack":{ack}}'

// ヘッダーは "Name: value" の行で編集する
export const formatHeaders = (headers: CallbackInput['headers']): string =>
  Object.entries(headers)
    .map(([k, v]) => `${k}: ${String(v)}`)
    .join('\n')

export function parseHeaders(text: string): Record<string, string> {
  const headers: Record<string, string> = {}
  for (const line of text.split('\n')) {
    const i = line.indexOf(':')
    const name = line.slice(0, i).trim()
    if (i > 0 && name) headers[name] = line.slice(i + 1).trim()
  }
  return headers
}
