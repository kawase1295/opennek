import { afterEach, expect, test } from 'vitest'
import type { CallbackInput } from './api.gen'
import {
  blankCallback,
  copyOf,
  headersOf,
  information,
  isSent,
  rowsOf,
  SUBTYPES,
  withSubtype,
  withType,
} from './callbacks'
import { setLang } from './i18n'

afterEach(() => setLang('en'))

const cb = (c: Partial<CallbackInput>): CallbackInput => ({ ...blankCallback(), ...c })

test('a new callback is a DATA UPLINK to a URL, as in the Backend', () => {
  expect(blankCallback()).toMatchObject({
    callbackType: 'DATA',
    callbackSubtype: 'UPLINK',
    channel: 'URL',
    httpMethod: 'POST',
    sendSni: true,
    sendDuplicate: false,
    contentType: 'application/json',
    downlink: false,
    enabled: true,
  })
})

test('each type offers its own subtypes', () => {
  expect(SUBTYPES.DATA).toEqual(['UPLINK', 'BIDIR'])
  expect(SUBTYPES.SERVICE).toEqual(['STATUS', 'ACKNOWLEDGE', 'REPEATER', 'DATA_ADVANCED'])
  expect(SUBTYPES.ERROR).toEqual([])
})

test('changing the type picks its first subtype; only a BIDIR stays chosen for downlink', () => {
  const bidir = cb({ callbackSubtype: 'BIDIR', downlink: true })
  expect(withType(bidir, 'SERVICE')).toMatchObject({ callbackType: 'SERVICE', callbackSubtype: 'STATUS', downlink: false })
  expect(withType(bidir, 'ERROR')).toMatchObject({ callbackSubtype: null, downlink: false })
  expect(withType(cb({}), 'DATA').callbackSubtype).toBe('UPLINK')
  expect(withSubtype(bidir, 'UPLINK')).toMatchObject({ callbackSubtype: 'UPLINK', downlink: false })
  expect(withSubtype(cb({}), 'BIDIR')).toMatchObject({ callbackSubtype: 'BIDIR', downlink: false })
})

test('OpenNEK sends DATA callbacks to a URL only', () => {
  expect(isSent(cb({}))).toBe(true)
  expect(isSent(cb({ callbackSubtype: 'BIDIR' }))).toBe(true)
  expect(isSent(cb({ callbackType: 'SERVICE', callbackSubtype: 'STATUS' }))).toBe(false)
  expect(isSent(cb({ callbackType: 'ERROR', callbackSubtype: null }))).toBe(false)
  expect(isSent(cb({ channel: 'BATCH_URL' }))).toBe(false)
  expect(isSent(cb({ channel: 'EMAIL' }))).toBe(false)
})

test('the list sums a callback up by its channel', () => {
  expect(information(cb({ url: 'https://x/up', httpMethod: 'PUT' }))).toBe('PUT https://x/up')
  expect(information(cb({ channel: 'BATCH_URL', url: 'https://x/batch' }))).toBe('https://x/batch')
  expect(information(cb({ channel: 'EMAIL', recipient: 'ops@example.com', subject: '[{device}]' }))).toBe(
    'ops@example.com · [{device}]',
  )
})

test('headers are edited as rows; empty names are left out', () => {
  expect(rowsOf({ Authorization: 'Bearer x', 'X-N': 1 })).toEqual([
    { name: 'Authorization', value: 'Bearer x' },
    { name: 'X-N', value: '1' },
  ])
  expect(
    headersOf([
      { name: ' X-Id ', value: '{device}' },
      { name: '', value: 'lost' },
      { name: 'A', value: '' },
    ]),
  ).toEqual({ 'X-Id': '{device}', A: '' })
})

test('a copy is not chosen for downlink and says it is a copy', () => {
  const c = copyOf(cb({ name: 'Webhook', callbackSubtype: 'BIDIR', downlink: true }))
  expect(c).toMatchObject({ name: 'Webhook (copy)', callbackSubtype: 'BIDIR', downlink: false })
  setLang('ja')
  expect(copyOf(cb({ name: '' })).name).toBe('(コピー)')
})
