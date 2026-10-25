// Checks the status badges. Run with `node --test host/ui/badges.test.mjs` (no dependencies)
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { authBadge, dlBadge } from './badges.js'
import { setLang, STRINGS } from './i18n.js'

test('badges follow the language chosen after the module is loaded', () => {
  setLang('ja')
  assert.ok(authBadge('bad').includes(STRINGS.ja['auth.bad']))
  assert.ok(authBadge('public').includes(STRINGS.ja['auth.public']))
  assert.ok(authBadge('unverified').includes(STRINGS.ja['auth.unverified']))
  for (const s of ['pending', 'sent', 'acked', 'failed', 'nodata', 'unsupported']) {
    assert.ok(dlBadge(s).includes(STRINGS.ja[`dl.${s}`]), s)
  }
  setLang('en')
  assert.ok(authBadge('bad').includes(STRINGS.en['auth.bad']))
})

test('badges carry their severity', () => {
  assert.equal(authBadge('ok'), '<span class="badge ok">OK</span>')
  assert.match(authBadge('bad'), /class="badge bad"/)
  assert.match(dlBadge('pending'), /class="badge warn"/)
  assert.match(dlBadge('failed'), /class="badge bad"/)
})

test('no badge for no downlink or an unknown value', () => {
  assert.equal(dlBadge('none'), '')
  assert.equal(dlBadge('other'), '')
  assert.equal(authBadge('other'), '')
})
