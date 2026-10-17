import { Database } from 'bun:sqlite'
import { z } from 'zod/v4'
import { parsePayloadConfig } from './payload'

const hex = (bytes: number) => z.string().regex(new RegExp(`^[0-9a-fA-F]{${bytes * 2}}$`), `${bytes} バイトの 16 進`)

export const DeviceInput = z.object({
  id: z.string().regex(/^[0-9a-fA-F]{8}$/, '8 桁の 16 進').transform((s) => s.toUpperCase()),
  name: z.string().max(64).default(''),
  key: z.union([hex(16), z.literal('')]).default('').transform((s) => s.toLowerCase()),
  downlinkMode: z.enum(['none', 'static', 'callback']).default('none'),
  downlinkData: z.union([hex(8), z.literal('')]).default('').transform((s) => s.toLowerCase()),
  payloadConfig: z
    .string()
    .default('')
    .check((ctx) => {
      try {
        parsePayloadConfig(ctx.value)
      } catch (e) {
        ctx.issues.push({ code: 'custom', message: (e as Error).message, input: ctx.value })
      }
    }),
})
export type DeviceInput = z.infer<typeof DeviceInput>
export type Device = DeviceInput & { createdAt: number; lastSeen: number | null; lastSeq: number | null }

export const CallbackInput = z.object({
  name: z.string().max(64).default(''),
  enabled: z.boolean().default(true),
  devices: z.string().default(''), // カンマ区切りのデバイス ID。空なら全デバイス
  method: z.enum(['GET', 'POST', 'PUT']).default('POST'),
  url: z.url(),
  headers: z.record(z.string(), z.string()).default({}),
  contentType: z.string().default('application/json'),
  body: z.string().default(''),
  downlink: z.boolean().default(false), // レスポンスからダウンリンクデータを取り出す
})
export type CallbackInput = z.infer<typeof CallbackInput>
export type Callback = CallbackInput & { id: number }

export const Settings = z.object({
  channel: z.number().int().min(1).max(13).default(1),
  serialPort: z.string().default('auto'),
  dedupWindowMs: z.number().int().min(50).max(5000).default(400),
  downlinkTimeoutMs: z.number().int().min(500).max(30000).default(5000),
  acceptUnknownDevices: z.boolean().default(true), // 未登録デバイスのメッセージも表示・コールバックする
  sigfoxPort: z.string().default(''), // 本物の Sigfox を受ける RSSI ストリーマのポート。空なら使わない
  sigfoxFreq: z.number().int().min(862_000_000).max(928_000_000).default(923_200_000), // RC3 のアップリンク中心周波数
  sigfoxDownlink: z.boolean().default(false), // 本物の Sigfox デバイスへダウンリンクを送信する。電波を出すので明示的に有効にする
})
export type Settings = z.infer<typeof Settings>

export type DownlinkStatus = 'none' | 'pending' | 'sent' | 'failed' | 'nodata' | 'unsupported'
export type Radio = 'espnow' | 'sigfox'
// public: 公開鍵で検証できた本物の Sigfox。unverified: 本物の Sigfox で鍵が分からない
export type Auth = 'ok' | 'bad' | 'unknown' | 'public' | 'unverified'

export type Message = {
  id: number
  deviceId: string
  time: number // ms
  seq: number
  data: string
  ack: boolean
  oob: boolean
  rssi: number
  nbFrames: number
  auth: Auth
  src: string
  station: string
  radio: Radio // ESP-NOW (OpenNEK devices) or real Sigfox received over the air
  decoded: Record<string, unknown> | null
  downlink: string | null
  downlinkStatus: DownlinkStatus
}

export type CallbackLog = {
  id: number
  callbackId: number
  messageId: number
  time: number
  status: number | null
  ok: boolean
  error: string | null
  durationMs: number
  response: string | null
}

const SCHEMA = `
CREATE TABLE IF NOT EXISTS devices (
  id TEXT PRIMARY KEY, name TEXT NOT NULL DEFAULT '', key TEXT NOT NULL DEFAULT '',
  downlink_mode TEXT NOT NULL DEFAULT 'none', downlink_data TEXT NOT NULL DEFAULT '',
  payload_config TEXT NOT NULL DEFAULT '', created_at INTEGER NOT NULL, last_seen INTEGER, last_seq INTEGER
);
CREATE TABLE IF NOT EXISTS messages (
  id INTEGER PRIMARY KEY AUTOINCREMENT, device_id TEXT NOT NULL, time INTEGER NOT NULL, seq INTEGER NOT NULL,
  data TEXT NOT NULL, ack INTEGER NOT NULL, oob INTEGER NOT NULL, rssi INTEGER NOT NULL, nb_frames INTEGER NOT NULL,
  auth TEXT NOT NULL, src TEXT NOT NULL, station TEXT NOT NULL, decoded TEXT,
  downlink TEXT, downlink_status TEXT NOT NULL DEFAULT 'none', radio TEXT NOT NULL DEFAULT 'espnow'
);
CREATE INDEX IF NOT EXISTS messages_device ON messages (device_id, id);
CREATE TABLE IF NOT EXISTS callbacks (
  id INTEGER PRIMARY KEY AUTOINCREMENT, data TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS callback_logs (
  id INTEGER PRIMARY KEY AUTOINCREMENT, callback_id INTEGER NOT NULL, message_id INTEGER NOT NULL, time INTEGER NOT NULL,
  status INTEGER, ok INTEGER NOT NULL, error TEXT, duration_ms INTEGER NOT NULL, response TEXT
);
CREATE TABLE IF NOT EXISTS settings (id INTEGER PRIMARY KEY CHECK (id = 1), data TEXT NOT NULL);
`

const MAX_MESSAGES = 10000
const MAX_LOGS = 2000

type Row = Record<string, any>

const toDevice = (r: Row): Device => ({
  id: r.id,
  name: r.name,
  key: r.key,
  downlinkMode: r.downlink_mode,
  downlinkData: r.downlink_data,
  payloadConfig: r.payload_config,
  createdAt: r.created_at,
  lastSeen: r.last_seen,
  lastSeq: r.last_seq,
})

const toMessage = (r: Row): Message => ({
  id: r.id,
  deviceId: r.device_id,
  time: r.time,
  seq: r.seq,
  data: r.data,
  ack: !!r.ack,
  oob: !!r.oob,
  rssi: r.rssi,
  nbFrames: r.nb_frames,
  auth: r.auth,
  src: r.src,
  station: r.station,
  radio: r.radio,
  decoded: r.decoded ? JSON.parse(r.decoded) : null,
  downlink: r.downlink,
  downlinkStatus: r.downlink_status,
})

const toLog = (r: Row): CallbackLog => ({
  id: r.id,
  callbackId: r.callback_id,
  messageId: r.message_id,
  time: r.time,
  status: r.status,
  ok: !!r.ok,
  error: r.error,
  durationMs: r.duration_ms,
  response: r.response,
})

export class Store {
  readonly db: Database

  constructor(path: string) {
    this.db = new Database(path, { create: true, strict: true })
    this.db.run('PRAGMA journal_mode = WAL')
    this.db.run(SCHEMA)
    // 0.1.0 の DB には radio 列が無い
    const cols = this.db.query('PRAGMA table_info(messages)').all() as Row[]
    if (!cols.some((c) => c.name === 'radio')) this.db.run("ALTER TABLE messages ADD COLUMN radio TEXT NOT NULL DEFAULT 'espnow'")
  }

  // --- settings
  getSettings(): Settings {
    const row = this.db.query('SELECT data FROM settings WHERE id = 1').get() as Row | null
    return Settings.parse(row ? JSON.parse(row.data) : {})
  }
  saveSettings(s: Settings) {
    this.db.query('INSERT OR REPLACE INTO settings (id, data) VALUES (1, ?)').run(JSON.stringify(s))
  }

  // --- devices
  listDevices(): Device[] {
    return (this.db.query('SELECT * FROM devices ORDER BY id').all() as Row[]).map(toDevice)
  }
  getDevice(id: string): Device | null {
    const r = this.db.query('SELECT * FROM devices WHERE id = ?').get(id) as Row | null
    return r ? toDevice(r) : null
  }
  upsertDevice(d: DeviceInput): Device {
    this.db
      .query(
        `INSERT INTO devices (id, name, key, downlink_mode, downlink_data, payload_config, created_at)
         VALUES ($id, $name, $key, $mode, $data, $payload, $now)
         ON CONFLICT (id) DO UPDATE SET name = $name, key = $key, downlink_mode = $mode, downlink_data = $data, payload_config = $payload`,
      )
      .run({ id: d.id, name: d.name, key: d.key, mode: d.downlinkMode, data: d.downlinkData, payload: d.payloadConfig, now: Date.now() })
    return this.getDevice(d.id)!
  }
  deleteDevice(id: string) {
    this.db.query('DELETE FROM devices WHERE id = ?').run(id)
  }
  touchDevice(id: string, seq: number, time: number) {
    this.db.query('UPDATE devices SET last_seen = ?, last_seq = ? WHERE id = ?').run(time, seq, id)
  }

  // --- messages
  insertMessage(m: Omit<Message, 'id'>): Message {
    const r = this.db
      .query(
        `INSERT INTO messages (device_id, time, seq, data, ack, oob, rssi, nb_frames, auth, src, station, decoded, downlink, downlink_status, radio)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING *`,
      )
      .get(
        m.deviceId, m.time, m.seq, m.data, m.ack ? 1 : 0, m.oob ? 1 : 0, m.rssi, m.nbFrames, m.auth,
        m.src, m.station, m.decoded ? JSON.stringify(m.decoded) : null, m.downlink, m.downlinkStatus, m.radio,
      ) as Row
    this.db.query('DELETE FROM messages WHERE id <= ?').run(r.id - MAX_MESSAGES)
    return toMessage(r)
  }
  updateMessage(id: number, patch: Partial<Pick<Message, 'downlink' | 'downlinkStatus' | 'decoded'>>): Message | null {
    const cur = this.getMessage(id)
    if (!cur) return null
    const next = { ...cur, ...patch }
    this.db
      .query('UPDATE messages SET downlink = ?, downlink_status = ?, decoded = ? WHERE id = ?')
      .run(next.downlink, next.downlinkStatus, next.decoded ? JSON.stringify(next.decoded) : null, id)
    return next
  }
  getMessage(id: number): Message | null {
    const r = this.db.query('SELECT * FROM messages WHERE id = ?').get(id) as Row | null
    return r ? toMessage(r) : null
  }
  listMessages(opts: { deviceId?: string; before?: number; limit?: number } = {}): Message[] {
    const limit = Math.min(opts.limit ?? 100, 1000)
    const rows = this.db
      .query(
        `SELECT * FROM messages WHERE ($dev IS NULL OR device_id = $dev) AND ($before IS NULL OR id < $before)
         ORDER BY id DESC LIMIT $limit`,
      )
      .all({ dev: opts.deviceId ?? null, before: opts.before ?? null, limit }) as Row[]
    return rows.map(toMessage)
  }
  clearMessages(deviceId?: string) {
    if (deviceId) this.db.query('DELETE FROM messages WHERE device_id = ?').run(deviceId)
    else this.db.run('DELETE FROM messages')
  }

  // --- callbacks
  listCallbacks(): Callback[] {
    return (this.db.query('SELECT * FROM callbacks ORDER BY id').all() as Row[]).map((r) => ({
      ...CallbackInput.parse(JSON.parse(r.data)),
      id: r.id,
    }))
  }
  getCallback(id: number): Callback | null {
    return this.listCallbacks().find((c) => c.id === id) ?? null
  }
  createCallback(c: CallbackInput): Callback {
    const r = this.db.query('INSERT INTO callbacks (data) VALUES (?) RETURNING id').get(JSON.stringify(c)) as Row
    return { ...c, id: r.id }
  }
  updateCallback(id: number, c: CallbackInput): Callback | null {
    const res = this.db.query('UPDATE callbacks SET data = ? WHERE id = ?').run(JSON.stringify(c), id)
    return res.changes ? { ...c, id } : null
  }
  deleteCallback(id: number) {
    this.db.query('DELETE FROM callbacks WHERE id = ?').run(id)
  }

  // --- callback logs
  insertLog(l: Omit<CallbackLog, 'id'>): CallbackLog {
    const r = this.db
      .query(
        `INSERT INTO callback_logs (callback_id, message_id, time, status, ok, error, duration_ms, response)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?) RETURNING *`,
      )
      .get(l.callbackId, l.messageId, l.time, l.status, l.ok ? 1 : 0, l.error, l.durationMs, l.response) as Row
    this.db.query('DELETE FROM callback_logs WHERE id <= ?').run(r.id - MAX_LOGS)
    return toLog(r)
  }
  listLogs(opts: { callbackId?: number; messageId?: number; limit?: number } = {}): CallbackLog[] {
    const rows = this.db
      .query(
        `SELECT * FROM callback_logs WHERE ($cb IS NULL OR callback_id = $cb) AND ($msg IS NULL OR message_id = $msg)
         ORDER BY id DESC LIMIT $limit`,
      )
      .all({ cb: opts.callbackId ?? null, msg: opts.messageId ?? null, limit: Math.min(opts.limit ?? 100, 1000) }) as Row[]
    return rows.map(toLog)
  }
}
