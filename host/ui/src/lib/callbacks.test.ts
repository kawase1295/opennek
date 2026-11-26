import { afterEach, expect, test } from 'vitest'
import type { CallbackInput } from './api.gen'
import {
  ACKNOWLEDGE_BODY,
  blankCallback,
  copyOf,
  DEFAULT_BODY,
  headersOf,
  information,
  isSent,
  rowsOf,
  SUBTYPES,
  variablesOf,
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

test('OpenNEK sends DATA and SERVICE ACKNOWLEDGE callbacks to a URL only', () => {
  expect(isSent(cb({}))).toBe(true)
  expect(isSent(cb({ callbackSubtype: 'BIDIR' }))).toBe(true)
  expect(isSent(cb({ callbackType: 'SERVICE', callbackSubtype: 'ACKNOWLEDGE' }))).toBe(true)
  expect(isSent(cb({ callbackType: 'SERVICE', callbackSubtype: 'STATUS' }))).toBe(false)
  expect(isSent(cb({ callbackType: 'SERVICE', callbackSubtype: 'REPEATER' }))).toBe(false)
  expect(isSent(cb({ callbackType: 'SERVICE', callbackSubtype: 'DATA_ADVANCED' }))).toBe(false)
  expect(isSent(cb({ callbackType: 'SERVICE', callbackSubtype: 'ACKNOWLEDGE', channel: 'EMAIL' }))).toBe(false)
  expect(isSent(cb({ callbackType: 'ERROR', callbackSubtype: null }))).toBe(false)
  expect(isSent(cb({ channel: 'BATCH_URL' }))).toBe(false)
  expect(isSent(cb({ channel: 'EMAIL' }))).toBe(false)
})

test("an ACKNOWLEDGE callback offers the Backend's ACKNOWLEDGE variables", () => {
  const ack = cb({ callbackType: 'SERVICE', callbackSubtype: 'ACKNOWLEDGE' })
  expect(variablesOf(ack)).toEqual(['{device}', '{time}', '{infoCode}', '{infoMessage}', '{downlinkAck}', '{downlinkOverusage}'])
  expect(variablesOf(cb({}))).toContain('{data}')
  expect(variablesOf(cb({}))).not.toContain('{infoCode}')
})

test('an untouched body follows the switch to and from ACKNOWLEDGE; an edited one stays', () => {
  const ack = withSubtype(withType(cb({}), 'SERVICE'), 'ACKNOWLEDGE')
  expect(ack.bodyTemplate).toBe(ACKNOWLEDGE_BODY)
  expect(withType(ack, 'DATA').bodyTemplate).toBe(DEFAULT_BODY)
  expect(withSubtype(ack, 'STATUS').bodyTemplate).toBe(DEFAULT_BODY)
  const edited = { ...ack, bodyTemplate: '{"code":{infoCode}}' }
  expect(withType(edited, 'DATA').bodyTemplate).toBe('{"code":{infoCode}}')
  expect(withSubtype(cb({ bodyTemplate: 'x={data}' }), 'BIDIR').bodyTemplate).toBe('x={data}')
  // every ACKNOWLEDGE variable is in its default body, which is JSON once they are filled in
  for (const v of variablesOf(ack)) expect(ACKNOWLEDGE_BODY).toContain(v)
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
