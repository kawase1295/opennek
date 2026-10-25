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

test('while receiving: the frequency', () => {
  expect(radioPill(status({ state: 'connected', port: 'COM4', freq: 923_200_000 }))).toEqual({
    tone: 'connected',
    label: 'Receiving',
    detail: '923.2 MHz',
    title: 'COM4',
  })
  // the frequency comes with the firmware's first line
  expect(radioPill(status({ state: 'connected', port: 'COM4' })).detail).toBe('COM4')
})

test('waiting and disconnected name the port; the error is the title', () => {
  expect(radioPill(status({ state: 'connecting', port: 'COM4' }))).toEqual({
    tone: 'connecting',
    label: 'Waiting for reply',
    detail: 'COM4',
    title: 'COM4',
  })
  expect(radioPill(status({ state: 'disconnected', port: 'COM4', error: 'gone' }))).toEqual({
    tone: 'error',
    label: 'Disconnected',
    detail: 'COM4',
    title: 'gone',
  })
})

test('no receiver is grey, not an error', () => {
  expect(radioPill(status({ state: 'off' }))).toEqual({ tone: 'off', label: 'No receiver', detail: '', title: '' })
  expect(radioPill(null).tone).toBe('off')
})
