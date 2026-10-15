import { z } from 'zod/v4'
import { fromHex, toHex } from '../frame'

// ゲートウェイ ⇔ ホストの JSON Lines (docs/protocol.md の (2))
const Mac = z.string().regex(/^([0-9A-Fa-f]{2}:){5}[0-9A-Fa-f]{2}$/)
export const GatewayLine = z.discriminatedUnion('t', [
  z.object({ t: z.literal('hello'), fw: z.string(), radio: z.string(), ch: z.number().int(), mac: Mac }),
  z.object({ t: z.literal('up'), src: Mac, rssi: z.number().int(), frame: z.string() }),
  z.object({ t: z.literal('sent'), dst: Mac, ok: z.boolean() }),
  z.object({ t: z.literal('log'), msg: z.string() }),
])
export type GatewayLine = z.infer<typeof GatewayLine>

export type GatewayStatus = {
  state: 'disconnected' | 'connecting' | 'connected'
  kind: 'serial' | 'sim'
  port: string | null
  fw?: string
  radio?: string
  channel?: number
  mac?: string
  error?: string
}

export type Uplink = { src: string; rssi: number; frame: Uint8Array; station: string }

type Listener<T> = (v: T) => void

const SENT_TIMEOUT_MS = 2000

// 行プロトコルの解釈とダウンリンク送信結果の待ち合わせ。実際の入出力はサブクラスが担う
export abstract class Gateway {
  protected st: GatewayStatus
  private uplinkListeners: Listener<Uplink>[] = []
  private statusListeners: Listener<GatewayStatus>[] = []
  private logListeners: Listener<string>[] = []
  private pendingSent: { dst: string; resolve: (ok: boolean) => void; timer: Timer }[] = []

  constructor(kind: GatewayStatus['kind']) {
    this.st = { state: 'disconnected', kind, port: null }
  }

  abstract start(): void
  abstract stop(): Promise<void>
  protected abstract writeLine(line: string): void

  status(): GatewayStatus {
    return { ...this.st }
  }
  onUplink(cb: Listener<Uplink>) {
    this.uplinkListeners.push(cb)
  }
  onStatus(cb: Listener<GatewayStatus>) {
    this.statusListeners.push(cb)
  }
  onLog(cb: Listener<string>) {
    this.logListeners.push(cb)
  }

  sendDownlink(dst: string, frame: Uint8Array): Promise<boolean> {
    if (this.st.state !== 'connected') return Promise.resolve(false)
    return new Promise((resolve) => {
      const entry = {
        dst: dst.toUpperCase(),
        resolve,
        timer: setTimeout(() => {
          this.pendingSent = this.pendingSent.filter((p) => p !== entry)
          resolve(false)
        }, SENT_TIMEOUT_MS),
      }
      this.pendingSent.push(entry)
      this.writeLine(JSON.stringify({ t: 'down', dst, frame: toHex(frame).toUpperCase() }))
    })
  }

  setChannel(ch: number) {
    if (this.st.state === 'connected') this.writeLine(JSON.stringify({ t: 'ch', ch }))
  }

  protected ping() {
    this.writeLine(JSON.stringify({ t: 'ping' }))
  }

  protected setStatus(patch: Partial<GatewayStatus>) {
    this.st = { ...this.st, ...patch }
    for (const l of this.statusListeners) l(this.status())
  }

  protected log(msg: string) {
    for (const l of this.logListeners) l(msg)
  }

  protected handleLine(raw: string) {
    const line = raw.trim()
    if (!line) return
    if (!line.startsWith('{')) return this.log(`gw: ${line}`) // ブートローダの出力など
    let msg: GatewayLine
    try {
      msg = GatewayLine.parse(JSON.parse(line))
    } catch {
      return this.log(`gw: unparsable line: ${line}`)
    }
    switch (msg.t) {
      case 'hello':
        this.setStatus({ state: 'connected', fw: msg.fw, radio: msg.radio, channel: msg.ch, mac: msg.mac.toUpperCase(), error: undefined })
        return
      case 'up': {
        let frame: Uint8Array
        try {
          frame = fromHex(msg.frame)
        } catch {
          return this.log(`gw: bad frame hex: ${msg.frame}`)
        }
        const station = (this.st.mac ?? '00:00:00:00:00:00').replaceAll(':', '').slice(4)
        for (const l of this.uplinkListeners) l({ src: msg.src.toUpperCase(), rssi: msg.rssi, frame, station })
        return
      }
      case 'sent': {
        const i = this.pendingSent.findIndex((p) => p.dst === msg.dst.toUpperCase())
        if (i < 0) return
        const [p] = this.pendingSent.splice(i, 1)
        clearTimeout(p!.timer)
        p!.resolve(msg.ok)
        return
      }
      case 'log':
        return this.log(`gw: ${msg.msg}`)
    }
  }
}
