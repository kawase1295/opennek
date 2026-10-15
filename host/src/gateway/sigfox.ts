import { type SigfoxMessage, SigfoxReceiver } from '../sigfox/receiver'
import { type LinePort, openLinePort } from './serial'

// Real Sigfox RC3 uplinks from the OpenNEK RSSI streamer (firmware-lsm100a, usually behind the ESP32-S3
// bridge). The streamer sends "I fw=.. f=.. bw=.. sr=.. boost=.." status lines and "R<seq>:<hex>" sample lines.
export type RadioStatus = {
  state: 'off' | 'disconnected' | 'connecting' | 'connected'
  port: string | null
  fw?: string
  freq?: number
  rate?: number
  error?: string
  frames: number // frames with a valid CRC
  badFrames: number
  lastFrameAt?: number
  lostLines: number
}

export type RadioMessage = SigfoxMessage & { receivedAt: number } // wall-clock ms of the first frame

const RETRY_MS = 2000
const STATUS_TIMEOUT_MS = 3000
const STALL_MS = 3000 // no samples for this long means the streamer is gone
const DEFAULT_RATE = 2000

type Listener<T> = (v: T) => void

export class SigfoxRadio {
  private st: RadioStatus = { state: 'off', port: null, frames: 0, badFrames: 0, lostLines: 0 }
  private port: LinePort | null = null
  private closing: Promise<void> = Promise.resolve()
  private stopped = true
  private retry: Timer | null = null
  private watchdog: Timer | null = null
  private rx: SigfoxReceiver | null = null
  private lastSeq = -1
  private lastSample = 0
  private statusListeners: Listener<RadioStatus>[] = []
  private messageListeners: Listener<RadioMessage>[] = []
  private logListeners: Listener<string>[] = []

  // port: '' disables the receiver; freq in Hz
  constructor(private settings: () => { port: string; freq: number }) {}

  status(): RadioStatus {
    return { ...this.st }
  }
  onStatus(cb: Listener<RadioStatus>) {
    this.statusListeners.push(cb)
  }
  onMessage(cb: Listener<RadioMessage>) {
    this.messageListeners.push(cb)
  }
  onLog(cb: Listener<string>) {
    this.logListeners.push(cb)
  }

  start() {
    this.stopped = false
    void this.connect()
  }

  async stop() {
    this.stopped = true
    if (this.retry) clearTimeout(this.retry)
    this.close()
    await this.closing
  }

  reconnect() {
    this.close()
    if (!this.stopped) void this.connect()
  }

  // Feeds one streamer line; public so recordings can be replayed without a port
  handleLine(raw: string) {
    const line = raw.replace(/\r$/, '')
    if (line.startsWith('I ')) {
      const field = (k: string) => new RegExp(`\\b${k}=(\\S+)`).exec(line)?.[1]
      const rate = Number(field('sr') ?? DEFAULT_RATE)
      if (!this.rx || this.rx.rate !== rate) this.startReceiver(rate)
      this.setStatus({ state: 'connected', fw: field('fw'), freq: Number(field('f')) || undefined, rate, error: undefined })
      return
    }
    if (!(line.startsWith('R') && line[5] === ':')) {
      if (line.trim()) this.log(`sigfox: ${line.trim()}`)
      return
    }
    const seq = Number.parseInt(line.slice(1, 5), 16)
    if (this.lastSeq >= 0 && seq !== ((this.lastSeq + 1) & 0xffff)) this.st.lostLines += (seq - this.lastSeq - 1 + 0x10000) & 0xffff
    this.lastSeq = seq
    const hex = line.slice(6)
    const samples = new Uint8Array(hex.length >> 1)
    for (let i = 0; i < samples.length; i++) samples[i] = Number.parseInt(hex.slice(i * 2, i * 2 + 2), 16)
    this.lastSample = Date.now()
    if (!this.rx) this.startReceiver(DEFAULT_RATE)
    if (this.st.state !== 'connected') this.setStatus({ state: 'connected', rate: this.rx!.rate, error: undefined })
    this.rx!.pushRaw(samples)
  }

  // Emits messages still waiting for their repetitions (end of a replay)
  flush() {
    this.rx?.flush()
  }

  private startReceiver(rate: number) {
    this.rx?.flush()
    const rx = new SigfoxReceiver(
      rate,
      (m) => {
        // stream time -> wall clock: the stream is (rx.time - m.time) seconds ahead of the first frame
        const receivedAt = Date.now() - Math.round((rx.time - m.time) * 1000)
        for (const l of this.messageListeners) l({ ...m, receivedAt })
      },
      (f) => {
        if (f.crcOk) this.setStatus({ frames: this.st.frames + 1, lastFrameAt: Date.now() })
        else this.setStatus({ badFrames: this.st.badFrames + 1 })
        const crc = f.crcOk ? (f.repaired ? `CRC ok (${f.repaired})` : 'CRC ok') : 'CRC error'
        this.log(`sigfox: frame ${f.rank + 1}/3 ${f.deviceId} seq=${f.counter} ${crc} rssi=${f.rssi.toFixed(1)}`)
      },
    )
    this.rx = rx
  }

  private async connect() {
    await this.closing
    if (this.stopped || this.port) return
    const { port: path, freq } = this.settings()
    if (!path) return this.setStatus({ state: 'off', port: null, error: undefined })
    this.setStatus({ state: 'connecting', port: path, error: undefined })
    try {
      this.port = openLinePort(path, (line) => this.handleLine(line), (e) => this.fail(e))
    } catch (e) {
      this.setStatus({ state: 'disconnected', error: e instanceof Error ? e.message : String(e) })
      return this.scheduleRetry()
    }
    this.log(`sigfox: opened ${path}`)
    this.lastSeq = -1
    this.lastSample = Date.now()
    this.port.write(`F${freq}`)
    this.port.write('?')
    this.watchdog = setInterval(() => {
      if (Date.now() - this.lastSample < Math.max(STALL_MS, STATUS_TIMEOUT_MS)) return
      this.setStatus({ state: 'connecting', error: 'RSSI ストリーマから応答がありません (firmware-lsm100a を書き込み、115200 bps で接続してください)' })
      try {
        this.port?.write('?')
      } catch (e) {
        this.fail(e)
      }
    }, 1000)
  }

  private fail(e: unknown) {
    if (!this.port) return
    this.log(`sigfox: ${e instanceof Error ? e.message : String(e)}`)
    this.close()
    this.setStatus({ state: 'disconnected', error: '受信機との接続が切れました' })
    this.scheduleRetry()
  }

  private close() {
    if (this.watchdog) clearInterval(this.watchdog)
    this.watchdog = null
    this.rx?.flush()
    this.rx = null
    if (this.port) this.closing = this.port.close()
    this.port = null
    if (this.st.state === 'connected' || this.st.state === 'connecting') this.setStatus({ state: 'disconnected' })
  }

  private scheduleRetry() {
    if (this.stopped) return
    if (this.retry) clearTimeout(this.retry)
    this.retry = setTimeout(() => void this.connect(), RETRY_MS)
  }

  private setStatus(patch: Partial<RadioStatus>) {
    this.st = { ...this.st, ...patch }
    for (const l of this.statusListeners) l(this.status())
  }

  private log(msg: string) {
    for (const l of this.logListeners) l(msg)
  }
}
