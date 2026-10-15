import { afterAll, expect, test } from 'bun:test'
import { constants, createReadStream, existsSync, mkdtempSync, openSync, writeSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { Store } from '../src/db'
import { Engine } from '../src/engine'
import { decodeFrame, encodeFrame, fromHex, toHex, TYPE_DOWNLINK, TYPE_UPLINK, verifyMac } from '../src/frame'
import { SerialGateway } from '../src/gateway/serial'

// socat で pty の対を作り、片側でゲートウェイのファームを真似て本物の SerialGateway を検証する
const hasSocat = Bun.which('socat') !== null
const dir = mkdtempSync(join(tmpdir(), 'opennek-'))
const fwPath = join(dir, 'fw')
const hostPath = join(dir, 'host')
const socat = hasSocat ? Bun.spawn(['socat', `pty,raw,echo=0,link=${fwPath}`, `pty,raw,echo=0,link=${hostPath}`], { stderr: 'ignore' }) : null

afterAll(() => socat?.kill())

test.skipIf(!hasSocat)('uplink and downlink over a serial line', async () => {
  for (let i = 0; i < 50 && !(existsSync(fwPath) && existsSync(hostPath)); i++) await Bun.sleep(20)
  const key = fromHex('00112233445566778899aabbccddeeff')
  const fd = openSync(fwPath, constants.O_RDWR | constants.O_NOCTTY)
  const send = (o: object) => writeSync(fd, `${JSON.stringify(o)}\n`)
  const downs: any[] = []
  let buf = ''
  const rs = createReadStream('', { fd, encoding: 'utf8' })
  rs.on('data', (chunk) => {
    buf += chunk
    let i: number
    while ((i = buf.indexOf('\n')) >= 0) {
      const cmd = JSON.parse(buf.slice(0, i))
      buf = buf.slice(i + 1)
      if (cmd.t === 'ping') send({ t: 'hello', fw: '0.1.0', radio: 'espnow', ch: 1, mac: '24:6F:28:AA:BB:CC' })
      if (cmd.t === 'ch') send({ t: 'hello', fw: '0.1.0', radio: 'espnow', ch: cmd.ch, mac: '24:6F:28:AA:BB:CC' })
      if (cmd.t === 'down') {
        downs.push(cmd)
        send({ t: 'sent', dst: cmd.dst, ok: true })
      }
    }
  })

  const store = new Store(':memory:')
  store.saveSettings({ ...store.getSettings(), dedupWindowMs: 100 })
  store.upsertDevice({ id: '0A1B2C3D', name: 'dev', key: toHex(key), downlinkMode: 'static', downlinkData: '1122334455667788', payloadConfig: '' })
  const gw = new SerialGateway(() => hostPath)
  const engine = new Engine(store, gw)
  const statuses: string[] = []
  engine.on((e) => e.type === 'gateway' && statuses.push(e.status.state))
  gw.start()
  for (let i = 0; i < 100 && gw.status().state !== 'connected'; i++) await Bun.sleep(20)
  expect(gw.status()).toMatchObject({ state: 'connected', fw: '0.1.0', channel: 1, mac: '24:6F:28:AA:BB:CC', port: hostPath })
  expect(gw.status().error).toBeUndefined()

  // ブートローダのゴミ行が混ざっても無視されること
  writeSync(fd, 'ets Jun  8 2016 00:22:57\r\nrst:0x1 (POWERON_RESET)\r\n')
  for (let repeat = 0; repeat < 3; repeat++) {
    const frame = encodeFrame({ type: TYPE_UPLINK, deviceId: '0A1B2C3D', seq: 42, payload: fromHex('a1b2'), key, ack: true, repeat })
    send({ t: 'up', src: '24:6F:28:11:22:33', rssi: -50 - repeat, frame: toHex(frame).toUpperCase() })
  }
  for (let i = 0; i < 100 && downs.length === 0; i++) await Bun.sleep(20)
  expect(downs).toHaveLength(1)
  expect(downs[0].dst).toBe('24:6F:28:11:22:33')
  const dl = decodeFrame(fromHex(downs[0].frame))
  expect(dl).toMatchObject({ type: TYPE_DOWNLINK, deviceId: '0A1B2C3D', seq: 42 })
  expect(toHex(dl.payload)).toBe('1122334455667788')
  expect(verifyMac(key, dl)).toBe(true)
  await Bun.sleep(50)
  const [m] = store.listMessages()
  expect(m).toMatchObject({ seq: 42, data: 'a1b2', rssi: -50, nbFrames: 3, auth: 'ok', downlinkStatus: 'sent' })

  await gw.stop()
  rs.destroy()
  expect(statuses).toContain('connecting')
})
