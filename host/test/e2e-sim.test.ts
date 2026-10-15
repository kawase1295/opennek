import { afterAll, beforeAll, expect, test } from 'bun:test'
import { Store } from '../src/db'
import { Engine } from '../src/engine'
import { SimGateway } from '../src/gateway/sim'
import { createApp } from '../src/server'

// 本物のコールバック受け口を立て、シミュレータからアップリンク → コールバック → ダウンリンクまで通す
const received: any[] = []
const sink = Bun.serve({
  port: 0,
  async fetch(req) {
    const text = await req.text()
    let body: any
    try {
      body = JSON.parse(text)
    } catch {
      // ペイロード定義が無いと {customData#temp} が置換されず JSON にならない
      return new Response('bad json', { status: 400 })
    }
    received.push({ body, auth: req.headers.get('authorization') })
    if (body.ack) return Response.json({ [body.device]: { downlinkData: 'deadbeef00112233' } })
    return new Response(null, { status: 204 })
  },
})

const store = new Store(':memory:')
store.saveSettings({ ...store.getSettings(), dedupWindowMs: 100 })
const gateway = new SimGateway()
const engine = new Engine(store, gateway)
const app = createApp(engine)
const call = (path: string, init?: { method?: string; body?: unknown }) =>
  app.request(`/api${path}`, {
    method: init?.method ?? 'GET',
    headers: { 'Content-Type': 'application/json' },
    body: init?.body ? JSON.stringify(init.body) : undefined,
  })

beforeAll(async () => {
  gateway.start()
  await Bun.sleep(10)
})
afterAll(() => sink.stop(true))

test('gateway status is connected', async () => {
  const s = await (await call('/status')).json()
  expect(s.gateway).toMatchObject({ state: 'connected', kind: 'sim' })
})

test('uplink → callback with custom data → downlink from callback response', async () => {
  const cb = await call('/callbacks', {
    method: 'POST',
    body: {
      name: 'sink',
      url: `http://localhost:${sink.port}/up`,
      headers: { Authorization: 'Bearer t' },
      body: '{"device":"{device}","data":"{data}","seq":{seqNumber},"ack":{ack},"temp":{customData#temp},"n":{nbFrames}}',
      downlink: true,
    },
  })
  expect(cb.status).toBe(201)

  // 1 回目でデバイス (鍵付き) が自動登録されるので、そこにペイロード定義とダウンリンク設定を足す
  const first = await (await call('/sim/uplink', { method: 'POST', body: { deviceId: '0a1b2c3d', data: '00' } })).json()
  expect(first.downlink).toBeNull()
  const dev = (await (await call('/devices')).json())[0]
  expect(dev).toMatchObject({ id: '0A1B2C3D', name: 'sim' })
  expect(dev.key).toMatch(/^[0-9a-f]{32}$/)
  const put = await call('/devices/0A1B2C3D', { method: 'PUT', body: { ...dev, payloadConfig: 'temp::int:16:little-endian', downlinkMode: 'callback' } })
  expect(put.status).toBe(200)

  const res = await (await call('/sim/uplink', { method: 'POST', body: { deviceId: '0A1B2C3D', data: '85ff', ack: true } })).json()
  expect(res).toEqual({ deviceId: '0A1B2C3D', seq: 1, downlink: 'deadbeef00112233' })

  const last = received.at(-1)
  expect(last.auth).toBe('Bearer t')
  expect(last.body).toEqual({ device: '0A1B2C3D', data: '85ff', seq: 1, ack: true, temp: -123, n: 3 })

  const [msg] = await (await call('/messages?limit=1')).json()
  expect(msg).toMatchObject({ seq: 1, auth: 'ok', nbFrames: 3, decoded: { temp: -123 }, downlink: 'deadbeef00112233', downlinkStatus: 'sent' })
  const logs = await (await call(`/callback-logs?message=${msg.id}`)).json()
  expect(logs[0]).toMatchObject({ ok: true, status: 200 })
})

test('static downlink and invalid input', async () => {
  const dev = (await (await call('/devices')).json())[0]
  await call('/devices/0A1B2C3D', { method: 'PUT', body: { ...dev, downlinkMode: 'static', downlinkData: '0102030405060708' } })
  const res = await (await call('/sim/uplink', { method: 'POST', body: { deviceId: '0A1B2C3D', ack: true } })).json()
  expect(res.downlink).toBe('0102030405060708')

  const bad = await call('/devices/0A1B2C3D', { method: 'PUT', body: { ...dev, payloadConfig: 'x::wat:8' } })
  expect(bad.status).toBe(400)
  expect((await bad.json()).error).toContain('unknown type')
})

test('frame signed with a wrong key is flagged and not forwarded', async () => {
  const before = received.length
  // 登録済みの鍵とは別の鍵で同じ ID を名乗る
  const { fromHex } = await import('../src/frame')
  await gateway.sendUplink({ deviceId: '0A1B2C3D', key: fromHex('ff'.repeat(16)), payload: fromHex('01') })
  await Bun.sleep(250)
  const [msg] = await (await call('/messages?limit=1')).json()
  expect(msg.auth).toBe('bad')
  expect(received.length).toBe(before)
})

test('CSV export', async () => {
  const csv = await (await call('/messages.csv')).text()
  expect(csv.split('\n')[0]).toBe('id,time,deviceId,seq,data,decoded,ack,oob,rssi,nbFrames,auth,downlink,downlinkStatus,radio')
  expect(csv).toContain('"{""temp"":-123}"')
})
