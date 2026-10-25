// コールバックの登録フォームと一覧で使う値。種類・チャンネル・項目は Sigfox Backend と同じ
// (ホストの store.rs の CallbackInput と同じ決まり)
import type { CallbackInput, CallbackSubtype, CallbackType } from './api.gen'
import { t } from './i18n'

export const TYPES: readonly CallbackType[] = ['DATA', 'SERVICE', 'ERROR']
export const SUBTYPES: Record<CallbackType, readonly CallbackSubtype[]> = {
  DATA: ['UPLINK', 'BIDIR'],
  SERVICE: ['STATUS', 'ACKNOWLEDGE', 'REPEATER', 'DATA_ADVANCED'],
  ERROR: [],
}

export const DEFAULT_BODY =
  '{"device":"{device}","time":{time},"data":"{data}","seqNumber":{seqNumber},"rssi":{rssi},"ack":{ack}}'

export function blankCallback(): CallbackInput {
  return {
    name: '',
    callbackType: 'DATA',
    callbackSubtype: 'UPLINK',
    channel: 'URL',
    enabled: true,
    sendDuplicate: false,
    payloadConfig: '',
    url: '',
    httpMethod: 'POST',
    // the Backend leaves it off, but most HTTPS servers need it
    sendSni: true,
    headers: {},
    contentType: 'application/json',
    bodyTemplate: DEFAULT_BODY,
    linePattern: '',
    recipient: '',
    subject: '',
    message: '',
    downlink: false,
    devices: '',
  }
}

// 種類を変えたらその最初の Subtype にする。ダウンリンクに選べるのは DATA BIDIR だけ
export function withType(c: CallbackInput, type: CallbackType): CallbackInput {
  return { ...c, callbackType: type, callbackSubtype: SUBTYPES[type][0] ?? null, downlink: false }
}

export function withSubtype(c: CallbackInput, subtype: CallbackSubtype): CallbackInput {
  return { ...c, callbackSubtype: subtype, downlink: false }
}

// OpenNEK が送るのは DATA を URL へ。ほかは Backend と同じ形で保存だけする
export const isSent = (c: Pick<CallbackInput, 'callbackType' | 'channel'>): boolean =>
  c.callbackType === 'DATA' && c.channel === 'URL'

// 一覧の Information 列
export function information(c: CallbackInput): string {
  switch (c.channel) {
    case 'URL':
      return `${c.httpMethod} ${c.url}`
    case 'BATCH_URL':
      return c.url
    case 'EMAIL':
      return `${c.recipient} · ${c.subject}`
  }
}

export type HeaderRow = { name: string; value: string }

export const rowsOf = (headers: CallbackInput['headers']): HeaderRow[] =>
  Object.entries(headers).map(([name, value]) => ({ name, value: String(value) }))

export function headersOf(rows: readonly HeaderRow[]): Record<string, string> {
  const headers: Record<string, string> = {}
  for (const r of rows) if (r.name.trim()) headers[r.name.trim()] = r.value
  return headers
}

// 複製は、ダウンリンクには選ばれていない状態で作る (Backend の BIDIR と同じ)
export const copyOf = (c: CallbackInput): CallbackInput => ({
  ...c,
  name: c.name ? `${c.name} ${t('cb.copy')}` : t('cb.copy'),
  downlink: false,
})
