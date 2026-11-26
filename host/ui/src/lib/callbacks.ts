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
export const ACKNOWLEDGE_BODY =
  '{"device":"{device}","time":{time},"infoCode":{infoCode},"infoMessage":"{infoMessage}","downlinkAck":{downlinkAck},"downlinkOverusage":{downlinkOverusage}}'

// 変数を入れるボタン。ACKNOWLEDGE の変数は Backend と同じくこの 6 つだけ (ホストの acknowledge.rs)
const DATA_VARIABLES = [
  '{device}', '{deviceName}', '{time}', '{data}', '{seqNumber}', '{ack}', '{oob}',
  '{rssi}', '{snr}', '{station}', '{nbFrames}', '{customData#name}',
]
const ACKNOWLEDGE_VARIABLES = ['{device}', '{time}', '{infoCode}', '{infoMessage}', '{downlinkAck}', '{downlinkOverusage}']

const isAcknowledge = (c: Pick<CallbackInput, 'callbackType' | 'callbackSubtype'>): boolean =>
  c.callbackType === 'SERVICE' && c.callbackSubtype === 'ACKNOWLEDGE'

export const variablesOf = (c: Pick<CallbackInput, 'callbackType' | 'callbackSubtype'>): readonly string[] =>
  isAcknowledge(c) ? ACKNOWLEDGE_VARIABLES : DATA_VARIABLES

const defaultBody = (c: Pick<CallbackInput, 'callbackType' | 'callbackSubtype'>): string =>
  isAcknowledge(c) ? ACKNOWLEDGE_BODY : DEFAULT_BODY

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

// 本文が初期値のままなら、変えた先の種類の初期値にする
function retemplated(before: CallbackInput, after: CallbackInput): CallbackInput {
  return before.bodyTemplate === defaultBody(before) ? { ...after, bodyTemplate: defaultBody(after) } : after
}

// 種類を変えたらその最初の Subtype にする。ダウンリンクに選べるのは DATA BIDIR だけ
export function withType(c: CallbackInput, type: CallbackType): CallbackInput {
  return retemplated(c, { ...c, callbackType: type, callbackSubtype: SUBTYPES[type][0] ?? null, downlink: false })
}

export function withSubtype(c: CallbackInput, subtype: CallbackSubtype): CallbackInput {
  return retemplated(c, { ...c, callbackSubtype: subtype, downlink: false })
}

// OpenNEK が送るのは DATA と SERVICE ACKNOWLEDGE を URL へ。ほかは Backend と同じ形で保存だけする
// (ホストの CallbackInput::trigger と同じ決まり)
export const isSent = (c: Pick<CallbackInput, 'callbackType' | 'callbackSubtype' | 'channel'>): boolean =>
  c.channel === 'URL' && (c.callbackType === 'DATA' || isAcknowledge(c))

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
