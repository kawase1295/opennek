import { readFileSync } from 'node:fs'
import { expect, test } from 'vitest'
import { VERSION } from '../src/version'

const pkg = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8'))

test('the version printed by the host is the published one', () => {
  expect(VERSION).toBe(pkg.version)
})

test('the package ships the CLI, the build and the UI only', () => {
  expect(pkg.private).toBeUndefined()
  expect(pkg.bin).toEqual({ opennek: 'bin/opennek.js' })
  expect(pkg.files).toEqual(['bin', 'dist'])
  expect(pkg.engines.node).toBe('>=22.13')
})
