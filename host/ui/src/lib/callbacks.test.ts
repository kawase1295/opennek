import { expect, test } from 'vitest'
import { formatHeaders, parseHeaders } from './callbacks'

test('headers are edited as "Name: value" lines', () => {
  expect(formatHeaders({ Authorization: 'Bearer x', 'X-N': 1 })).toBe('Authorization: Bearer x\nX-N: 1')
  expect(parseHeaders('Authorization: Bearer x\n\n X-Id : {device} \nbroken\n: no name')).toEqual({
    Authorization: 'Bearer x',
    'X-Id': '{device}',
  })
  // a value may hold colons
  expect(parseHeaders('X-Url: http://a:1/b')).toEqual({ 'X-Url': 'http://a:1/b' })
})
