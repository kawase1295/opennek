import { afterAll, describe, expect, test } from 'bun:test'
import { constants, createReadStream, existsSync, mkdtempSync, openSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { Store } from '../src/db'
import { Engine, type EngineEvent } from '../src/engine'
import { SimGateway } from '../src/gateway/sim'
import { SigfoxRadio } from '../src/gateway/sigfox'
import { PUBLIC_KEY, uplinkAuth } from '../src/sigfox/auth'

// Real RC3 uplinks from device 00763DA3 recorded with the LSM100A streamer (2 kHz, raw = -2 * dBm)
const live = readFileSync(join(import.meta.dir, 'fixtures/rc3-00763DA3-live.rssi'))

// The streamer's line format: 64 samples per line with a 16-bit sequence number
function* streamerLines(raw: Uint8Array) {
  yield 'I fw=0.1.0 f=923200000 bw=10 sr=2000 boost=1'
  for (let i = 0, seq = 0; i < raw.length; i += 64, seq++) {
    yield `R${(seq & 0xffff).toString(16).padStart(4, '0')}:${Buffer.from(raw.subarray(i, i + 64)).toString('hex')}`
  }
}

const settle = async (ms = 50) => await Bun.sleep(ms)

test('real Sigfox uplinks become messages and run callbacks', async () => {
  const hits: { url: string; body: any }[] = []
  const server = Bun.serve({
    port: 0,
    async fetch(req) {
      hits.push({ url: req.url, body: await req.json() })
      return new Response(null, { status: 204 })
    },
  })
  const store = new Store(':memory:')
  store.createCallback({
    name: 'test', enabled: true, devices: '', method: 'POST',
    url: `http://127.0.0.1:${server.port}/up?device={device}`, headers: {}, contentType: 'application/json',
    body: '{"device":"{device}","seq":{seqNumber},"data":"{data}","ack":{ack},"rssi":{rssi},"frames":{nbFrames},"station":"{station}"}',
    downlink: false,
  })
  const radio = new SigfoxRadio(() => ({ port: '', freq: 923_200_000 }))
  const engine = new Engine(store, new SimGateway(), radio)
  const events: EngineEvent[] = []
  engine.on((e) => events.push(e))

  for (const line of streamerLines(live)) radio.handleLine(line)
  radio.flush()
  for (let i = 0; i < 100 && hits.length < 3; i++) await settle(20)
  server.stop(true)

  const messages = store.listMessages().reverse()
  expect(messages.map((m) => [m.seq, m.data, m.ack, m.radio, m.auth])).toEqual([
    // the device uses its private key, which the host does not know
    [1982, '1234567890', false, 'sigfox', 'unverified'],
    [1983, '11', false, 'sigfox', 'unverified'],
    [1984, '11', true, 'sigfox', 'unverified'],
  ])
  for (const m of messages) {
    expect(m.deviceId).toBe('00763DA3')
    expect(m.nbFrames).toBeGreaterThanOrEqual(1)
    expect(m.rssi).toBeLessThan(0)
  }
  // Receive only: a requested downlink is marked as unsupported instead of waiting for data
  expect(messages[2]!.downlinkStatus).toBe('unsupported')
  expect(store.getDevice('00763DA3')).toMatchObject({ lastSeq: 1984 })

  hits.sort((a, b) => a.body.seq - b.body.seq)
  expect(hits.map((h) => h.body)).toMatchObject([
    { device: '00763DA3', seq: 1982, data: '1234567890', ack: false, station: '0000' },
    { device: '00763DA3', seq: 1983, data: '11', ack: false },
    { device: '00763DA3', seq: 1984, data: '11', ack: true },
  ])
  expect(hits[0]!.url).toEndWith('/up?device=00763DA3')

  const status = radio.status()
  expect(status).toMatchObject({ state: 'connected', fw: '0.1.0', freq: 923_200_000, rate: 2000, lostLines: 0 })
  expect(status.frames).toBeGreaterThanOrEqual(3)
  expect(events.some((e) => e.type === 'radio')).toBe(true)
})

test('a late repetition does not create a second message', () => {
  const store = new Store(':memory:')
  const engine = new Engine(store, new SimGateway())
  const m = {
    deviceId: '00763DA3', counter: 7, payload: new Uint8Array([0x11]), bit: null, downlink: false, auth: new Uint8Array(2),
    time: 0, rssi: -80.5, ranks: [0, 1], clockErrorPpm: 0, receivedAt: Date.now(),
  }
  engine.handleSigfox(m)
  engine.handleSigfox({ ...m, ranks: [2] })
  expect(store.listMessages()).toHaveLength(1)
  expect(store.listMessages()[0]).toMatchObject({ seq: 7, rssi: -80, nbFrames: 2, radio: 'sigfox' })
})

describe('authentication of real Sigfox uplinks', () => {
  const uplink = (key: Uint8Array, counter: number) => {
    const f = { deviceId: '0A1B2C3D', counter, payload: Uint8Array.of(1, 2, 3), bit: null, downlink: false }
    return { ...f, auth: uplinkAuth(key, f), time: 0, rssi: -90, ranks: [0, 1, 2], clockErrorPpm: 0, receivedAt: Date.now() }
  }
  const privateKey = Uint8Array.from(Buffer.from('3f2a10990102030405060708090a0b0c', 'hex'))
  const setup = (key: string, acceptUnknownDevices: boolean) => {
    const store = new Store(':memory:')
    store.saveSettings({ ...store.getSettings(), acceptUnknownDevices })
    if (key) store.upsertDevice({ id: '0A1B2C3D', name: '', key, downlinkMode: 'none', downlinkData: '', payloadConfig: '' })
    return { store, engine: new Engine(store, new SimGateway()) }
  }
  const lastAuth = (store: Store) => store.listMessages()[0]!.auth

  test('a device in public key mode is verified without registering a key', () => {
    const { store, engine } = setup('', true)
    engine.handleSigfox(uplink(PUBLIC_KEY, 1))
    expect(lastAuth(store)).toBe('public')
  })

  test('anyone can sign with the public key, so it does not pass for a registered key', () => {
    const { store, engine } = setup(Buffer.from(privateKey).toString('hex'), true)
    engine.handleSigfox(uplink(PUBLIC_KEY, 1))
    expect(lastAuth(store)).toBe('bad')
  })

  test('public key uplinks run callbacks only while unknown devices are accepted', async () => {
    const hits: number[] = []
    const server = Bun.serve({ port: 0, fetch: () => (hits.push(1), new Response(null, { status: 204 })) })
    for (const accept of [false, true]) {
      const { store, engine } = setup('', accept)
      store.createCallback({
        name: '', enabled: true, devices: '', method: 'POST', url: `http://127.0.0.1:${server.port}/`, headers: {},
        contentType: 'application/json', body: '{}', downlink: false,
      })
      engine.handleSigfox(uplink(PUBLIC_KEY, 1))
      for (let i = 0; i < 50 && store.listLogs().length === 0 && accept; i++) await Bun.sleep(10)
    }
    server.stop(true)
    expect(hits).toHaveLength(1)
  })

  test('a registered device key is checked', () => {
    const { store, engine } = setup(Buffer.from(privateKey).toString('hex'), false)
    engine.handleSigfox(uplink(privateKey, 1))
    expect(lastAuth(store)).toBe('ok')
    engine.handleSigfox({ ...uplink(privateKey, 2), auth: Uint8Array.of(0, 0, 0, 0) })
    expect(lastAuth(store)).toBe('bad')
  })

  test('without a matching key the uplink is unverified', () => {
    const { store, engine } = setup('', true)
    engine.handleSigfox(uplink(privateKey, 1))
    expect(lastAuth(store)).toBe('unverified')
  })
})

test('an existing database gains the radio column', () => {
  const path = join(mkdtempSync(join(tmpdir(), 'opennek-')), 'old.db')
  const { Database } = require('bun:sqlite')
  const old = new Database(path, { create: true })
  old.run(`CREATE TABLE messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT, device_id TEXT NOT NULL, time INTEGER NOT NULL, seq INTEGER NOT NULL,
    data TEXT NOT NULL, ack INTEGER NOT NULL, oob INTEGER NOT NULL, rssi INTEGER NOT NULL, nb_frames INTEGER NOT NULL,
    auth TEXT NOT NULL, src TEXT NOT NULL, station TEXT NOT NULL, decoded TEXT,
    downlink TEXT, downlink_status TEXT NOT NULL DEFAULT 'none')`)
  old.run(`INSERT INTO messages (device_id, time, seq, data, ack, oob, rssi, nb_frames, auth, src, station)
    VALUES ('F0000001', 1, 1, '01', 0, 0, -50, 3, 'ok', '02:00:00:00:00:01', '0001')`)
  old.close()
  const store = new Store(path)
  expect(store.listMessages()[0]).toMatchObject({ deviceId: 'F0000001', radio: 'espnow' })
  store.db.close()
})

// The receiver over a real serial line: socat stands in for the bridge and the streamer
const hasSocat = Bun.which('socat') !== null
const dir = mkdtempSync(join(tmpdir(), 'opennek-'))
const fwPath = join(dir, 'fw')
const hostPath = join(dir, 'host')
const socat = hasSocat ? Bun.spawn(['socat', `pty,raw,echo=0,link=${fwPath}`, `pty,raw,echo=0,link=${hostPath}`], { stderr: 'ignore' }) : null
afterAll(() => socat?.kill())

test.skipIf(!hasSocat)('Sigfox receiver over a serial line', async () => {
  for (let i = 0; i < 50 && !(existsSync(fwPath) && existsSync(hostPath)); i++) await settle(20)
  const fd = openSync(fwPath, constants.O_RDWR | constants.O_NOCTTY)
  const commands: string[] = []
  const rs = createReadStream('', { fd, encoding: 'latin1' })
  let buf = ''
  rs.on('data', (chunk) => {
    buf += chunk
    let i: number
    while ((i = buf.indexOf('\n')) >= 0) {
      commands.push(buf.slice(0, i))
      buf = buf.slice(i + 1)
    }
  })

  const store = new Store(':memory:')
  const radio = new SigfoxRadio(() => ({ port: hostPath, freq: 923_100_000 }))
  new Engine(store, new SimGateway(), radio)
  radio.start()
  for (let i = 0; i < 100 && commands.length < 2; i++) await settle(20)
  // The host tunes the streamer and asks for its status
  expect(commands).toEqual(['F923100000', '?'])

  // Written by another process: a blocking write here would fill the pty while this process cannot read it
  const recording = join(dir, 'stream.txt')
  writeFileSync(recording, `${[...streamerLines(live)].join('\n')}\n`)
  const cat = Bun.spawn(['sh', '-c', 'cat "$0" > "$1"', recording, fwPath])
  await cat.exited
  for (let i = 0; i < 200 && store.listMessages().length < 2; i++) await settle(20)
  // The last message is still waiting for its quiet period; stopping flushes it
  await radio.stop()
  for (let i = 0; i < 50 && store.listMessages().length < 3; i++) await settle(20)
  expect(store.listMessages().map((m) => m.seq).sort()).toEqual([1982, 1983, 1984])
  expect(radio.status().state).toBe('disconnected')
  rs.destroy()
})
