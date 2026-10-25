import { expect, test } from 'vitest'
import type { Message } from './api.gen'
import { upsertMessage } from './messages'

const msg = (id: number, deviceId = 'A'): Message => ({
  id,
  deviceId,
  time: id,
  seq: id,
  data: '00',
  ack: false,
  oob: false,
  rssi: -100,
  nbFrames: 3,
  auth: 'ok',
  station: '0000',
  decoded: null,
  downlink: null,
  downlinkStatus: 'none',
})

test('messages stay newest first', () => {
  const list = [msg(5), msg(3)]
  expect(upsertMessage(list, msg(4), '').map((m) => m.id)).toEqual([5, 4, 3])
  expect(upsertMessage(list, msg(9), '').map((m) => m.id)).toEqual([9, 5, 3])
  expect(upsertMessage([], msg(1), '').map((m) => m.id)).toEqual([1])
})

test('a known message is replaced in place', () => {
  const updated = { ...msg(3), downlinkStatus: 'sent' as const }
  const list = upsertMessage([msg(5), msg(3)], updated, '')
  expect(list).toEqual([msg(5), updated])
})

test('messages of other devices are left out while one is shown', () => {
  expect(upsertMessage([msg(1, 'A')], msg(2, 'B'), 'A').map((m) => m.id)).toEqual([1])
  expect(upsertMessage([msg(1, 'A')], msg(2, 'A'), 'A').map((m) => m.id)).toEqual([2, 1])
})
