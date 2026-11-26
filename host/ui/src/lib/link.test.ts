import { describe, expect, test } from 'vitest'
import { percent, rssiAxis, timeAxis } from './link'

// local time, as the axis shows it
const at = (month: number, day: number, h: number, m: number, s = 0) => new Date(2026, month - 1, day, h, m, s).getTime()

describe('the RSSI axis', () => {
  test('has round dBm ticks around the values', () => {
    expect(rssiAxis(-112, -95)).toEqual({ lo: -115, hi: -95, ticks: [-115, -110, -105, -100, -95] })
    expect(rssiAxis(-120, -70)).toEqual({ lo: -120, hi: -60, ticks: [-120, -100, -80, -60] })
  })

  test('spans 10 dB around a single value', () => {
    expect(rssiAxis(-90, -90)).toEqual({ lo: -95, hi: -85, ticks: [-95, -90, -85] })
  })
})

describe('the time axis', () => {
  const ticks = (first: number, last: number, maxTicks: number) => timeAxis(first, last, maxTicks).ticks

  test('has round local times, no more than asked for', () => {
    expect(ticks(at(11, 26, 9, 7), at(11, 26, 11, 2), 5)).toEqual([
      { time: at(11, 26, 9, 30), label: '09:30' },
      { time: at(11, 26, 10, 0), label: '10:00' },
      { time: at(11, 26, 10, 30), label: '10:30' },
      { time: at(11, 26, 11, 0), label: '11:00' },
    ])
  })

  test('shows the date at midnight', () => {
    expect(ticks(at(11, 26, 22, 10), at(11, 27, 2, 20), 6).map((t) => t.label)).toEqual([
      '23:00',
      '11/27',
      '01:00',
      '02:00',
    ])
    expect(ticks(at(11, 20, 15, 0), at(11, 26, 9, 0), 4).map((t) => t.label)).toEqual(['11/21', '11/23', '11/25'])
  })

  test('spans a minute around a single time, with seconds', () => {
    const axis = timeAxis(at(11, 26, 10, 0, 15), at(11, 26, 10, 0, 15), 5)
    expect([axis.lo, axis.hi]).toEqual([at(11, 26, 9, 59, 45), at(11, 26, 10, 0, 45)])
    expect(axis.ticks.map((t) => t.label)).toEqual(['10:00:00', '10:00:30'])
  })
})

test('shares are whole percents or one decimal', () => {
  expect(percent(3, 3)).toBe('100%')
  expect(percent(0, 5)).toBe('0%')
  expect(percent(2, 3)).toBe('66.7%')
  expect(percent(1, 8)).toBe('12.5%')
})
