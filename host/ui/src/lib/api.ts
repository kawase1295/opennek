// The host's HTTP API. The types come from the host (api.gen.ts), so a change there fails `npm run check`
import type {
  Callback,
  CallbackInput,
  CallbackLog,
  ConsoleLine,
  Device,
  Event,
  Message,
  PortInfo,
  Settings,
  Status,
  TestResult,
} from './api.gen'

// What PUT /api/devices/{id} takes
export type DeviceInput = Pick<Device, 'name' | 'key' | 'downlinkMode' | 'downlinkData' | 'payloadConfig'>

type Method = 'GET' | 'POST' | 'PUT' | 'DELETE'

async function request<T>(method: Method, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`/api${path}`, {
    method,
    ...(body === undefined ? {} : { headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) }),
  })
  if (res.status === 204) return undefined as T
  const json: unknown = await res.json().catch(() => ({}))
  if (!res.ok) {
    const error = typeof json === 'object' && json && 'error' in json ? String(json.error) : `HTTP ${res.status}`
    throw new Error(error)
  }
  // the host's responses are the types it generates
  return json as T
}

const query = (q: Record<string, string | number | undefined>): string => {
  const params = new URLSearchParams()
  for (const [k, v] of Object.entries(q)) if (v !== undefined && v !== '') params.set(k, String(v))
  const s = params.toString()
  return s ? `?${s}` : ''
}

export const api = {
  status: () => request<Status>('GET', '/status'),
  settings: () => request<Settings>('GET', '/settings'),
  saveSettings: (patch: Partial<Settings>) => request<Settings>('PUT', '/settings', patch),
  serialPorts: () => request<PortInfo[]>('GET', '/serial/ports'),

  devices: () => request<Device[]>('GET', '/devices'),
  saveDevice: (id: string, input: DeviceInput) => request<Device>('PUT', `/devices/${id}`, input),
  deleteDevice: (id: string) => request<void>('DELETE', `/devices/${id}`),

  messages: (q: { device?: string; before?: number; limit: number }) =>
    request<Message[]>('GET', `/messages${query(q)}`),
  clearMessages: (device: string) => request<void>('DELETE', `/messages${query({ device })}`),
  messagesCsvUrl: (device: string) => `/api/messages.csv${query({ device })}`,

  callbacks: () => request<Callback[]>('GET', '/callbacks'),
  saveCallback: (id: number | null, input: CallbackInput) =>
    id === null
      ? request<Callback>('POST', '/callbacks', input)
      : request<Callback>('PUT', `/callbacks/${id}`, input),
  deleteCallback: (id: number) => request<void>('DELETE', `/callbacks/${id}`),
  testCallback: (id: number) => request<TestResult>('POST', `/callbacks/${id}/test`),
  callbackLogs: (q: { message?: number; limit?: number }) => request<CallbackLog[]>('GET', `/callback-logs${query(q)}`),

  hostLog: () => request<ConsoleLine[]>('GET', '/logs'),
}

// GET /api/events: one SSE event per Event, named by its type
export function subscribe(handlers: {
  open: () => void
  error: () => void
  event: (e: Event) => void
}): EventSource {
  const es = new EventSource('/api/events')
  es.addEventListener('open', handlers.open)
  es.addEventListener('error', handlers.error)
  for (const type of ['message', 'radio', 'console', 'callbackLog'] satisfies Event['type'][]) {
    es.addEventListener(type, (e: MessageEvent<string>) => handlers.event(JSON.parse(e.data) as Event))
  }
  return es
}
