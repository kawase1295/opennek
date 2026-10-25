<script lang="ts">
  import { api } from '../lib/api'
  import type { Language } from '../lib/api.gen'
  import { app, TABS, type Tab } from '../lib/app.svelte'
  import { LANGUAGES, lang, t } from '../lib/i18n'
  import { radioPill } from '../lib/radio'
  import { errorText, showToast } from '../lib/toast.svelte'

  const NAV: Record<Tab, string> = {
    messages: t('nav.messages'),
    devices: t('nav.devices'),
    callbacks: t('nav.callbacks'),
    log: t('nav.log'),
    settings: t('nav.settings'),
  }
  const pill = $derived(radioPill(app.radio))
  let chosen = $state<Language>(lang)

  // 文字列は起動時に一度だけ訳すので、設定に保存して読み込み直す
  async function changeLang(): Promise<void> {
    try {
      await api.saveSettings({ language: chosen })
      location.reload()
    } catch (err) {
      chosen = lang
      showToast(errorText(err))
    }
  }
</script>

<header>
  <div class="brand">
    <svg viewBox="0 0 32 32" aria-hidden="true"><circle cx="16" cy="16" r="5" /><path d="M8 8a11 11 0 0 0 0 16M24 8a11 11 0 0 1 0 16" /></svg>
    <span>OpenNEK</span>
  </div>
  <nav>
    {#each TABS as tab (tab)}
      <button class:active={app.tab === tab} aria-current={app.tab === tab ? 'page' : undefined} onclick={() => app.showTab(tab)}>{NAV[tab]}</button>
    {/each}
  </nav>
  <div class="header-end">
    <span class="radio-pill {pill.tone}" title={pill.title || t('pill.radio')}>
      <i></i>{pill.label}{#if pill.detail}<span class="detail">{pill.detail}</span>{/if}
    </span>
    <select aria-label="Language" bind:value={chosen} onchange={changeLang}>
      {#each Object.entries(LANGUAGES) as [code, name] (code)}
        <option value={code}>{name}</option>
      {/each}
    </select>
  </div>
</header>
