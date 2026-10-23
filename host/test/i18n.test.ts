import { expect, test } from 'bun:test'
import { readFileSync } from 'node:fs'

// server.ts が同じファイルを text として import しているので、クエリを付けて別のモジュールとして読む
const { STRINGS, LANGUAGES, lang, t } = await import(`../src/ui/i18n.js?${'module'}`)
const html = readFileSync(new URL('../src/ui/index.html', import.meta.url), 'utf8')
const app = readFileSync(new URL('../src/ui/app.js', import.meta.url), 'utf8')

const htmlKeys = new Set([...html.matchAll(/data-i18n(?:-html|-placeholder|-title)?="([^"]+)"/g)].map((m) => m[1]!))
const appKeys = new Set([...app.matchAll(/\bt\('([^']+)'/g)].map((m) => m[1]!))
// keys built at run time: t(`radio.${state}`), t(`dl.${status}`), ...
const appPrefixes = [...app.matchAll(/\bt\(`([a-zA-Z]+\.)\$\{/g)].map((m) => m[1]!)

test('defaults to English without a stored choice', () => {
  expect(lang).toBe('en')
  expect(Object.keys(LANGUAGES)).toEqual(Object.keys(STRINGS))
})

test('every key used by app.js has an English string', () => {
  for (const k of appKeys) expect(STRINGS.en[k], k).toBeString()
})

test('Japanese covers index.html and every English key, with nothing extra', () => {
  const expected = new Set([...htmlKeys, ...Object.keys(STRINGS.en)])
  expect(new Set(Object.keys(STRINGS.ja))).toEqual(expected)
})

test('every English string is used by index.html or app.js', () => {
  for (const k of Object.keys(STRINGS.en)) {
    expect(htmlKeys.has(k) || appKeys.has(k) || appPrefixes.some((p) => k.startsWith(p)), k).toBe(true)
  }
})

test('placeholders match between languages', () => {
  const vars = (s: string) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort()
  for (const [k, s] of Object.entries<string>(STRINGS.en)) expect(vars(STRINGS.ja[k]), k).toEqual(vars(s))
})

test('t() fills only the given variables', () => {
  expect(t('radio.framesValue', { frames: 7, bad: 1 })).toBe('7 (1 CRC errors)')
  expect(t('cb.target', {})).toBe('Devices: {devices}')
  expect(t('no.such.key')).toBe('no.such.key')
})
