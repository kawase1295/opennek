// Streaming Sigfox receiver: takes RSSI samples as they arrive, decodes complete frames and merges the up
// to three repetitions of a message into one.
import { type DecodedFrame, decodeBurst, findBursts, noiseFloor } from './demod'
import type { FrameSpan } from './frequency'

export type SigfoxMessage = {
  deviceId: string
  counter: number
  payload: Uint8Array
  bit: 0 | 1 | null
  downlink: boolean
  control: boolean // keep-alive or downlink confirmation
  auth: Uint8Array
  time: number // stream time of the first frame received, in seconds
  firstFrameEnd: number | null // stream time of the end of frame 1 (rank 0), which starts the device's downlink timer
  rssi: number // best RSSI of the received frames (dBm)
  ranks: number[] // which repetitions (0..2) were received with a valid CRC
  frames: FrameSpan[] // when each of them was on air
  clockErrorPpm: number
}

const BUFFER_S = 12
const STEP_S = 0.5
const TAIL_S = 0.3 // a burst counts as finished once it ended this long before the newest sample
// Repetitions start ~3.7-4 s apart and a frame is only decoded ~2 s after it starts, so wait this long
// after the latest repetition before calling a message complete
const QUIET_S = 10
// From the end of one repetition to the start of the next; the same for 1- and 5-byte payloads (AX-SFJK, RC3)
export const FRAME_GAP_S = 2.43
const SAME_BURST_S = 0.3 // the detected start of a burst moves a little between scans

export class SigfoxReceiver {
  private buf: Float32Array
  private len = 0 // samples in buf
  private base = 0 // absolute index of buf[0]
  private sinceScan = 0
  private seen: number[] = [] // absolute start samples of bursts already decoded
  private pending = new Map<string, SigfoxMessage & { last: number }>()

  constructor(
    readonly rate: number,
    private onMessage: (m: SigfoxMessage) => void,
    private onFrame: (f: DecodedFrame) => void = () => {},
  ) {
    this.buf = new Float32Array(Math.round(BUFFER_S * rate))
  }

  get time() {
    return (this.base + this.len) / this.rate
  }

  // Raw streamer values (dBm = -value / 2)
  pushRaw(raw: ArrayLike<number>) {
    const dbm = new Float32Array(raw.length)
    for (let i = 0; i < raw.length; i++) dbm[i] = -raw[i]! / 2
    this.push(dbm)
  }

  push(dbm: Float32Array) {
    for (let off = 0; off < dbm.length; ) {
      if (this.len === this.buf.length) {
        // keep the newest half
        const keep = this.buf.length >> 1
        this.buf.copyWithin(0, this.len - keep, this.len)
        this.base += this.len - keep
        this.len = keep
      }
      const n = Math.min(dbm.length - off, this.buf.length - this.len)
      this.buf.set(dbm.subarray(off, off + n), this.len)
      this.len += n
      off += n
      this.sinceScan += n
      if (this.sinceScan >= STEP_S * this.rate) {
        this.sinceScan = 0
        this.scan()
      }
    }
    this.expire(false)
  }

  // Emits everything still pending (end of a capture)
  flush() {
    this.scan(true)
    this.expire(true)
  }

  private scan(final = false) {
    const view = this.buf.subarray(0, this.len)
    const tail = final ? 0 : Math.round(TAIL_S * this.rate)
    for (const b of findBursts(view, this.rate, undefined, noiseFloor(view))) {
      // Only bursts that have ended; ones touching the start of the buffer were decoded earlier
      if (b.end > this.len - tail || b.start === 0) continue
      const start = this.base + b.start
      if (this.seen.some((s) => Math.abs(s - start) < SAME_BURST_S * this.rate)) continue
      this.seen.push(start)
      const frame = decodeBurst(view, this.rate, b, this.base / this.rate)
      if (!frame) continue
      this.onFrame(frame)
      if (frame.crcOk) this.merge(frame)
    }
    this.seen = this.seen.filter((s) => s >= this.base - this.rate)
  }

  private merge(f: DecodedFrame) {
    const key = `${f.deviceId}:${f.counter}`
    const m = this.pending.get(key)
    if (m) {
      if (!m.ranks.includes(f.rank)) {
        m.ranks.push(f.rank)
        m.frames.push({ rank: f.rank, time: f.time, end: f.end })
      }
      m.rssi = Math.max(m.rssi, f.rssi)
      m.last = f.time
      if (f.rank === 0) m.firstFrameEnd = f.end
      return
    }
    this.pending.set(key, {
      deviceId: f.deviceId,
      counter: f.counter,
      payload: f.payload,
      bit: f.bit,
      downlink: f.downlink,
      control: f.control,
      auth: f.auth,
      time: f.time,
      firstFrameEnd: f.rank === 0 ? f.end : null,
      rssi: f.rssi,
      ranks: [f.rank],
      frames: [{ rank: f.rank, time: f.time, end: f.end }],
      clockErrorPpm: f.clockErrorPpm,
      last: f.time,
    })
  }

  private expire(all: boolean) {
    for (const [key, m] of this.pending) {
      if (all || m.ranks.length === 3 || this.time - m.last > QUIET_S) {
        this.pending.delete(key)
        const { last: _, ...message } = m
        message.ranks.sort()
        message.firstFrameEnd ??= estimateFirstFrameEnd(message.frames)
        this.onMessage(message)
      }
    }
  }
}

// When frame 1 was lost (often when the receiver retuned during it), its end from a later repetition: all three
// last as long and are FRAME_GAP_S apart. The device starts its downlink timer there.
export function estimateFirstFrameEnd(frames: FrameSpan[]): number | null {
  const f = frames.filter((x) => x.rank > 0).sort((a, b) => a.rank - b.rank)[0]
  return f ? f.end - f.rank * (f.end - f.time + FRAME_GAP_S) : null
}
