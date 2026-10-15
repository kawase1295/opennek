import { randomBytes } from 'node:crypto'
import { decodeFrame, encodeFrame, fromHex, SEQ_MOD, toHex, TYPE_DOWNLINK, TYPE_UPLINK, verifyMac } from '../frame'
import { Gateway } from './gateway'

// ハードウェア無しで動かすための仮想ゲートウェイ。ゲートウェイのファームと同じ行プロトコルを内部で話す
const SIM_MAC = '02:00:00:00:00:01'

type VirtualDevice = { key: Uint8Array; mac: string; seq: number }
type Waiter = { deviceId: string; seq: number; resolve: (data: Uint8Array | null) => void }

export class SimGateway extends Gateway {
  private channel = 1
  private devices = new Map<string, VirtualDevice>()
  private waiters: Waiter[] = []

  constructor() {
    super('sim')
  }

  start() {
    this.setStatus({ state: 'connecting', port: 'sim' })
    this.reply({ t: 'hello', fw: 'sim', radio: 'sim', ch: this.channel, mac: SIM_MAC })
  }

  async stop() {
    this.setStatus({ state: 'disconnected' })
  }

  protected writeLine(line: string) {
    const cmd = JSON.parse(line)
    if (cmd.t === 'ping') this.reply({ t: 'hello', fw: 'sim', radio: 'sim', ch: this.channel, mac: SIM_MAC })
    if (cmd.t === 'ch') {
      this.channel = cmd.ch
      this.reply({ t: 'hello', fw: 'sim', radio: 'sim', ch: this.channel, mac: SIM_MAC })
    }
    if (cmd.t === 'down') queueMicrotask(() => this.deliverDownlink(cmd.dst, fromHex(cmd.frame)))
  }

  private reply(obj: object) {
    queueMicrotask(() => this.handleLine(JSON.stringify(obj)))
  }

  // 仮想デバイスからアップリンクを送る。ack なら dlWaitMs までダウンリンクを待って返す
  async sendUplink(opts: {
    deviceId: string
    key: Uint8Array
    payload: Uint8Array
    ack?: boolean
    oob?: boolean
    rssi?: number
    dlWaitMs?: number
  }): Promise<{ seq: number; downlink: Uint8Array | null }> {
    let dev = this.devices.get(opts.deviceId)
    if (!dev || toHex(dev.key) !== toHex(opts.key)) {
      const mac = ['02', ...randomBytes(5).toString('hex').match(/../g)!].join(':').toUpperCase()
      dev = { key: opts.key, mac, seq: dev?.seq ?? 0 }
      this.devices.set(opts.deviceId, dev)
    }
    const seq = dev.seq
    dev.seq = (dev.seq + 1) % SEQ_MOD
    const rssi = opts.rssi ?? -40 - Math.floor(Math.random() * 60)
    const waiting = opts.ack
      ? new Promise<Uint8Array | null>((resolve) => {
          const w: Waiter = { deviceId: opts.deviceId, seq, resolve }
          this.waiters.push(w)
          setTimeout(() => {
            this.waiters = this.waiters.filter((x) => x !== w)
            resolve(null)
          }, opts.dlWaitMs ?? 8000)
        })
      : Promise.resolve(null)
    for (let repeat = 0; repeat < 3; repeat++) {
      const frame = encodeFrame({ type: TYPE_UPLINK, deviceId: opts.deviceId, seq, payload: opts.payload, key: opts.key, ack: opts.ack, oob: opts.oob, repeat })
      this.handleLine(JSON.stringify({ t: 'up', src: dev.mac, rssi: rssi - repeat * 2, frame: toHex(frame) }))
      await Bun.sleep(30)
    }
    return { seq, downlink: await waiting }
  }

  private deliverDownlink(dst: string, raw: Uint8Array) {
    const entry = [...this.devices.entries()].find(([, d]) => d.mac === dst.toUpperCase())
    this.handleLine(JSON.stringify({ t: 'sent', dst, ok: !!entry }))
    if (!entry) return
    const [deviceId, dev] = entry
    try {
      const f = decodeFrame(raw)
      if (f.type !== TYPE_DOWNLINK || f.deviceId !== deviceId || !verifyMac(dev.key, f)) return
      const i = this.waiters.findIndex((w) => w.deviceId === deviceId && w.seq === f.seq)
      if (i >= 0) this.waiters.splice(i, 1)[0]!.resolve(f.payload)
    } catch {}
  }
}
