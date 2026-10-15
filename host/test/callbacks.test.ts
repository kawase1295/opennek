import { expect, test } from 'bun:test'
import { extractDownlink, renderTemplate } from '../src/callbacks'

test('renderTemplate replaces known variables and leaves others', () => {
  const vars = { device: '0A1B2C3D', data: 'cafe', 'customData#temp': '21.5' }
  expect(renderTemplate('{"d":"{device}","x":"{data}","t":{customData#temp},"u":"{unknown}"}', vars)).toBe(
    '{"d":"0A1B2C3D","x":"cafe","t":21.5,"u":"{unknown}"}',
  )
  expect(renderTemplate('https://x/?q={data}', { data: 'a b' }, encodeURIComponent)).toBe('https://x/?q=a%20b')
})

test('extractDownlink accepts Sigfox style and flat responses', () => {
  expect(extractDownlink('{"0A1B2C3D":{"downlinkData":"0102030405060708"}}', '0A1B2C3D')).toBe('0102030405060708')
  expect(extractDownlink('{"downlinkData":"AABBCCDDEEFF0011"}', '0A1B2C3D')).toBe('aabbccddeeff0011')
  expect(extractDownlink('{"downlinkData":"0102"}', '0A1B2C3D')).toBeNull()
  expect(extractDownlink('not json', '0A1B2C3D')).toBeNull()
})
