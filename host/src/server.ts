import { randomBytes } from 'node:crypto'
import { Hono } from 'hono'
import { streamSSE } from 'hono/streaming'
import { z } from 'zod/v4'
import { CallbackInput, DeviceInput, Settings } from './db'
import { appliesTo, runCallback } from './callbacks'
import type { Engine, EngineEvent } from './engine'
import { fromHex } from './frame'
import { listSerialPorts, SerialGateway } from './gateway/serial'
import { SimGateway } from './gateway/sim'
import { VERSION } from './version'
import appJs from './ui/app.js' with { type: 'text' }
import i18nJs from './ui/i18n.js' with { type: 'text' }
import indexHtml from './ui/index.html' with { type: 'text' }
import styleCss from './ui/style.css' with { type: 'text' }

const parse = <T>(schema: z.ZodType<T>, body: unknown) => {
  const r = schema.safeParse(body)
  if (r.success) return { ok: true as const, data: r.data }
  return { ok: false as const, error: r.error.issues.map((i) => `${i.path.join('.') || 'body'}: ${i.message}`).join('; ') }
}

const SimUplink = z.object({
  deviceId: z.string().regex(/^[0-9a-fA-F]{8}$/).transform((s) => s.toUpperCase()).default('F0000001'),
  data: z.string().regex(/^([0-9a-fA-F]{2}){0,12}$/, '0-12 bytes of hex').default(''),
  ack: z.boolean().default(false),
  oob: z.boolean().default(false),
})

const csvCell = (v: unknown) => {
  const s = v === null || v === undefined ? '' : typeof v === 'object' ? JSON.stringify(v) : String(v)
  return /[",\n]/.test(s) ? `"${s.replaceAll('"', '""')}"` : s
}

export function createApp(engine: Engine) {
  const { store, gateway, sigfox } = engine
  const app = new Hono()

  app.onError((err, c) => c.json({ error: err.message }, 500))

  // --- UI
  app.get('/', (c) => c.html(indexHtml as unknown as string)) // bun-types は .html を HTMLBundle と型付けするが type: 'text' なので文字列
  app.get('/app.js', (c) => c.body(appJs, 200, { 'Content-Type': 'text/javascript; charset=utf-8' }))
  app.get('/i18n.js', (c) => c.body(i18nJs, 200, { 'Content-Type': 'text/javascript; charset=utf-8' }))
  app.get('/style.css', (c) => c.body(styleCss, 200, { 'Content-Type': 'text/css; charset=utf-8' }))

  const api = new Hono()

  api.get('/status', (c) => c.json({ version: VERSION, gateway: gateway.status(), radio: sigfox?.status() ?? null, settings: store.getSettings() }))

  // --- settings
  api.get('/settings', (c) => c.json(store.getSettings()))
  api.put('/settings', async (c) => {
    const before = store.getSettings()
    const r = parse(Settings, { ...before, ...(await c.req.json()) })
    if (!r.ok) return c.json({ error: r.error }, 400)
    store.saveSettings(r.data)
    if (r.data.channel !== before.channel) gateway.setChannel(r.data.channel)
    const sigfoxChanged = r.data.sigfoxPort !== before.sigfoxPort || r.data.sigfoxFreq !== before.sigfoxFreq
    // 自動検出のゲートウェイは Sigfox 受信機のポートを避けるので、それが変わったときも繋ぎ直す
    const gatewayChanged = r.data.serialPort !== before.serialPort || (r.data.serialPort === 'auto' && r.data.sigfoxPort !== before.sigfoxPort)
    if (sigfoxChanged) sigfox?.reconnect()
    if (gatewayChanged && gateway instanceof SerialGateway) gateway.reconnect()
    return c.json(r.data)
  })
  api.get('/serial/ports', (c) => c.json(listSerialPorts()))

  // --- devices
  api.get('/devices', (c) => c.json(store.listDevices()))
  api.put('/devices/:id', async (c) => {
    const r = parse(DeviceInput, { ...(await c.req.json()), id: c.req.param('id') })
    if (!r.ok) return c.json({ error: r.error }, 400)
    return c.json(store.upsertDevice(r.data))
  })
  api.delete('/devices/:id', (c) => {
    store.deleteDevice(c.req.param('id').toUpperCase())
    return c.body(null, 204)
  })

  // --- messages
  api.get('/messages', (c) => {
    const q = c.req.query()
    return c.json(
      store.listMessages({
        deviceId: q.device?.toUpperCase() || undefined,
        before: q.before ? Number(q.before) : undefined,
        limit: q.limit ? Number(q.limit) : undefined,
      }),
    )
  })
  api.delete('/messages', (c) => {
    store.clearMessages(c.req.query('device')?.toUpperCase() || undefined)
    return c.body(null, 204)
  })
  api.get('/messages.csv', (c) => {
    const rows = store.listMessages({ deviceId: c.req.query('device')?.toUpperCase() || undefined, limit: 1000 })
    const cols = ['id', 'time', 'deviceId', 'seq', 'data', 'decoded', 'ack', 'oob', 'rssi', 'nbFrames', 'auth', 'downlink', 'downlinkStatus', 'radio'] as const
    const lines = [cols.join(','), ...rows.map((m) => cols.map((k) => csvCell(k === 'time' ? new Date(m.time).toISOString() : m[k])).join(','))]
    return c.body(`${lines.join('\n')}\n`, 200, {
      'Content-Type': 'text/csv; charset=utf-8',
      'Content-Disposition': 'attachment; filename="opennek-messages.csv"',
    })
  })

  // --- callbacks
  api.get('/callbacks', (c) => c.json(store.listCallbacks()))
  api.post('/callbacks', async (c) => {
    const r = parse(CallbackInput, await c.req.json())
    if (!r.ok) return c.json({ error: r.error }, 400)
    return c.json(store.createCallback(r.data), 201)
  })
  api.put('/callbacks/:id', async (c) => {
    const r = parse(CallbackInput, await c.req.json())
    if (!r.ok) return c.json({ error: r.error }, 400)
    const cb = store.updateCallback(Number(c.req.param('id')), r.data)
    return cb ? c.json(cb) : c.json({ error: 'not found' }, 404)
  })
  api.delete('/callbacks/:id', (c) => {
    store.deleteCallback(Number(c.req.param('id')))
    return c.body(null, 204)
  })
  // 直近のメッセージ (無ければダミー) でコールバックを試し撃ちする。ログには残さない
  api.post('/callbacks/:id/test', async (c) => {
    const cb = store.getCallback(Number(c.req.param('id')))
    if (!cb) return c.json({ error: 'not found' }, 404)
    const latest = store.listMessages({ limit: 50 }).find((m) => appliesTo(cb, m.deviceId))
    const m = latest ?? {
      id: 0, deviceId: 'F0000001', time: Date.now(), seq: 0, data: '0102030405', ack: cb.downlink, oob: false, rssi: -60,
      nbFrames: 3, auth: 'ok' as const, src: '02:00:00:00:00:00', station: '00000000', radio: 'espnow' as const, decoded: null, downlink: null, downlinkStatus: 'none' as const,
    }
    const result = await runCallback(cb, m, store.getDevice(m.deviceId)?.name ?? '', store.getSettings().downlinkTimeoutMs)
    return c.json({ message: m, result })
  })
  api.get('/callback-logs', (c) => {
    const q = c.req.query()
    return c.json(
      store.listLogs({
        callbackId: q.callback ? Number(q.callback) : undefined,
        messageId: q.message ? Number(q.message) : undefined,
        limit: q.limit ? Number(q.limit) : undefined,
      }),
    )
  })

  // --- simulator (--sim のときだけ)
  api.post('/sim/uplink', async (c) => {
    if (!(gateway instanceof SimGateway)) return c.json({ error: 'simulator is not enabled (start with --sim)' }, 400)
    const r = parse(SimUplink, await c.req.json())
    if (!r.ok) return c.json({ error: r.error }, 400)
    let device = store.getDevice(r.data.deviceId)
    if (!device?.key) {
      device = store.upsertDevice({
        id: r.data.deviceId,
        name: device?.name || 'sim',
        key: randomBytes(16).toString('hex'),
        downlinkMode: device?.downlinkMode ?? 'none',
        downlinkData: device?.downlinkData ?? '',
        payloadConfig: device?.payloadConfig ?? '',
      })
    }
    const settings = store.getSettings()
    const res = await gateway.sendUplink({
      deviceId: device.id,
      key: fromHex(device.key),
      payload: fromHex(r.data.data),
      ack: r.data.ack,
      oob: r.data.oob,
      dlWaitMs: settings.dedupWindowMs + settings.downlinkTimeoutMs + 1000,
    })
    return c.json({ deviceId: device.id, seq: res.seq, downlink: res.downlink ? Buffer.from(res.downlink).toString('hex') : null })
  })

  // --- live events
  api.get('/events', (c) =>
    streamSSE(c, async (stream) => {
      const queue: EngineEvent[] = []
      let wake: (() => void) | null = null
      const off = engine.on((e) => {
        queue.push(e)
        wake?.()
      })
      stream.onAbort(() => {
        off()
        wake?.()
      })
      await stream.writeSSE({ event: 'gateway', data: JSON.stringify(gateway.status()) })
      if (sigfox) await stream.writeSSE({ event: 'radio', data: JSON.stringify({ type: 'radio', status: sigfox.status() }) })
      while (!stream.aborted) {
        const e = queue.shift()
        if (e) {
          await stream.writeSSE({ event: e.type, data: JSON.stringify(e) })
          continue
        }
        // 25 秒ごとにコメントを送ってプロキシ等に切られないようにする
        await new Promise<void>((resolve) => {
          wake = resolve
          setTimeout(resolve, 25_000)
        })
        wake = null
        if (!queue.length && !stream.aborted) await stream.write(': ping\n\n')
      }
    }),
  )

  app.route('/api', api)
  return app
}
