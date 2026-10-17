// Frequency of uplink frame 1, which the downlink frequency is derived from (#6), measured with the GFSK carrier
// frequency offset (CFO) register that the LSM100A streamer reports from fw 0.2.0 on.
//
// The register reads about 7 % low (CFO_SCALE), so a reading far from the receiver frequency is only good to a
// few hundred Hz even after correcting for that. To get well within the device's 4.8 kHz receive bandwidth, the
// receiver is tuned to the coarse frame 1 reading before frame 2 starts. Frames 2 and 3 (frame 1 +/- 6 kHz,
// sigfox-ep-lib SIGFOX_EP_FREQUENCY_compute_uplink) then land close to the receiver frequency, and frame 1 is
// their midpoint; on hardware this agreed with the direct reading within 122 Hz.

const XTAL_HZ = 32_000_000
const STREAMER_BIT_RATE = 50_000 // the streamer's GFSK setting (firmware-lsm100a configureRx)
export const CFO_EVERY = 8 // one CFO value per 8 RSSI samples
export const RC3_DELTA_F_MF_HZ = 6000
// Measured on hardware (LSM100A, RX_BW 234 kHz): one frame read 50 171 Hz from one receiver frequency and 3 784 Hz
// from another 50 171 Hz higher, so the register gives 0.925 of the offset; frames 2/3 came out 11.1-11.4 kHz
// apart instead of 12 kHz (0.926-0.946)
export const CFO_SCALE = 0.93

const ABOVE_DB = 15 // a sample this far above the noise floor counts as signal
const SETTLE_S = 0.3 // CFO readings are trusted this long into a frame and after a retune
const GAP_S = 0.1 // a longer gap ends a frame (the DBPSK dips are shorter)
const QUIET_S = 10 // a longer gap ends a message: back to the base frequency
const KEEP_S = 60
// The scale is known to about 1 %, and only from readings up to 50 kHz: a message read further than this from the
// receiver frequency gets no frequency rather than one that may miss the device's window (+/- 1 kHz is the aim)
const MAX_OFFSET_HZ = 10_000

// Hz per CFO LSB, as in SUBGRF_GetCFO (STM32CubeWL): the RX_BW code has the BWSELR layout
export function cfoStepHz(bwCode: number, bitRate = STREAMER_BIT_RATE) {
  const mant = [4, 8, 10, 12][(bwCode >> 3) & 3]!
  const cfFs = XTAL_HZ / (mant * (1 << ((bwCode & 7) + 1)))
  const osr = Math.floor(cfFs / bitRate)
  const interp = osr < 4 ? 4 : osr < 8 ? 2 : 1
  return (cfFs * interp) / 4096
}

export const cfoSigned = (v: number) => (v & 0x800 ? v - 0x1000 : v)

const median = (xs: number[]) => {
  const s = [...xs].sort((a, b) => a - b)
  return s[Math.floor(s.length / 2)]!
}

export type FrameSpan = { rank: number; time: number; end: number } // stream seconds

// Follows the stream sample by sample. Sample indices count from the start of the stream, like SigfoxReceiver.
export class FrequencyTracker {
  private n = 0 // samples so far
  private lineStart = 0
  private tuning: { at: number; freq: number }[] = []
  private readings: { at: number; lo: number; hz: number }[] = [] // lo + CFO
  private floor = Number.NaN
  private frameFrom = -1
  private lastSignal = -Infinity
  private retuned = false
  private requested: number | null = null

  constructor(
    readonly rate: number,
    readonly baseFreq: number,
    readonly stepHz = cfoStepHz(0x0a),
  ) {}

  get freq() {
    return this.tuning.at(-1)?.freq ?? this.baseFreq
  }

  // The streamer's status line reports the frequency it is tuned to
  tuned(freq: number) {
    if (this.requested === freq) this.requested = null
    if (freq !== this.freq || !this.tuning.length) this.tuning.push({ at: this.n, freq })
  }

  // One R line of RSSI samples (dBm)
  samples(dbm: ArrayLike<number>) {
    this.lineStart = this.n
    for (let i = 0; i < dbm.length; i++, this.n++) {
      const v = dbm[i]!
      if (Number.isNaN(this.floor)) this.floor = v
      if (v > this.floor + ABOVE_DB) {
        if (this.n - this.lastSignal > QUIET_S * this.rate) this.retuned = false // a new message
        if (this.n - this.lastSignal > GAP_S * this.rate) this.frameFrom = this.n
        this.lastSignal = this.n
      } else {
        this.floor += (v - this.floor) / this.rate // about one second to follow the noise floor
      }
    }
    const keep = this.n - KEEP_S * this.rate
    if (this.readings[0] && this.readings[0].at < keep) this.readings = this.readings.filter((r) => r.at >= keep)
    if (this.tuning.length > 1 && this.tuning[1]!.at < keep) this.tuning = this.tuning.filter((t, i, a) => i === a.length - 1 || a[i + 1]!.at >= keep)
  }

  // The C line for the last R line: raw 12-bit register values
  cfo(raw: number[]) {
    const settle = SETTLE_S * this.rate
    const tunedAt = this.tuning.at(-1)?.at ?? 0
    for (const [k, v] of raw.entries()) {
      const at = this.lineStart + k * CFO_EVERY
      if (at < tunedAt + settle && this.tuning.length > 1) continue // the register is still catching up
      this.readings.push({ at, lo: this.freq, hz: this.freq + (cfoSigned(v) * this.stepHz) / CFO_SCALE })
    }
  }

  // Where to tune now, or null: to the coarse frame 1 reading once it has settled, and back to the base
  // frequency after a message
  nextTuning(): number | null {
    if (this.requested !== null) return null
    const settle = SETTLE_S * this.rate
    if (this.n - this.lastSignal > QUIET_S * this.rate) {
      return this.freq !== this.baseFreq ? (this.requested = this.baseFreq) : null
    }
    if (this.retuned || this.n - this.lastSignal > GAP_S * this.rate || this.n - this.frameFrom < settle) return null
    const hz = this.readings.filter((r) => r.at >= this.frameFrom + this.rate * GAP_S).map((r) => r.hz)
    if (hz.length < 5) return null
    this.retuned = true
    const target = Math.round(median(hz))
    return target !== this.freq ? (this.requested = target) : null
  }

  // Frequency of one frame against the last receiver frequency used during it (the closest one after a retune)
  frameFrequency(f: FrameSpan): { hz: number; lo: number } | null {
    const from = Math.round((f.time + SETTLE_S) * this.rate)
    const to = Math.round(f.end * this.rate)
    const inFrame = this.readings.filter((r) => r.at >= from && r.at < to)
    const lo = inFrame.at(-1)?.lo
    const hz = inFrame.filter((r) => r.lo === lo).map((r) => r.hz)
    return lo === undefined || hz.length < 5 ? null : { hz: median(hz), lo }
  }

  // Frame 1 of a message: the midpoint of frames 2 and 3 when the device sent them at frame 1 +/- 6 kHz and they
  // were measured with the same receiver frequency, otherwise frame 1 itself. Null unless it was measured close
  // to the receiver frequency.
  frame1(frames: FrameSpan[], downlink: boolean): number | null {
    const at = (rank: number) => {
      const f = frames.find((x) => x.rank === rank)
      return f ? this.frameFrequency(f) : null
    }
    const [f1, f2, f3] = [at(0), at(1), at(2)]
    if (downlink && f2 && f3 && f2.lo === f3.lo) {
      // a plausible spacing (12 kHz) guards against a frame from another device
      const spacing = f2.hz - f3.hz
      const mid = (f2.hz + f3.hz) / 2
      if (spacing > 1.5 * RC3_DELTA_F_MF_HZ && spacing < 2.5 * RC3_DELTA_F_MF_HZ && Math.abs(mid - f2.lo) <= MAX_OFFSET_HZ) {
        return Math.round(mid)
      }
    }
    return f1 && Math.abs(f1.hz - f1.lo) <= MAX_OFFSET_HZ ? Math.round(f1.hz) : null
  }
}
