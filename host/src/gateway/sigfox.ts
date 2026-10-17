import { toHex } from '../frame'
import { FrequencyTracker } from '../sigfox/frequency'
import { type SigfoxMessage, SigfoxReceiver } from '../sigfox/receiver'
import { type LinePort, openLinePort } from './serial'

// Real Sigfox RC3 uplinks from the OpenNEK RSSI streamer (firmware-lsm100a, usually behind the ESP32-S3
// bridge). The streamer sends "I fw=.. f=.. bw=.. sr=.. boost=.." status lines, "R<seq>:<hex>" sample lines and,
// from fw 0.2.0, "C<seq>:<hex>" CFO lines that give the frequency of each frame (sigfox/frequency.ts).
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

export type RadioMessage = SigfoxMessage & {
  receivedAt: number // wall-clock ms of the first frame
  firstFrameEndAt: number | null // wall-clock ms of the end of frame 1
  // Carrier frequency of frame 1, which the downlink frequency is derived from. Null when it could not be measured
  // closely enough: streamer before 0.2.0, frequency following off, or frames missed
  uplinkFreqHz: number | null
}

// Sends Sigfox downlinks (an SX1262 radio, #6). The transmitter owns the regulatory side (power cap, carrier
// sense, transmit-time limits) and resolves false when it did not transmit, e.g. after the deadline.
export type SigfoxDownlinkTx = { deviceId: string; counter: number; freqHz: number; frame: Uint8Array; deadline: number }
export interface SigfoxTransmitter {
  transmit(tx: SigfoxDownlinkTx): Promise<boolean>
}

// Downlinks from the streamer itself (firmware-lsm100a 0.3.0): "T<hz>,<dBm>,<hex>" answered by "D <result>". The
// firmware enforces the channel, the power cap, carrier sense and the transmit-time limits; TX is allowed with "E1"
// only around each transmission.
const TX_FW = [0, 3, 0]
const TX_DBM = 10 // the firmware's maximum
const TX_REPLY_MS = 2000 // carrier sense and 280 ms on air
const TX_RETRY_MS = 200 // after "busy": the channel was in use

const RETRY_MS = 2000
const STATUS_TIMEOUT_MS = 3000
const STALL_MS = 3000 // no samples for this long means the streamer is gone
const DEFAULT_RATE = 2000

type Listener<T> = (v: T) => void

const atLeast = (fw: string | undefined, min: number[]) => {
  const v = (fw ?? '').split('.').map(Number)
  for (const [i, m] of min.entries()) if ((v[i] ?? 0) !== m) return (v[i] ?? 0) > m
  return true
}

export class SigfoxRadio implements SigfoxTransmitter {
  private st: RadioStatus = { state: 'off', port: null, frames: 0, badFrames: 0, lostLines: 0 }
  private port: LinePort | null = null
  private closing: Promise<void> = Promise.resolve()
  private stopped = true
  private retry: Timer | null = null
  private watchdog: Timer | null = null
  private rx: SigfoxReceiver | null = null
  private tracker: FrequencyTracker | null = null
  private txReply: ((result: string) => void) | null = null
  private txBusy = false
  private lastSeq = -1
  private lastSample = 0
  private statusListeners: Listener<RadioStatus>[] = []
  private messageListeners: Listener<RadioMessage>[] = []
  private logListeners: Listener<string>[] = []

  // port: '' disables the receiver; freq in Hz. follow: tune to frame 1 of each message so that its frequency can
  // be measured for a downlink, then back to freq
  constructor(private settings: () => { port: string; freq: number; follow?: boolean }) {}

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

  // One downlink frame, retried while the channel is busy until the deadline (wall-clock ms)
  async transmit(tx: SigfoxDownlinkTx): Promise<boolean> {
    if (!this.port || this.st.state !== 'connected') return this.txFailed('receiver not connected')
    if (!atLeast(this.st.fw, TX_FW)) return this.txFailed(`firmware-lsm100a ${TX_FW.join('.')} or later needed (fw=${this.st.fw ?? '?'})`)
    if (this.txBusy) return this.txFailed('another downlink is being sent')
    this.txBusy = true
    try {
      this.write('E1')
      for (;;) {
        if (Date.now() > tx.deadline) return this.txFailed("the device's receive window has closed")
        const result = await this.command(`T${tx.freqHz},${TX_DBM},${toHex(tx.frame).toUpperCase()}`)
        this.log(`sigfox: TX ${(tx.freqHz / 1e6).toFixed(6)} MHz ${TX_DBM} dBm: ${result ?? 'no reply'}`)
        if (result?.startsWith('ok')) return true
        if (!result?.startsWith('busy')) return false
        await Bun.sleep(TX_RETRY_MS)
      }
    } finally {
      this.write('E0')
      this.txBusy = false
    }
  }

  private txFailed(why: string) {
    this.log(`sigfox: TX not sent: ${why}`)
    return false
  }

  private command(line: string): Promise<string | null> {
    return new Promise((resolve) => {
      const timer = setTimeout(() => {
        this.txReply = null
        resolve(null)
      }, TX_REPLY_MS)
      this.txReply = (result) => {
        clearTimeout(timer)
        this.txReply = null
        resolve(result)
      }
      this.write(line)
    })
  }

  private write(line: string) {
    try {
      this.port?.write(line)
    } catch (e) {
      this.fail(e)
    }
  }

  // Feeds one streamer line; public so recordings can be replayed without a port
  handleLine(raw: string) {
    const line = raw.replace(/\r$/, '')
    if (line.startsWith('D ')) {
      this.txReply?.(line.slice(2).trim())
      return
    }
    if (line.startsWith('I ')) {
      const field = (k: string) => new RegExp(`\\b${k}=(\\S+)`).exec(line)?.[1]
      const rate = Number(field('sr') ?? DEFAULT_RATE)
      if (!this.rx || this.rx.rate !== rate) this.startReceiver(rate)
      const freq = Number(field('f'))
      if (freq) this.tracker!.tuned(freq)
      this.setStatus({ state: 'connected', fw: field('fw'), freq: Number(field('f')) || undefined, rate, error: undefined })
      return
    }
    if (line.startsWith('C') && line[5] === ':') {
      // CFO values for the R line with the same seq, which came just before
      if (Number.parseInt(line.slice(1, 5), 16) !== this.lastSeq) return
      const hex = line.slice(6)
      const raw: number[] = []
      for (let i = 0; i + 3 <= hex.length; i += 3) raw.push(Number.parseInt(hex.slice(i, i + 3), 16))
      this.tracker?.cfo(raw)
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
    this.tracker!.samples(Array.from(samples, (v) => -v / 2))
    if (this.settings().follow) {
      const freq = this.tracker!.nextTuning()
      if (freq !== null) this.tune(freq)
    }
  }

  // Emits messages still waiting for their repetitions (end of a replay)
  flush() {
    this.rx?.flush()
  }

  private tune(freq: number) {
    this.log(`sigfox: tuning to ${(freq / 1e6).toFixed(6)} MHz`)
    this.write(`F${freq}`)
  }

  private startReceiver(rate: number) {
    this.rx?.flush()
    const tracker = new FrequencyTracker(rate, this.settings().freq)
    this.tracker = tracker
    const rx = new SigfoxReceiver(
      rate,
      (m) => {
        // stream time -> wall clock: the stream is (rx.time - t) seconds ahead of t
        const now = Date.now()
        const wall = (t: number) => now - Math.round((rx.time - t) * 1000)
        const firstFrameEndAt = m.firstFrameEnd === null ? null : wall(m.firstFrameEnd)
        const uplinkFreqHz = tracker.frame1(m.frames, m.downlink)
        if (m.downlink) {
          // receiver frequency + CFO of each frame, to check the measurement
          const readings = m.frames.map((f) => {
            const r = tracker.frameFrequency(f)
            const cfo = r ? Math.round(r.hz - r.lo) : 0
            return r ? `${f.rank + 1}: ${(r.lo / 1e6).toFixed(6)} ${cfo >= 0 ? '+' : ''}${cfo} Hz` : `${f.rank + 1}: -`
          })
          const at = uplinkFreqHz === null ? 'not measured' : `at ${(uplinkFreqHz / 1e6).toFixed(6)} MHz`
          this.log(`sigfox: frame 1 of ${m.deviceId} seq=${m.counter} ${at} (${readings.join(', ')})`)
        }
        for (const l of this.messageListeners) l({ ...m, receivedAt: wall(m.time), firstFrameEndAt, uplinkFreqHz })
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
    this.port.write('C1') // CFO lines (fw 0.2.0); older streamers ignore it
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
