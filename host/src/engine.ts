import { appliesTo, runCallback } from './callbacks'
import type { Auth, CallbackLog, Device, Message, Radio, Store } from './db'
import { decodeFrame, encodeFrame, fromHex, toHex, TYPE_DOWNLINK, TYPE_UPLINK, verifyMac, type Frame } from './frame'
import type { Gateway, GatewayStatus, Uplink } from './gateway/gateway'
import type { RadioMessage, RadioStatus, SigfoxRadio, SigfoxTransmitter } from './gateway/sigfox'
import { decodePayload } from './payload'
import { PUBLIC_KEY, verifyUplink } from './sigfox/auth'
import { downlinkFrequency, downlinkWindow, encodeDownlink } from './sigfox/downlink'

export type EngineEvent =
  | { type: 'message'; message: Message }
  | { type: 'gateway'; status: GatewayStatus }
  | { type: 'radio'; status: RadioStatus }
  | { type: 'callbackLog'; log: CallbackLog }
  | { type: 'console'; line: string; time: number }

type Pending = { frame: Frame; src: string; station: string; rssi: number; nbFrames: number; auth: Auth; time: number }

// 受信経路によらない、まとめ終わったアップリンク
type Incoming = {
  radio: Radio
  deviceId: string
  seq: number
  payload: Uint8Array
  ack: boolean
  oob: boolean
  rssi: number
  nbFrames: number
  auth: Auth
  src: string
  station: string
  time: number
  // 本物の Sigfox のダウンリンクに必要な、フレーム 1 の終了時刻と周波数
  firstFrameEndAt: number | null
  uplinkFreqHz: number | null
}

// 同じ (デバイス, seq) の遅れて届いた repeat を無視する期間
const RECENT_MS = 10_000
// Sigfox はメッセージの最後の repeat から 10 秒待ってまとめるので、それより遅れた repeat を無視する期間
const SIGFOX_RECENT_MS = 60_000
// 本物の Sigfox を受けたときの station 変数 (Sigfox の基地局 ID の代わり)
const SIGFOX_STATION = '0000'

export class Engine {
  private pending = new Map<string, Pending>()
  private recent = new Map<string, number>()
  private listeners: ((e: EngineEvent) => void)[] = []

  constructor(
    readonly store: Store,
    readonly gateway: Gateway,
    readonly sigfox: SigfoxRadio | null = null,
    readonly sigfoxTx: SigfoxTransmitter | null = null,
  ) {
    gateway.onUplink((u) => this.handleUplink(u))
    gateway.onStatus((status) => this.emit({ type: 'gateway', status }))
    gateway.onLog((line) => this.console(line))
    sigfox?.onMessage((m) => this.handleSigfox(m))
    sigfox?.onStatus((status) => this.emit({ type: 'radio', status }))
    sigfox?.onLog((line) => this.console(line))
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
      const p = this.pending.get(key)!
      this.pending.delete(key)
      this.recent.set(key, Date.now())
      const f = p.frame
      const incoming: Incoming = {
        radio: 'espnow', deviceId: f.deviceId, seq: f.seq, payload: f.payload, ack: f.ack, oob: f.oob,
        rssi: p.rssi, nbFrames: p.nbFrames, auth: p.auth, src: p.src, station: p.station, time: p.time,
        firstFrameEndAt: null, uplinkFreqHz: null,
      }
      void this.finalize(incoming).catch((e) => this.console(`error: ${(e as Error).stack ?? e}`))
    }, this.store.getSettings().dedupWindowMs)
  }

  // 本物の Sigfox アップリンク。repeat は受信機側でまとめ済み
  handleSigfox(m: RadioMessage) {
    const key = `sigfox:${m.deviceId}:${m.counter}`
    const now = Date.now()
    for (const [k, t] of this.recent) if (now - t > Math.max(RECENT_MS, SIGFOX_RECENT_MS)) this.recent.delete(k)
    if (this.recent.has(key)) return this.console(`sigfox: late repetition of ${m.deviceId} seq=${m.counter} ignored`)
    this.recent.set(key, now)
    const incoming: Incoming = {
      radio: 'sigfox',
      deviceId: m.deviceId,
      seq: m.counter,
      // 1 ビットメッセージ (AT$SB) はデータ無し。値は Sigfox と同じく data に入れない
      payload: m.payload,
      ack: m.downlink,
      oob: false,
      rssi: Math.round(m.rssi),
      nbFrames: m.ranks.length,
      auth: this.sigfoxAuth(m),
      src: 'sigfox',
      station: SIGFOX_STATION,
      time: m.receivedAt,
      firstFrameEndAt: m.firstFrameEndAt,
      uplinkFreqHz: m.uplinkFreqHz,
    }
    void this.finalize(incoming).catch((e) => this.console(`error: ${(e as Error).stack ?? e}`))
  }

  // 登録済みの鍵を優先する。公開鍵は誰でも署名できるので、未登録のデバイスと同じ扱いの 'public' にする
  private sigfoxAuth(m: RadioMessage): Auth {
    const key = this.store.getDevice(m.deviceId)?.key
    if (key) return verifyUplink(fromHex(key), m) ? 'ok' : 'bad'
    return verifyUplink(PUBLIC_KEY, m) ? 'public' : 'unverified'
  }

  private async finalize(f: Incoming) {
    const settings = this.store.getSettings()
    let device: Device | null = this.store.getDevice(f.deviceId)
    if (!device) {
      device = this.store.upsertDevice({ id: f.deviceId, name: '', key: '', downlinkMode: 'none', downlinkData: '', payloadConfig: '' })
    }
    this.store.touchDevice(f.deviceId, f.seq, f.time)

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
      time: f.time,
      seq: f.seq,
      data: toHex(f.payload),
      ack: f.ack,
      oob: f.oob,
      rssi: f.rssi,
      nbFrames: f.nbFrames,
      auth: f.auth,
      src: f.src,
      station: f.station,
      radio: f.radio,
      decoded,
      downlink: null,
      downlinkStatus: f.ack ? 'pending' : 'none',
    })
    this.emit({ type: 'message', message })

    // 鍵が合わないフレームはなりすましの可能性があるのでコールバックしない
    const trusted = f.auth === 'ok' || (f.auth !== 'bad' && settings.acceptUnknownDevices)
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
    if (f.radio === 'sigfox') {
      // 公開鍵モードのデバイスには公開鍵で認証値を付ける。未登録デバイスを受け付けない設定ならコールバックと同じく送らない
      const key = f.auth === 'ok' ? fromHex(device.key) : f.auth === 'public' && trusted ? PUBLIC_KEY : null
      return this.sigfoxDownlink(f, message, data, key)
    }
    if (!data || !device.key || f.auth !== 'ok') {
      if (data && !device.key) this.console(`downlink for ${f.deviceId} skipped: device key not registered`)
      message = this.store.updateMessage(message.id, { downlinkStatus: 'nodata' })!
      return this.emit({ type: 'message', message })
    }
    const frame = encodeFrame({ type: TYPE_DOWNLINK, deviceId: f.deviceId, seq: f.seq, payload: fromHex(data), key: fromHex(device.key) })
    const ok = await this.gateway.sendDownlink(f.src, frame)
    message = this.store.updateMessage(message.id, { downlink: data, downlinkStatus: ok ? 'sent' : 'failed' })!
    this.emit({ type: 'message', message })
  }

  // デバイスの受信ウィンドウ (フレーム 1 の終了から T_W 後) まで待って送信機に渡す
  private async sigfoxDownlink(f: Incoming, message: Message, data: string | null, key: Uint8Array | null) {
    const update = (patch: Parameters<Store['updateMessage']>[1]) => {
      message = this.store.updateMessage(message.id, patch)!
      this.emit({ type: 'message', message })
    }
    const name = `sigfox downlink for ${f.deviceId} seq=${f.seq}`
    // 送信機が無い・送信が有効でないときは受信専用 (コールバックには ack=true のまま渡している)
    if (!this.sigfoxTx || !this.store.getSettings().sigfoxDownlink) return update({ downlinkStatus: 'unsupported' })
    if (!data || !key) {
      if (data) this.console(`${name} skipped: uplink not authenticated (auth=${f.auth})`)
      return update({ downlinkStatus: 'nodata' })
    }
    if (f.firstFrameEndAt === null || f.uplinkFreqHz === null) {
      this.console(`${name} skipped: ${f.firstFrameEndAt === null ? 'frame 1 not received' : 'frequency of frame 1 unknown'}`)
      return update({ downlinkStatus: 'unsupported' })
    }
    const { at, deadline } = downlinkWindow(f.firstFrameEndAt)
    if (Date.now() > deadline) {
      this.console(`${name} skipped: the device's receive window has closed`)
      return update({ downlink: data, downlinkStatus: 'failed' })
    }
    const freqHz = downlinkFrequency(f.uplinkFreqHz)
    const frame = encodeDownlink({ deviceId: f.deviceId, counter: f.seq, payload: fromHex(data), key })
    await Bun.sleep(Math.max(0, at - Date.now()))
    // 電波を出すので送信はすべて記録する
    this.console(`${name}: TX ${(freqHz / 1e6).toFixed(6)} MHz data=${data} frame=${toHex(frame)}`)
    const ok = await this.sigfoxTx.transmit({ deviceId: f.deviceId, counter: f.seq, freqHz, frame, deadline }).catch((e) => {
      this.console(`${name}: ${(e as Error).message}`)
      return false
    })
    this.console(`${name}: ${ok ? 'sent' : 'not sent'}`)
    update({ downlink: data, downlinkStatus: ok ? 'sent' : 'failed' })
  }
}
