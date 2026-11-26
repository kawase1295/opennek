// 通信品質のグラフの目盛りと割合

export type Axis<T> = { lo: number; hi: number; ticks: T[] }
export type TimeTick = { time: number; label: string }

const RSSI_STEPS = [1, 2, 5, 10, 20, 50]
const MAX_RSSI_INTERVALS = 4
// 値が 1 つでも縦軸に幅を持たせる (dB)
const MIN_RSSI_SPAN = 10

export function rssiAxis(min: number, max: number): Axis<number> {
  const pad = Math.max(0, MIN_RSSI_SPAN - (max - min)) / 2
  const step = RSSI_STEPS.find((s) => (max - min + 2 * pad) / s <= MAX_RSSI_INTERVALS) ?? 50
  const lo = Math.floor((min - pad) / step) * step
  const hi = Math.ceil((max + pad) / step) * step
  const ticks: number[] = []
  for (let v = lo; v <= hi; v += step) ticks.push(v)
  return { lo, hi, ticks }
}

type Unit = 'second' | 'minute' | 'hour' | 'day'
type Step = { unit: Unit; n: number }
const UNIT_MS: Record<Unit, number> = { second: 1000, minute: 60_000, hour: 3_600_000, day: 86_400_000 }
const TIME_STEPS: Step[] = [
  ...[10, 30].map((n) => ({ unit: 'second' as const, n })),
  ...[1, 5, 15, 30].map((n) => ({ unit: 'minute' as const, n })),
  ...[1, 3, 6, 12].map((n) => ({ unit: 'hour' as const, n })),
  ...[1, 2, 7, 14, 30, 91, 182, 365].map((n) => ({ unit: 'day' as const, n })),
]
// 時刻が 1 つでも横軸に幅を持たせる (ms)
const MIN_TIME_SPAN = 60_000

// 目盛りは現地時刻の切りのいいところに置くので、Date の現地時刻の項目で数える
function advance(d: Date, unit: Unit, n: number): void {
  if (unit === 'day') d.setDate(d.getDate() + n)
  else if (unit === 'hour') d.setHours(d.getHours() + n)
  else if (unit === 'minute') d.setMinutes(d.getMinutes() + n)
  else d.setSeconds(d.getSeconds() + n)
}

// time 以降で最初の目盛り: 日なら次の 0 時、ほかは n の倍数の時刻
function firstTick(time: number, { unit, n }: Step): Date {
  const d = new Date(time)
  if (unit === 'day') d.setHours(0, 0, 0, 0)
  else if (unit === 'hour') d.setHours(d.getHours() - (d.getHours() % n), 0, 0, 0)
  else if (unit === 'minute') d.setMinutes(d.getMinutes() - (d.getMinutes() % n), 0, 0)
  else d.setSeconds(d.getSeconds() - (d.getSeconds() % n), 0)
  if (d.getTime() < time) advance(d, unit, unit === 'day' ? 1 : n)
  return d
}

const pad2 = (v: number): string => String(v).padStart(2, '0')

function timeLabel(d: Date, unit: Unit): string {
  if (d.getHours() === 0 && d.getMinutes() === 0 && d.getSeconds() === 0) return `${d.getMonth() + 1}/${d.getDate()}`
  const hm = `${pad2(d.getHours())}:${pad2(d.getMinutes())}`
  return unit === 'second' ? `${hm}:${pad2(d.getSeconds())}` : hm
}

// first から last (ms) の横軸。目盛りは maxTicks 本まで
export function timeAxis(first: number, last: number, maxTicks: number): Axis<TimeTick> {
  const pad = Math.max(0, MIN_TIME_SPAN - (last - first)) / 2
  const lo = first - pad
  const hi = last + pad
  const step = TIME_STEPS.find((s) => (hi - lo) / (UNIT_MS[s.unit] * s.n) < maxTicks) ?? { unit: 'day', n: 365 }
  const ticks: TimeTick[] = []
  for (const d = firstTick(lo, step); d.getTime() <= hi; advance(d, step.unit, step.n)) {
    ticks.push({ time: d.getTime(), label: timeLabel(d, step.unit) })
  }
  return { lo, hi, ticks }
}

// 割合 (%)。整数でなければ小数 1 桁
export const percent = (part: number, whole: number): string => `${Math.round((part / whole) * 1000) / 10}%`
