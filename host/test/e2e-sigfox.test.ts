import { afterAll, describe, expect, test } from 'vitest'
import { constants, existsSync, mkdtempSync, openSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { Store } from '../src/db'
import { Engine, type EngineEvent } from '../src/engine'
import { type SigfoxDownlinkTx, SigfoxRadio } from '../src/radio'
import { PUBLIC_KEY, uplinkAuth } from '../src/sigfox/auth'
import { encodeDownlink, RC3_DL_T_RX_MS, RC3_DL_T_W_MS } from '../src/sigfox/downlink'
import { DatabaseSync } from 'node:sqlite'
import { ReadStream as TtyReadStream } from 'node:tty'
import { hasCommand, ptyPair, run, serveFetch, sleep } from './helpers'

// Real RC3 uplinks from device 00763DA3 recorded with the LSM100A streamer (2 kHz, raw = -2 * dBm)
const live = readFileSync(join(import.meta.dirname, '../../fixtures/rc3-00763DA3-live.rssi'))

// The streamer's line format: 64 samples per line with a 16-bit sequence number
function* streamerLines(raw: Uint8Array) {
  yield 'I fw=0.1.0 f=923200000 bw=10 sr=2000 boost=1'
  for (let i = 0, seq = 0; i < raw.length; i += 64, seq++) {
    yield `R${(seq & 0xffff).toString(16).padStart(4, '0')}:${Buffer.from(raw.subarray(i, i + 64)).toString('hex')}`
  }
}

const settle = async (ms = 50) => await sleep(ms)

test('real Sigfox uplinks become messages and run callbacks', async () => {
  const hits: { url: string; body: any }[] = []
  const server = await serveFetch(async (req) => {
    hits.push({ url: req.url, body: await req.json() })
    return new Response(null, { status: 204 })
  })
  const store = new Store(':memory:')
  store.createCallback({
    name: 'test', enabled: true, devices: '', method: 'POST',
    url: `http://127.0.0.1:${server.port}/up?device={device}`, headers: {}, contentType: 'application/json',
    body: '{"device":"{device}","seq":{seqNumber},"data":"{data}","ack":{ack},"rssi":{rssi},"frames":{nbFrames},"station":"{station}"}',
    downlink: false,
  })
  const radio = new SigfoxRadio(() => ({ port: '', freq: 923_200_000 }))
  const engine = new Engine(store, radio)
  const events: EngineEvent[] = []
  engine.on((e) => events.push(e))
  const radioMessages: { receivedAt: number; firstFrameEndAt: number | null; ranks: number[] }[] = []
  radio.onMessage((m) => radioMessages.push(m))

  for (const line of streamerLines(live)) radio.handleLine(line)
  radio.flush()
  // The downlink status of the last message is settled after its callbacks have answered
  const settled = () => hits.length >= 3 && store.listMessages().every((m) => m.downlinkStatus !== 'pending')
  for (let i = 0; i < 100 && !settled(); i++) await settle(20)
  server.close()

  const messages = store.listMessages().reverse()
  expect(messages.map((m) => [m.seq, m.data, m.ack, m.auth])).toEqual([
    // the device uses its private key, which the host does not know
    [1982, '1234567890', false, 'unverified'],
    [1983, '11', false, 'unverified'],
    [1984, '11', true, 'unverified'],
  ])
  for (const m of messages) {
    expect(m.deviceId).toBe('00763DA3')
    expect(m.nbFrames).toBeGreaterThanOrEqual(1)
    expect(m.rssi).toBeLessThan(0)
  }
  // Receive only: a requested downlink is marked as unsupported instead of waiting for data
  expect(messages[2]!.downlinkStatus).toBe('unsupported')
  expect(store.getDevice('00763DA3')).toMatchObject({ lastSeq: 1984 })
  // Frame 1 starts the device's downlink timer; it lasts 0.5-2 s depending on the payload
  for (const m of radioMessages.filter((m) => m.ranks.includes(0))) {
    expect(m.firstFrameEndAt! - m.receivedAt).toBeGreaterThan(500)
    expect(m.firstFrameEndAt! - m.receivedAt).toBeLessThan(2500)
  }

  hits.sort((a, b) => a.body.seq - b.body.seq)
  expect(hits.map((h) => h.body)).toMatchObject([
    { device: '00763DA3', seq: 1982, data: '1234567890', ack: false, station: '0000' },
    { device: '00763DA3', seq: 1983, data: '11', ack: false },
    { device: '00763DA3', seq: 1984, data: '11', ack: true },
  ])
  expect(hits[0]!.url).toMatch(/\/up\?device=00763DA3$/)

  const status = radio.status()
  expect(status).toMatchObject({ state: 'connected', fw: '0.1.0', freq: 923_200_000, rate: 2000, lostLines: 0 })
  expect(status.frames).toBeGreaterThanOrEqual(3)
  expect(events.some((e) => e.type === 'radio')).toBe(true)
})

test('a late repetition does not create a second message', () => {
  const store = new Store(':memory:')
  const engine = new Engine(store)
  const m = {
    deviceId: '00763DA3', counter: 7, payload: new Uint8Array([0x11]), bit: null, downlink: false, control: false, auth: new Uint8Array(2),
    time: 0, rssi: -80.5, ranks: [0, 1], frames: [], clockErrorPpm: 0, receivedAt: Date.now(), firstFrameEnd: null, firstFrameEndAt: null, uplinkFreqHz: null,
  }
  engine.handleSigfox(m)
  engine.handleSigfox({ ...m, ranks: [2] })
  expect(store.listMessages()).toHaveLength(1)
  expect(store.listMessages()[0]).toMatchObject({ seq: 7, rssi: -80, nbFrames: 2 })
})

describe('authentication of real Sigfox uplinks', () => {
  const uplink = (key: Uint8Array, counter: number) => {
    const f = { deviceId: '0A1B2C3D', counter, payload: Uint8Array.of(1, 2, 3), bit: null, downlink: false, control: false }
    return { ...f, auth: uplinkAuth(key, f), time: 0, rssi: -90, ranks: [0, 1, 2], frames: [], clockErrorPpm: 0, receivedAt: Date.now(), firstFrameEnd: null, firstFrameEndAt: null, uplinkFreqHz: null }
  }
  const privateKey = Uint8Array.from(Buffer.from('3f2a10990102030405060708090a0b0c', 'hex'))
  const setup = (key: string, acceptUnknownDevices: boolean) => {
    const store = new Store(':memory:')
    store.saveSettings({ ...store.getSettings(), acceptUnknownDevices })
    if (key) store.upsertDevice({ id: '0A1B2C3D', name: '', key, downlinkMode: 'none', downlinkData: '', payloadConfig: '' })
    return { store, engine: new Engine(store) }
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
    const server = await serveFetch(() => (hits.push(1), new Response(null, { status: 204 })))
    for (const accept of [false, true]) {
      const { store, engine } = setup('', accept)
      store.createCallback({
        name: '', enabled: true, devices: '', method: 'POST', url: `http://127.0.0.1:${server.port}/`, headers: {},
        contentType: 'application/json', body: '{}', downlink: false,
      })
      engine.handleSigfox(uplink(PUBLIC_KEY, 1))
      for (let i = 0; i < 50 && store.listLogs().length === 0 && accept; i++) await sleep(10)
    }
    server.close()
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

describe('downlinks to real Sigfox devices', () => {
  const ULF = 923_183_000 // frame 1 somewhere in the macro channel
  const uplink = (key: Uint8Array, counter: number, extra: object = {}) => {
    const f = { deviceId: '0A1B2C3D', counter, payload: Uint8Array.of(1, 2, 3), bit: null, downlink: true, control: false }
    // the device's receive window opens 100 ms after the start of the transmission
    const firstFrameEndAt = Date.now() - RC3_DL_T_W_MS - 2000 + 100
    return {
      ...f, auth: uplinkAuth(key, f), time: 0, rssi: -90, ranks: [0, 1, 2], frames: [], clockErrorPpm: 0,
      receivedAt: firstFrameEndAt - 1500, firstFrameEnd: 0, firstFrameEndAt, uplinkFreqHz: ULF, ...extra,
    }
  }
  const setup = (opts: { sigfoxDownlink?: boolean; acceptUnknownDevices?: boolean; key?: string; result?: boolean } = {}) => {
    const store = new Store(':memory:')
    store.saveSettings({ ...store.getSettings(), sigfoxDownlink: opts.sigfoxDownlink ?? true, acceptUnknownDevices: opts.acceptUnknownDevices ?? true })
    store.upsertDevice({ id: '0A1B2C3D', name: '', key: opts.key ?? '', downlinkMode: 'static', downlinkData: '0102030405060708', payloadConfig: '' })
    const sent: SigfoxDownlinkTx[] = []
    const tx = { transmit: async (t: SigfoxDownlinkTx) => (sent.push(t), opts.result ?? true) }
    const engine = new Engine(store, null, tx)
    const lines: string[] = []
    engine.on((e) => e.type === 'console' && lines.push(e.line))
    return { store, engine, sent, lines }
  }
  const settled = async (store: Store) => {
    for (let i = 0; i < 100 && store.listMessages()[0]?.downlinkStatus === 'pending'; i++) await settle(10)
    return store.listMessages()[0]!
  }

  test('a public key device gets its static data in its receive window', async () => {
    const { store, engine, sent, lines } = setup()
    const up = uplink(PUBLIC_KEY, 42)
    engine.handleSigfox(up)
    expect(store.listMessages()[0]!.downlinkStatus).toBe('pending')
    expect(sent).toHaveLength(0)
    const m = await settled(store)
    expect(m).toMatchObject({ downlinkStatus: 'sent', downlink: '0102030405060708' })
    expect(sent).toHaveLength(1)
    const t = sent[0]!
    expect(t.freqHz).toBe(ULF - 1_000_000)
    expect(t.frame).toEqual(encodeDownlink({ deviceId: '0A1B2C3D', counter: 42, key: PUBLIC_KEY, payload: Uint8Array.of(1, 2, 3, 4, 5, 6, 7, 8) }))
    expect(t.deadline).toBe(up.firstFrameEndAt + RC3_DL_T_W_MS + RC3_DL_T_RX_MS - 1000)
    // every transmission is logged
    expect(lines.some((l) => l.includes('TX 922.183000 MHz'))).toBe(true)
  })

  test('a registered private key is used for a verified device', async () => {
    const key = '3f2a10990102030405060708090a0b0c'
    const { store, engine, sent } = setup({ key, acceptUnknownDevices: false })
    engine.handleSigfox(uplink(Uint8Array.from(Buffer.from(key, 'hex')), 7))
    expect((await settled(store)).downlinkStatus).toBe('sent')
    expect(sent[0]!.frame).toEqual(encodeDownlink({ deviceId: '0A1B2C3D', counter: 7, key: Uint8Array.from(Buffer.from(key, 'hex')), payload: Uint8Array.of(1, 2, 3, 4, 5, 6, 7, 8) }))
  })

  test('nothing is sent unless transmitting is enabled', async () => {
    const { store, engine, sent } = setup({ sigfoxDownlink: false })
    engine.handleSigfox(uplink(PUBLIC_KEY, 1))
    expect((await settled(store)).downlinkStatus).toBe('unsupported')
    expect(sent).toHaveLength(0)
  })

  test('an unauthenticated uplink gets no downlink', async () => {
    const privateKey = Uint8Array.from(Buffer.from('3f2a10990102030405060708090a0b0c', 'hex'))
    for (const [opts, key] of [[{}, privateKey], [{ acceptUnknownDevices: false }, PUBLIC_KEY]] as const) {
      const { store, engine, sent } = setup(opts)
      engine.handleSigfox(uplink(key, 1))
      expect((await settled(store)).downlinkStatus).toBe('nodata')
      expect(sent).toHaveLength(0)
    }
  })

  test('without the frequency or the end of frame 1 there is nothing to aim at', async () => {
    for (const extra of [{ uplinkFreqHz: null }, { firstFrameEndAt: null, firstFrameEnd: null, ranks: [1, 2] }]) {
      const { store, engine, sent } = setup()
      engine.handleSigfox(uplink(PUBLIC_KEY, 1, extra))
      expect((await settled(store)).downlinkStatus).toBe('unsupported')
      expect(sent).toHaveLength(0)
    }
  })

  test('a closed receive window or a refused transmission fails the downlink', async () => {
    const late = setup()
    late.engine.handleSigfox(uplink(PUBLIC_KEY, 1, { firstFrameEndAt: Date.now() - RC3_DL_T_W_MS - RC3_DL_T_RX_MS }))
    expect((await settled(late.store)).downlinkStatus).toBe('failed')
    expect(late.sent).toHaveLength(0)

    const refused = setup({ result: false })
    refused.engine.handleSigfox(uplink(PUBLIC_KEY, 1))
    expect((await settled(refused.store)).downlinkStatus).toBe('failed')
    expect(refused.sent).toHaveLength(1)
  })
})

test('an existing database loses the ESP-NOW messages and their columns', () => {
  const path = join(mkdtempSync(join(tmpdir(), 'opennek-')), 'old.db')
  const old = new DatabaseSync(path)
  old.exec(`CREATE TABLE messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT, device_id TEXT NOT NULL, time INTEGER NOT NULL, seq INTEGER NOT NULL,
    data TEXT NOT NULL, ack INTEGER NOT NULL, oob INTEGER NOT NULL, rssi INTEGER NOT NULL, nb_frames INTEGER NOT NULL,
    auth TEXT NOT NULL, src TEXT NOT NULL, station TEXT NOT NULL, decoded TEXT,
    downlink TEXT, downlink_status TEXT NOT NULL DEFAULT 'none', radio TEXT NOT NULL DEFAULT 'espnow')`)
  old.exec(`CREATE TABLE callback_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT, callback_id INTEGER NOT NULL, message_id INTEGER NOT NULL, time INTEGER NOT NULL,
    status INTEGER, ok INTEGER NOT NULL, error TEXT, duration_ms INTEGER NOT NULL, response TEXT)`)
  old.exec(`INSERT INTO messages (device_id, time, seq, data, ack, oob, rssi, nb_frames, auth, src, station, radio) VALUES
    ('F0000001', 1, 1, '01', 0, 0, -50, 3, 'ok', '02:00:00:00:00:01', '0001', 'espnow'),
    ('00763DA3', 2, 7, '02', 0, 0, -80, 3, 'unverified', 'sigfox', '0000', 'sigfox')`)
  old.exec(`INSERT INTO callback_logs (callback_id, message_id, time, ok, duration_ms) VALUES (1, 1, 1, 1, 5), (1, 2, 2, 1, 5)`)
  old.close()

  const store = new Store(path)
  expect(store.listMessages()).toEqual([
    expect.objectContaining({ deviceId: '00763DA3', seq: 7, station: '0000' }),
  ])
  expect(store.listMessages()[0]).not.toHaveProperty('radio')
  expect(store.listMessages()[0]).not.toHaveProperty('src')
  expect(store.listLogs().map((l) => l.messageId)).toEqual([2])
  // inserting works on the migrated table
  store.insertMessage({
    deviceId: '00763DA3', time: 3, seq: 8, data: '', ack: false, oob: false, rssi: -80, nbFrames: 3, auth: 'unverified',
    station: '0000', decoded: null, downlink: null, downlinkStatus: 'none',
  })
  expect(store.listMessages()).toHaveLength(2)
  store.db.close()
})

// The receiver over a real serial line: socat stands in for the bridge and the streamer
const hasSocat = hasCommand('socat')
const dir = mkdtempSync(join(tmpdir(), 'opennek-'))
const fwPath = join(dir, 'fw')
const hostPath = join(dir, 'host')
const socat = hasSocat ? ptyPair(fwPath, hostPath) : null
afterAll(() => socat?.kill())

test.skipIf(!hasSocat)('Sigfox receiver over a serial line', async () => {
  for (let i = 0; i < 50 && !(existsSync(fwPath) && existsSync(hostPath)); i++) await settle(20)
  const fd = openSync(fwPath, constants.O_RDWR | constants.O_NOCTTY)
  const commands: string[] = []
  const rs = new TtyReadStream(fd)
  rs.setEncoding('latin1')
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
  new Engine(store, radio)
  radio.start()
  for (let i = 0; i < 100 && commands.length < 3; i++) await settle(20)
  // The host tunes the streamer, turns on the CFO lines and asks for its status
  expect(commands).toEqual(['F923100000', 'C1', '?'])

  // Written by another process: a blocking write here would fill the pty while this process cannot read it
  const recording = join(dir, 'stream.txt')
  writeFileSync(recording, `${[...streamerLines(live)].join('\n')}\n`)
  await run('sh', ['-c', 'cat "$0" > "$1"', recording, fwPath])
  for (let i = 0; i < 200 && store.listMessages().length < 2; i++) await settle(20)
  // The last message is still waiting for its quiet period; stopping flushes it
  await radio.stop()
  for (let i = 0; i < 50 && store.listMessages().length < 3; i++) await settle(20)
  expect(store.listMessages().map((m) => m.seq).sort()).toEqual([1982, 1983, 1984])
  expect(radio.status().state).toBe('disconnected')
  rs.destroy()
})
