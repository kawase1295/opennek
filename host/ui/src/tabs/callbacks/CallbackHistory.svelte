<script lang="ts">
  import { onMount } from 'svelte'
  import { api } from '../../lib/api'
  import type { CallbackLog } from '../../lib/api.gen'
  import { app } from '../../lib/app.svelte'
  import { logBadge } from '../../lib/badges'
  import { information } from '../../lib/callbacks'
  import { fmtTime } from '../../lib/format'
  import { t } from '../../lib/i18n'
  import Badge from '../../components/Badge.svelte'
  import EmptyState from '../../components/EmptyState.svelte'
  import PageHeader from '../../components/PageHeader.svelte'

  let { id }: { id: number } = $props()
  const LIMIT = 100

  const cb = $derived(app.callbacks.find((c) => c.id === id))
  let logs = $state<CallbackLog[]>([])

  onMount(async () => {
    logs = await api.callbackLogs({ callback: id, limit: LIMIT })
  })
</script>

<PageHeader title={t('cb.historyOf', { name: cb?.name || `#${id}` })} intro={cb ? information(cb) : ''}>
  {#snippet actions()}
    <button class="ghost" onclick={() => (app.callbackView = { kind: 'list' })}>← {t('cb.back')}</button>
  {/snippet}
</PageHeader>
<div class="table-wrap">
  {#if logs.length}
    <table>
      <thead>
        <tr><th>{t('log.time')}</th><th>{t('log.message')}</th><th>{t('log.result')}</th><th>{t('log.duration')}</th><th>{t('log.response')}</th></tr>
      </thead>
      <tbody>
        {#each logs as l (l.id)}
          <tr>
            <td>{fmtTime(l.time)}</td>
            <td class="mono">#{l.messageId}</td>
            <td><Badge badge={logBadge(l)} /></td>
            <td>{l.durationMs} ms</td>
            <td class="mono">{(l.response ?? l.error ?? '').slice(0, 120)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <EmptyState title={t('cb.noHistory')} />
  {/if}
</div>
