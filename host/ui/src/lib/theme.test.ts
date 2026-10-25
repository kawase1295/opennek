import { expect, test } from 'vitest'
import { themeAttribute } from './theme'

test('light and dark are forced with data-theme; system leaves it to the OS', () => {
  expect(themeAttribute('light')).toBe('light')
  expect(themeAttribute('dark')).toBe('dark')
  expect(themeAttribute('system')).toBeNull()
})
