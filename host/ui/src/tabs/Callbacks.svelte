<script lang="ts">
  import { onMount } from 'svelte'
  import { api } from '../lib/api'
  import type { Callback } from '../lib/api.gen'
  import { app } from '../lib/app.svelte'
  import { logBadge } from '../lib/badges'
  import { fmtTime } from '../lib/format'
  import { t } from '../lib/i18n'
  import { errorText, showToast } from '../lib/toast.svelte'
  import Badge from '../components/Badge.svelte'
  import CallbackDialog from './CallbackDialog.svelte'

  let editor: CallbackDialog
  let testing = $state<number | null>(null)

  onMount(() => void app.loadCallbackLogs())

  async function remove(c: Callback): Promise<void> {
    if (!confirm(t('cb.confirmDelete'))) return
    await api.deleteCallback(c.id)
    await app.loadCallbacks()
  }

  // 本物のリクエストが飛ぶので、宛先を見せてから送る
  async function test(c: Callback): Promise<void> {
    if (!confirm(t('cb.confirmTest', { method: c.method, url: c.url }))) return
    testing = c.id
    try {
      const { result } = await api.testCallback(c.id)
      showToast(
        result.ok
          ? `${t('cb.testOk', { status: result.status ?? '', ms: result.durationMs })}${result.downlink ? ` / downlink ${result.downlink}` : ''}`
          : t('cb.testFailed', { error: result.error ?? '' }),
      )
    } catch (err) {
      showToast(errorText(err))
    } finally {
      testing = null
    }
  }
</script>

<div class="toolbar">
  <span class="muted">{t('cb.intro')}</span>
  <span class="spacer"></span>
  <button class="primary" onclick={() => editor.open(null)}>{t('common.add')}</button>
</div>
<div class="cards">
  {#each app.callbacks as c (c.id)}
    <div class="card">
      <div>
        <strong>{c.name || t('cb.untitled', { id: c.id })}</strong>
        {#if c.enabled}<span class="badge ok">{t('cb.enabled')}</span>{:else}<span class="badge muted">{t('cb.disabled')}</span>{/if}
        {#if c.downlink}<span class="badge warn">{t('cb.downlink')}</span>{/if}
      </div>
      <div class="url" title={c.url}><code>{c.method}</code> {c.url}</div>
      <div class="muted">{t('cb.target', { devices: c.devices || t('cb.allDevices') })}</div>
      <div class="actions">
        <button class="small" disabled={testing === c.id} onclick={() => test(c)}>{t('cb.test')}</button>
        <button class="small" onclick={() => editor.open(c)}>{t('common.edit')}</button>
        <button class="small danger" onclick={() => remove(c)}>{t('common.delete')}</button>
      </div>
    </div>
  {/each}
</div>
{#if app.callbacks.length === 0}<p class="empty">{t('cb.empty')}</p>{/if}
<h3>{t('cb.log')}</h3>
<div class="table-wrap">
  <table>
    <thead>
      <tr>
        <th>{t('log.time')}</th><th>{t('log.callback')}</th><th>{t('log.message')}</th>
        <th>{t('log.result')}</th><th>{t('log.duration')}</th><th>{t('log.response')}</th>
      </tr>
    </thead>
    <tbody>
      {#each app.callbackLogs as l (l.id)}
        <tr>
          <td>{fmtTime(l.time)}</td>
          <td>{app.callbacks.find((c) => c.id === l.callbackId)?.name || `#${l.callbackId}`}</td>
          <td>#{l.messageId}</td>
          <td><Badge badge={logBadge(l)} /></td>
          <td>{l.durationMs} ms</td>
          <td class="mono">{(l.response ?? l.error ?? '').slice(0, 80)}</td>
        </tr>
      {/each}
    </tbody>
  </table>
</div>

<CallbackDialog bind:this={editor} />
