import type { Callback, Message } from './db'

// Sigfox のカスタムコールバック互換の変数を埋め込む ({device}, {data}, {customData#temp} など)
export function templateVars(m: Message, deviceName: string): Record<string, string> {
  const vars: Record<string, string> = {
    device: m.deviceId,
    deviceName,
    time: String(Math.floor(m.time / 1000)),
    data: m.data,
    seqNumber: String(m.seq),
    ack: String(m.ack),
    oob: String(m.oob),
    rssi: String(m.rssi),
    snr: '0',
    station: m.station,
    nbFrames: String(m.nbFrames),
  }
  for (const [k, v] of Object.entries(m.decoded ?? {})) vars[`customData#${k}`] = String(v)
  return vars
}

export const renderTemplate = (tpl: string, vars: Record<string, string>, encode: (s: string) => string = (s) => s) =>
  tpl.replace(/\{([A-Za-z]+(?:#[A-Za-z0-9_]+)?)\}/g, (all, name: string) => (name in vars ? encode(vars[name]!) : all))

export const appliesTo = (cb: Callback, deviceId: string) => {
  const list = cb.devices.split(',').map((s) => s.trim().toUpperCase()).filter(Boolean)
  return list.length === 0 || list.includes(deviceId)
}

export type CallbackResult = {
  status: number | null
  ok: boolean
  error: string | null
  durationMs: number
  response: string | null
  downlink: string | null // ダウンリンクデータを返したときだけ 16 進 8 バイト
}

const MAX_RESPONSE_LOG = 2000

export function extractDownlink(body: string, deviceId: string): string | null {
  let json: unknown
  try {
    json = JSON.parse(body)
  } catch {
    return null
  }
  if (!json || typeof json !== 'object') return null
  const obj = json as Record<string, any>
  const data = obj[deviceId]?.downlinkData ?? obj[deviceId.toLowerCase()]?.downlinkData ?? obj.downlinkData
  return typeof data === 'string' && /^[0-9a-fA-F]{16}$/.test(data) ? data.toLowerCase() : null
}

export async function runCallback(cb: Callback, m: Message, deviceName: string, timeoutMs: number): Promise<CallbackResult> {
  const vars = templateVars(m, deviceName)
  const started = performance.now()
  const elapsed = () => Math.round(performance.now() - started)
  try {
    const url = renderTemplate(cb.url, vars, encodeURIComponent)
    const headers: Record<string, string> = {}
    for (const [k, v] of Object.entries(cb.headers)) headers[k] = renderTemplate(v, vars)
    const hasBody = cb.method !== 'GET'
    if (hasBody) headers['Content-Type'] ??= cb.contentType
    const res = await fetch(url, {
      method: cb.method,
      headers,
      body: hasBody ? renderTemplate(cb.body, vars) : undefined,
      signal: AbortSignal.timeout(timeoutMs),
    })
    const text = await res.text()
    return {
      status: res.status,
      ok: res.ok,
      error: res.ok ? null : `HTTP ${res.status}`,
      durationMs: elapsed(),
      response: text.slice(0, MAX_RESPONSE_LOG),
      downlink: cb.downlink && res.ok && res.status !== 204 ? extractDownlink(text, m.deviceId) : null,
    }
  } catch (e) {
    const error = e instanceof Error ? (e.name === 'TimeoutError' ? `timeout (${timeoutMs} ms)` : e.message) : String(e)
    return { status: null, ok: false, error, durationMs: elapsed(), response: null, downlink: null }
  }
}
