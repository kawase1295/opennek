import { expect, test } from 'vitest'
import { setLang } from './i18n'
import { NO_RECEIVER, portOptions, sigfoxPortSetting } from './settings'

setLang('en')

const kit = { path: 'COM4', label: 'XIAO ESP32S3 kit (COM4)', kit: true }
const other = { path: '/dev/ttyUSB0', label: 'CP2102', kit: false }

test('the port list: find the kit, no receiver, then the ports', () => {
  const { options, value } = portOptions([kit, other], 'COM4')
  expect(options.map((o) => o.value)).toEqual(['', NO_RECEIVER, 'COM4', '/dev/ttyUSB0'])
  // a label that names the path is not given it twice
  expect(options[2]?.text).toBe('XIAO ESP32S3 kit (COM4)')
  expect(options[3]?.text).toBe('CP2102 (/dev/ttyUSB0)')
  expect(value).toBe('COM4')
})

test('a saved port that is not plugged in stays selectable', () => {
  const { options, value } = portOptions([], '/dev/ttyACM9')
  expect(options.map((o) => o.value)).toEqual(['', NO_RECEIVER, '/dev/ttyACM9'])
  expect(value).toBe('/dev/ttyACM9')
})

test('null (no receiver) and "" (not set) are kept apart', () => {
  expect(portOptions([], null).value).toBe(NO_RECEIVER)
  expect(portOptions([], '').value).toBe('')
  expect(sigfoxPortSetting(NO_RECEIVER)).toBeNull()
  expect(sigfoxPortSetting('')).toBe('')
  expect(sigfoxPortSetting('COM4')).toBe('COM4')
})
