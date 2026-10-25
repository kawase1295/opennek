import { expect, test } from 'vitest'
import { decodedText, mhz, spaced } from './format'

test('hex is shown byte by byte', () => {
  expect(spaced('c8002a')).toBe('c8 00 2a')
  expect(spaced('')).toBe('—')
  expect(spaced(null)).toBe('—')
})

test('decoded values are shown as name=value', () => {
  expect(decodedText({ temp: 223, hum: 63 })).toBe('temp=223  hum=63')
  expect(decodedText({ door: true, label: 'x' })).toBe('door=true  label=x')
  expect(decodedText(null)).toBe('')
})

test('frequencies are shown in MHz', () => {
  expect(mhz(923_200_000, 1)).toBe('923.2 MHz')
  expect(mhz(923_200_000, 3)).toBe('923.200 MHz')
})
