import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, test } from 'vitest'
import { chooseLang, type Key, LANGUAGES, lang, setLang, STRINGS, t } from './i18n'

// every .svelte and .ts file of the UI but the tests
function sources(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const path = join(dir, e.name)
    if (e.isDirectory()) return sources(path)
    return /\.(svelte|ts)$/.test(e.name) && !e.name.endsWith('.test.ts') ? [path] : []
  })
}
const code = sources(join(import.meta.dirname, '..'))
  .map((f) => readFileSync(f, 'utf8'))
  .join('\n')
const usedKeys = new Set([...code.matchAll(/\bt\('([^']+)'/g)].map((m) => m[1]))
// keys built at run time: t(`radio.${state}`), t(`dl.${status}`), ...
const usedPrefixes = [...code.matchAll(/\bt\(`([a-zA-Z]+\.)\$\{/g)].map((m) => m[1] ?? '')

test('defaults to English until the settings are read', () => {
  expect(lang).toBe('en')
  expect(Object.keys(LANGUAGES)).toEqual(Object.keys(STRINGS))
})

test('the language comes from the settings', () => {
  expect(chooseLang('ja', null, ['en-US'])).toEqual({ lang: 'ja', migrate: false })
  expect(chooseLang('en', null, ['ja-JP'])).toEqual({ lang: 'en', migrate: false })
})

test("until one is chosen, the browser's language is used", () => {
  expect(chooseLang(null, null, ['ja-JP', 'en-US'])).toEqual({ lang: 'ja', migrate: false })
  expect(chooseLang(null, null, ['ja'])).toEqual({ lang: 'ja', migrate: false })
  // the first one the UI has
  expect(chooseLang(null, null, ['fr-FR', 'ja', 'en'])).toEqual({ lang: 'ja', migrate: false })
  expect(chooseLang(null, null, ['en-GB', 'ja'])).toEqual({ lang: 'en', migrate: false })
  // none the UI has, or no browser languages
  expect(chooseLang(null, null, ['fr-FR'])).toEqual({ lang: 'en', migrate: false })
  expect(chooseLang(null, null, [])).toEqual({ lang: 'en', migrate: false })
  // unknown or missing (the host is down) is the same as not chosen
  expect(chooseLang('fr', null, ['ja-JP'])).toEqual({ lang: 'ja', migrate: false })
  expect(chooseLang(undefined, null, ['en-US'])).toEqual({ lang: 'en', migrate: false })
})

test('a choice stored by an older version moves to the settings once', () => {
  expect(chooseLang(null, 'ja', ['en-US'])).toEqual({ lang: 'ja', migrate: true })
  // versions before the language could be left unchosen stored the default "en"
  expect(chooseLang('en', 'ja', ['en-US'])).toEqual({ lang: 'ja', migrate: true })
  expect(chooseLang('en', 'en', ['ja-JP'])).toEqual({ lang: 'en', migrate: false })
  expect(chooseLang('en', 'fr', ['ja-JP'])).toEqual({ lang: 'en', migrate: false })
  expect(chooseLang(null, 'fr', ['ja-JP'])).toEqual({ lang: 'ja', migrate: false })
  // a language chosen in the settings wins
  expect(chooseLang('ja', 'en', ['en-US'])).toEqual({ lang: 'ja', migrate: false })
})

test('setLang() switches t()', () => {
  setLang('ja')
  expect(lang).toBe('ja')
  expect(t('common.saved')).toBe(STRINGS.ja['common.saved'])
  setLang('xx')
  expect(lang).toBe('en')
  expect(t('common.saved')).toBe('Saved')
})

describe('the strings', () => {
  test('every string is used by the UI', () => {
    for (const k of Object.keys(STRINGS.en)) {
      expect(usedKeys.has(k) || usedPrefixes.some((p) => k.startsWith(p)), k).toBe(true)
    }
  })

  test('placeholders match between languages', () => {
    const vars = (s: string) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort()
    for (const [k, s] of Object.entries(STRINGS.en)) expect(vars(STRINGS.ja[k as Key]), k).toEqual(vars(s))
  })

  test('t() fills only the given variables', () => {
    setLang('en')
    expect(t('radio.framesValue', { frames: 7, bad: 1 })).toBe('7 (1 CRC errors)')
    expect(t('cb.target', {})).toBe('Devices: {devices}')
  })
})
