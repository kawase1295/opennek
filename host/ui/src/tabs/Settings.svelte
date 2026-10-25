<script lang="ts">
  import { onMount } from 'svelte'
  import { api } from '../lib/api'
  import type { Theme } from '../lib/api.gen'
  import { app } from '../lib/app.svelte'
  import { fmtTime, mhz } from '../lib/format'
  import { t } from '../lib/i18n'
  import { portOptions, sigfoxPortSetting, type PortOption } from '../lib/settings'
  import { applyTheme } from '../lib/theme'
  import { errorText, showToast } from '../lib/toast.svelte'
  import PageHeader from '../components/PageHeader.svelte'

  const THEMES = ['system', 'light', 'dark'] as const satisfies readonly Theme[]

  let loaded = $state(false)
  let ports = $state<PortOption[]>([])
  let port = $state('')
  let downlinkTimeoutMs = $state(0)
  let acceptUnknownDevices = $state(false)
  let sigfoxFreq = $state(0)
  let sigfoxDownlink = $state(false)
  let theme = $state<Theme>('system')

  const r = $derived(app.radio)

  onMount(async () => {
    const [s, found] = await Promise.all([api.settings(), api.serialPorts()])
    const choice = portOptions(found, s.sigfoxPort)
    ports = choice.options
    port = choice.value
    downlinkTimeoutMs = s.downlinkTimeoutMs
    acceptUnknownDevices = s.acceptUnknownDevices
    sigfoxFreq = s.sigfoxFreq
    sigfoxDownlink = s.sigfoxDownlink
    theme = s.theme
    loaded = true
  })

  // 見た目はその場で変え、すぐ保存する
  async function chooseTheme(next: Theme): Promise<void> {
    const before = theme
    theme = next
    applyTheme(next)
    try {
      await api.saveSettings({ theme: next })
    } catch (err) {
      theme = before
      applyTheme(before)
      showToast(errorText(err))
    }
  }

  async function save(e: SubmitEvent): Promise<void> {
    e.preventDefault()
    try {
      await api.saveSettings({
        downlinkTimeoutMs,
        acceptUnknownDevices,
        sigfoxPort: sigfoxPortSetting(port),
        sigfoxFreq,
        sigfoxDownlink,
      })
      showToast(t('common.saved'))
    } catch (err) {
      showToast(errorText(err))
    }
  }
</script>

<PageHeader title={t('nav.settings')} />
<div class="card">
  <h3>{t('set.radio')}</h3>
  <dl class="kv">
    {#if r && r.state !== 'off'}
      <dt>{t('kv.state')}</dt><dd>{t(`radio.${r.state}`)}{r.error ? ` — ${r.error}` : ''}</dd>
      <dt>{t('kv.port')}</dt><dd>{r.port ?? '—'}</dd>
      <dt>{t('kv.firmware')}</dt><dd>{r.fw ?? '—'}</dd>
      <dt>{t('kv.freq')}</dt><dd>{r.freq ? mhz(r.freq, 3) : '—'}</dd>
      <dt>{t('kv.rate')}</dt><dd>{r.rate ? `${r.rate} Hz` : '—'}</dd>
      <dt>{t('kv.frames')}</dt><dd>{t('radio.framesValue', { frames: r.frames, bad: r.badFrames })}</dd>
      <dt>{t('kv.lastFrame')}</dt><dd>{fmtTime(r.lastFrameAt)}</dd>
      <dt>{t('kv.lostLines')}</dt><dd>{r.lostLines}</dd>
    {:else}
      <dt>{t('kv.state')}</dt><dd>{t('radio.offHint')}</dd>
    {/if}
  </dl>
</div>
{#if loaded}
  <div class="card form" id="appearance">
    <h3>{t('set.appearance')}</h3>
    <div class="field">
      <span class="label">{t('set.theme')}</span>
      <div class="seg" role="group" aria-label={t('set.theme')}>
        {#each THEMES as v (v)}
          <button type="button" aria-pressed={theme === v} onclick={() => chooseTheme(v)}>{t(`theme.${v}`)}</button>
        {/each}
      </div>
    </div>
  </div>
  <form id="settings-form" class="card form" onsubmit={save}>
    <h3>{t('set.host')}</h3>
    <label><span>{t('set.timeout')}</span><input bind:value={downlinkTimeoutMs} type="number" min="500" max="30000" required /></label>
    <label class="check"><input type="checkbox" bind:checked={acceptUnknownDevices} /><span>{t('set.acceptUnknown')}</span></label>
    <h3>{t('set.sigfox')}</h3>
    <label><span>{t('set.sigfoxPort')}</span>
      <select bind:value={port}>
        {#each ports as o (o.value)}<option value={o.value}>{o.text}</option>{/each}
      </select>
      <small>{t('set.sigfoxPortHint')}</small></label>
    <label><span>{t('set.sigfoxFreq')}</span><input bind:value={sigfoxFreq} type="number" min="862000000" max="928000000" step="100" required /></label>
    <label class="check"><input type="checkbox" bind:checked={sigfoxDownlink} /><span class="check-text"><span>{t('set.sigfoxDownlink')}</span>
      <small>{t('set.sigfoxDownlinkHint')}</small></span></label>
    <div class="actions"><button class="primary">{t('common.save')}</button></div>
  </form>
{/if}
