import { expect, test } from 'bun:test'
import { cfoSigned, cfoStepHz } from '../tools/cfo-capture'

test('CFO step follows SUBGRF_GetCFO for the streamer bandwidths', () => {
  // 234.3 kHz: demod sampling 500 kHz
  expect(cfoStepHz(0x0a)).toBeCloseTo(500_000 / 4096, 3)
  // 4.8 kHz: 10.4 kHz sampling, oversampling 17 at 600 bps; interpolated by 4 when the bit rate is above it
  expect(cfoStepHz(0x1f, 600)).toBeCloseTo(32e6 / (12 * 256) / 4096, 3)
  expect(cfoStepHz(0x1f, 50_000)).toBeCloseTo(((32e6 / (12 * 256)) * 4) / 4096, 3)
})

test('CFO register values are 12-bit signed', () => {
  expect(cfoSigned(0x7ff)).toBe(2047)
  expect(cfoSigned(0x800)).toBe(-2048)
  expect(cfoSigned(0xfff)).toBe(-1)
})
