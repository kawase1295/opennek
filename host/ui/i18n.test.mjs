// Checks the UI strings against index.html and the scripts. Run with `node --test host/ui/i18n.test.mjs` (no dependencies)
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import { chooseLang, LANGUAGES, lang, setLang, STRINGS, t } from './i18n.js'

const html = readFileSync(new URL('./index.html', import.meta.url), 'utf8')
// the scripts that call t()
const app = ['./app.js', './badges.js'].map((f) => readFileSync(new URL(f, import.meta.url), 'utf8')).join('\n')

const htmlKeys = new Set([...html.matchAll(/data-i18n(?:-html|-placeholder|-title)?="([^"]+)"/g)].map((m) => m[1]))
const appKeys = new Set([...app.matchAll(/\bt\('([^']+)'/g)].map((m) => m[1]))
// keys built at run time: t(`radio.${state}`), t(`dl.${status}`), ...
const appPrefixes = [...app.matchAll(/\bt\(`([a-zA-Z]+\.)\$\{/g)].map((m) => m[1])

test('defaults to English until the settings are read', () => {
  assert.equal(lang, 'en')
  assert.deepEqual(Object.keys(LANGUAGES), Object.keys(STRINGS))
})

test('the language comes from the settings', () => {
  assert.deepEqual(chooseLang('ja', null, ['en-US']), { lang: 'ja', migrate: false })
  assert.deepEqual(chooseLang('en', null, ['ja-JP']), { lang: 'en', migrate: false })
})

test('until one is chosen, the browser\'s language is used', () => {
  assert.deepEqual(chooseLang(null, null, ['ja-JP', 'en-US']), { lang: 'ja', migrate: false })
  assert.deepEqual(chooseLang(null, null, ['ja']), { lang: 'ja', migrate: false })
  // the first one the UI has
  assert.deepEqual(chooseLang(null, null, ['fr-FR', 'ja', 'en']), { lang: 'ja', migrate: false })
  assert.deepEqual(chooseLang(null, null, ['en-GB', 'ja']), { lang: 'en', migrate: false })
  // none the UI has, or no browser languages
  assert.deepEqual(chooseLang(null, null, ['fr-FR']), { lang: 'en', migrate: false })
  assert.deepEqual(chooseLang(null, null, []), { lang: 'en', migrate: false })
  // unknown or missing (the host is down) is the same as not chosen
  assert.deepEqual(chooseLang('fr', null, ['ja-JP']), { lang: 'ja', migrate: false })
  assert.deepEqual(chooseLang(undefined, null, ['en-US']), { lang: 'en', migrate: false })
})

test('a choice stored by an older version moves to the settings once', () => {
  assert.deepEqual(chooseLang(null, 'ja', ['en-US']), { lang: 'ja', migrate: true })
  // versions before the language could be left unchosen stored the default "en"
  assert.deepEqual(chooseLang('en', 'ja', ['en-US']), { lang: 'ja', migrate: true })
  assert.deepEqual(chooseLang('en', 'en', ['ja-JP']), { lang: 'en', migrate: false })
  assert.deepEqual(chooseLang('en', 'fr', ['ja-JP']), { lang: 'en', migrate: false })
  assert.deepEqual(chooseLang(null, 'fr', ['ja-JP']), { lang: 'ja', migrate: false })
  // a language chosen in the settings wins
  assert.deepEqual(chooseLang('ja', 'en', ['en-US']), { lang: 'ja', migrate: false })
})

test('setLang() switches t()', () => {
  setLang('ja')
  assert.equal(lang, 'ja')
  assert.equal(t('common.saved'), STRINGS.ja['common.saved'])
  setLang('xx')
  assert.equal(lang, 'en')
  assert.equal(t('common.saved'), 'Saved')
})

test('every key used by the scripts has an English string', () => {
  for (const k of appKeys) assert.equal(typeof STRINGS.en[k], 'string', k)
})

test('Japanese covers index.html and every English key, with nothing extra', () => {
  const expected = new Set([...htmlKeys, ...Object.keys(STRINGS.en)])
  assert.deepEqual(new Set(Object.keys(STRINGS.ja)), expected)
})

test('every English string is used by index.html or the scripts', () => {
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
