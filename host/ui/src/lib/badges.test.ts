import { afterEach, expect, test } from 'vitest'
import { authBadge, dlBadge, logBadge } from './badges'
import { setLang, STRINGS } from './i18n'

afterEach(() => setLang('en'))

test('badges follow the language chosen after the module is loaded', () => {
  setLang('ja')
  expect(authBadge('bad').label).toBe(STRINGS.ja['auth.bad'])
  expect(authBadge('public').label).toBe(STRINGS.ja['auth.public'])
  expect(authBadge('unverified').label).toBe(STRINGS.ja['auth.unverified'])
  for (const s of ['pending', 'sent', 'acked', 'failed', 'nodata', 'unsupported'] as const) {
    expect(dlBadge(s)?.label, s).toBe(STRINGS.ja[`dl.${s}`])
  }
})

test('a verified uplink is quiet, a key mismatch stands out', () => {
  expect(authBadge('ok')).toEqual({ tone: 'ok', look: 'quiet', label: 'OK' })
  expect(authBadge('bad')).toEqual({ tone: 'bad', look: 'solid', label: 'Key mismatch' })
  expect(authBadge('public')).toEqual({ tone: 'neutral', look: 'soft', label: 'Public key' })
  expect(authBadge('unverified')).toEqual({ tone: 'neutral', look: 'soft', label: 'Unverified' })
})

test('downlinks show their state with a dot', () => {
  expect(dlBadge('pending')).toEqual({ tone: 'warn', look: 'dot', label: 'Pending' })
  expect(dlBadge('sent')?.tone).toBe('ok')
  expect(dlBadge('acked')?.tone).toBe('ok')
  expect(dlBadge('failed')?.tone).toBe('bad')
  expect(dlBadge('nodata')?.tone).toBe('neutral')
  expect(dlBadge('unsupported')?.tone).toBe('neutral')
})

test('no badge when no downlink was asked for', () => {
  expect(dlBadge('none')).toBeNull()
})

test("a callback's result shows its HTTP status, else its error", () => {
  expect(logBadge({ ok: true, status: 200, error: null })).toEqual({ tone: 'neutral', look: 'mono', label: '200' })
  expect(logBadge({ ok: false, status: 500, error: null })).toEqual({ tone: 'bad', look: 'mono', label: '500' })
  expect(logBadge({ ok: false, status: null, error: 'connection refused' })).toEqual({
    tone: 'bad',
    look: 'soft',
    label: 'connection refused',
  })
})
