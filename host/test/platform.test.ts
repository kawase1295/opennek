import { expect, test } from 'vitest'
import { browserCommand, defaultDataDir } from '../src/platform'

test('the data directory follows each OS', () => {
  expect(defaultDataDir('linux', {}, '/home/u')).toBe('/home/u/.local/share/opennek')
  expect(defaultDataDir('linux', { XDG_DATA_HOME: '/data' }, '/home/u')).toBe('/data/opennek')
  expect(defaultDataDir('win32', { APPDATA: 'C:\\Users\\u\\AppData\\Roaming' }, 'C:\\Users\\u')).toBe('C:\\Users\\u\\AppData\\Roaming\\opennek')
  expect(defaultDataDir('win32', {}, 'C:\\Users\\u')).toBe('C:\\Users\\u\\AppData\\Roaming\\opennek')
  expect(defaultDataDir('darwin', {}, '/Users/u')).toBe('/Users/u/Library/Application Support/opennek')
})

test('the browser is opened with the OS command', () => {
  const url = 'http://127.0.0.1:8085/'
  expect(browserCommand('linux', url)).toEqual(['xdg-open', [url]])
  expect(browserCommand('darwin', url)).toEqual(['open', [url]])
  // start is built into cmd; the empty argument (quoted by Node) is the window title
  expect(browserCommand('win32', url)).toEqual(['cmd', ['/c', 'start', '', url]])
})
