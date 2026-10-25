import { afterEach, expect, test } from 'vitest'
import type { RadioStatus } from './api.gen'
import { setLang } from './i18n'
import { radioPill } from './radio'

afterEach(() => setLang('en'))

const status = (s: Partial<RadioStatus>): RadioStatus => ({
  state: 'off',
  port: null,
  frames: 0,
  badFrames: 0,
  lostLines: 0,
  ...s,
})

test('the pill shows the frequency while receiving', () => {
  expect(radioPill(status({ state: 'connected', port: 'COM4', freq: 923_200_000 }))).toEqual({
    kind: 'connected',
    text: 'Sigfox · 923.2 MHz',
    title: 'COM4',
  })
})

test('the pill shows the state otherwise, with the error as its title', () => {
  expect(radioPill(status({ state: 'connecting', port: 'COM4' }))).toEqual({
    kind: 'connecting',
    text: 'Sigfox · Waiting for reply',
    title: 'COM4',
  })
  expect(radioPill(status({ state: 'disconnected', port: 'COM4', error: 'gone' }))).toEqual({
    kind: 'disconnected',
    text: 'Sigfox · Disconnected',
    title: 'gone',
  })
  // off looks like disconnected, as before
  expect(radioPill(status({ state: 'off' }))).toEqual({ kind: 'disconnected', text: 'Sigfox · Off', title: '' })
  expect(radioPill(null)).toEqual({ kind: 'disconnected', text: 'Sigfox · Off', title: '' })
})
