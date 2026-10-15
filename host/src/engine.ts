import { appliesTo, runCallback } from './callbacks'
import type { Auth, CallbackLog, Device, Message, Store } from './db'
import { decodeFrame, encodeFrame, fromHex, toHex, TYPE_DOWNLINK, TYPE_UPLINK, verifyMac, type Frame } from './frame'
import type { Gateway, GatewayStatus, Uplink } from './gateway/gateway'
import { decodePayload } from './payload'

export type EngineEvent =
  | { type: 'message'; message: Message }
  | { type: 'gateway'; status: GatewayStatus }
  | { type: 'callbackLog'; log: CallbackLog }
  | { type: 'console'; line: string; time: number }

type Pending = { frame: Frame; src: string; station: string; rssi: number; nbFrames: number; auth: Auth; time: number }

// 同じ (デバイス, seq) の遅れて届いた repeat を無視する期間
const RECENT_MS = 10_000

export class Engine {
  private pending = new Map<string, Pending>()
  private recent = new Map<string, number>()
  private listeners: ((e: EngineEvent) => void)[] = []

  constructor(
    readonly store: Store,
    readonly gateway: Gateway,
  ) {
    gateway.onUplink((u) => this.handleUplink(u))
    gateway.onStatus((status) => this.emit({ type: 'gateway', status }))
    gateway.onLog((line) => this.console(line))
  }

  on(cb: (e: EngineEvent) => void) {
    this.listeners.push(cb)
    return () => {
      this.listeners = this.listeners.filter((l) => l !== cb)
    }
  }

  private emit(e: EngineEvent) {
    for (const l of this.listeners) l(e)
  }

  console(line: string) {
    this.emit({ type: 'console', line, time: Date.now() })
  }

  handleUplink(u: Uplink) {
    let frame: Frame
    try {
      frame = decodeFrame(u.frame)
    } catch (e) {
      return this.console(`drop frame from ${u.src}: ${(e as Error).message}`)
    }
    if (frame.type !== TYPE_UPLINK) return
    const key = `${frame.deviceId}:${frame.seq}:${toHex(frame.mac)}`
    const now = Date.now()
    for (const [k, t] of this.recent) if (now - t > RECENT_MS) this.recent.delete(k)
    if (this.recent.has(key)) return

    const p = this.pending.get(key)
    if (p) {
      p.nbFrames++
      p.rssi = Math.max(p.rssi, u.rssi)
      return
    }
    const device = this.store.getDevice(frame.deviceId)
    const auth: Auth = device?.key ? (verifyMac(fromHex(device.key), frame) ? 'ok' : 'bad') : 'unknown'
    this.pending.set(key, { frame, src: u.src, station: u.station, rssi: u.rssi, nbFrames: 1, auth, time: now })
    setTimeout(() => {
      const entry = this.pending.get(key)!
      this.pending.delete(key)
      this.recent.set(key, Date.now())
      void this.finalize(entry).catch((e) => this.console(`error: ${(e as Error).stack ?? e}`))
    }, this.store.getSettings().dedupWindowMs)
  }

  private async finalize(p: Pending) {
    const settings = this.store.getSettings()
    const f = p.frame
    let device: Device | null = this.store.getDevice(f.deviceId)
    if (!device) {
      device = this.store.upsertDevice({ id: f.deviceId, name: '', key: '', downlinkMode: 'none', downlinkData: '', payloadConfig: '' })
    }
    this.store.touchDevice(f.deviceId, f.seq, p.time)

    let decoded: Record<string, unknown> | null = null
    if (device.payloadConfig && f.payload.length) {
      try {
        decoded = decodePayload(device.payloadConfig, f.payload)
      } catch (e) {
        decoded = { _error: (e as Error).message }
      }
    }
    let message = this.store.insertMessage({
      deviceId: f.deviceId,
      time: p.time,
      seq: f.seq,
      data: toHex(f.payload),
      ack: f.ack,
      oob: f.oob,
      rssi: p.rssi,
      nbFrames: p.nbFrames,
      auth: p.auth,
      src: p.src,
      station: p.station,
      decoded,
      downlink: null,
      downlinkStatus: f.ack ? 'pending' : 'none',
    })
    this.emit({ type: 'message', message })

    // 鍵が合わないフレームはなりすましの可能性があるのでコールバックしない
    const trusted = p.auth === 'ok' || (p.auth === 'unknown' && settings.acceptUnknownDevices)
    const callbacks = trusted ? this.store.listCallbacks().filter((c) => c.enabled && appliesTo(c, f.deviceId)) : []
    const results = await Promise.all(
      callbacks.map(async (cb) => {
        const r = await runCallback(cb, message, device.name, settings.downlinkTimeoutMs)
        const log = this.store.insertLog({
          callbackId: cb.id,
          messageId: message.id,
          time: Date.now(),
          status: r.status,
          ok: r.ok,
          error: r.error,
          durationMs: r.durationMs,
          response: r.response,
        })
        this.emit({ type: 'callbackLog', log })
        return r
      }),
    )
    if (!f.ack) return

    let data: string | null = null
    if (device.downlinkMode === 'static' && device.downlinkData) data = device.downlinkData
    if (device.downlinkMode === 'callback') data = results.find((r) => r.downlink)?.downlink ?? null
    if (!data || !device.key || p.auth !== 'ok') {
      if (data && !device.key) this.console(`downlink for ${f.deviceId} skipped: device key not registered`)
      message = this.store.updateMessage(message.id, { downlinkStatus: 'nodata' })!
      return this.emit({ type: 'message', message })
    }
    const frame = encodeFrame({ type: TYPE_DOWNLINK, deviceId: f.deviceId, seq: f.seq, payload: fromHex(data), key: fromHex(device.key) })
    const ok = await this.gateway.sendDownlink(p.src, frame)
    message = this.store.updateMessage(message.id, { downlink: data, downlinkStatus: ok ? 'sent' : 'failed' })!
    this.emit({ type: 'message', message })
  }
}
