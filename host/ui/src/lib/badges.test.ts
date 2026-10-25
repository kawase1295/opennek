import { afterEach, expect, test } from 'vitest'
import { authBadge, dlBadge, logBadge } from './badges'
import { setLang, STRINGS } from './i18n'

afterEach(() => setLang('en'))

test('badges follow the language chosen after the module is loaded', () => {
  setLang('ja')
  expect(authBadge('bad')).toEqual({ kind: 'bad', label: STRINGS.ja['auth.bad'] })
  expect(authBadge('public')).toEqual({ kind: 'muted', label: STRINGS.ja['auth.public'] })
  expect(authBadge('unverified')).toEqual({ kind: 'muted', label: STRINGS.ja['auth.unverified'] })
  for (const s of ['pending', 'sent', 'acked', 'failed', 'nodata', 'unsupported'] as const) {
    expect(dlBadge(s)?.label, s).toBe(STRINGS.ja[`dl.${s}`])
  }
})

test('badges carry their severity', () => {
  expect(authBadge('ok')).toEqual({ kind: 'ok', label: 'OK' })
  expect(dlBadge('pending')?.kind).toBe('warn')
  expect(dlBadge('sent')?.kind).toBe('ok')
  expect(dlBadge('acked')?.kind).toBe('ok')
  expect(dlBadge('failed')?.kind).toBe('bad')
  expect(dlBadge('nodata')?.kind).toBe('muted')
})

test('no badge when no downlink was asked for', () => {
  expect(dlBadge('none')).toBeNull()
})

test("a callback's result shows its HTTP status, else its error", () => {
  expect(logBadge({ ok: true, status: 200, error: null })).toEqual({ kind: 'ok', label: '200' })
  expect(logBadge({ ok: false, status: 500, error: null })).toEqual({ kind: 'bad', label: '500' })
  expect(logBadge({ ok: false, status: null, error: 'connection refused' })).toEqual({
    kind: 'bad',
    label: 'connection refused',
  })
})
