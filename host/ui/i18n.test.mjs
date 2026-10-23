// Checks the UI strings against index.html and app.js. Run with `node --test host/ui/i18n.test.mjs` (no dependencies)
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import { LANGUAGES, lang, STRINGS, t } from './i18n.js'

const html = readFileSync(new URL('./index.html', import.meta.url), 'utf8')
const app = readFileSync(new URL('./app.js', import.meta.url), 'utf8')

const htmlKeys = new Set([...html.matchAll(/data-i18n(?:-html|-placeholder|-title)?="([^"]+)"/g)].map((m) => m[1]))
const appKeys = new Set([...app.matchAll(/\bt\('([^']+)'/g)].map((m) => m[1]))
// keys built at run time: t(`radio.${state}`), t(`dl.${status}`), ...
const appPrefixes = [...app.matchAll(/\bt\(`([a-zA-Z]+\.)\$\{/g)].map((m) => m[1])

test('defaults to English without a stored choice', () => {
  assert.equal(lang, 'en')
  assert.deepEqual(Object.keys(LANGUAGES), Object.keys(STRINGS))
})

test('every key used by app.js has an English string', () => {
  for (const k of appKeys) assert.equal(typeof STRINGS.en[k], 'string', k)
})

test('Japanese covers index.html and every English key, with nothing extra', () => {
  const expected = new Set([...htmlKeys, ...Object.keys(STRINGS.en)])
  assert.deepEqual(new Set(Object.keys(STRINGS.ja)), expected)
})

test('every English string is used by index.html or app.js', () => {
  for (const k of Object.keys(STRINGS.en)) {
    assert.ok(htmlKeys.has(k) || appKeys.has(k) || appPrefixes.some((p) => k.startsWith(p)), k)
  }
})

test('placeholders match between languages', () => {
  const vars = (s) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort()
  for (const [k, s] of Object.entries(STRINGS.en)) assert.deepEqual(vars(STRINGS.ja[k]), vars(s), k)
})

test('t() fills only the given variables', () => {
  assert.equal(t('radio.framesValue', { frames: 7, bad: 1 }), '7 (1 CRC errors)')
  assert.equal(t('cb.target', {}), 'Devices: {devices}')
  assert.equal(t('no.such.key'), 'no.such.key')
})
