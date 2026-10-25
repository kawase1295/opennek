<script lang="ts">
  import { api } from '../../lib/api'
  import type { CallbackSummary } from '../../lib/api.gen'
  import { app } from '../../lib/app.svelte'
  import { logBadge } from '../../lib/badges'
  import { information, isSent } from '../../lib/callbacks'
  import { ask } from '../../lib/confirm.svelte'
  import { fmtTime } from '../../lib/format'
  import { t } from '../../lib/i18n'
  import { errorText, showToast } from '../../lib/toast.svelte'
  import Badge from '../../components/Badge.svelte'
  import EmptyState from '../../components/EmptyState.svelte'
  import PageHeader from '../../components/PageHeader.svelte'

  let testing = $state<number | null>(null)

  // 一覧から変えるのは有効/無効とダウンリンクの選択だけ。残りの項目はそのまま保存し直す
  async function update(c: CallbackSummary, change: { enabled?: boolean; downlink?: boolean }): Promise<void> {
    const { id, lastLog: _, ...input } = c
    try {
      await api.saveCallback(id, { ...input, ...change })
      await app.loadCallbacks()
    } catch (err) {
      showToast(errorText(err))
    }
  }

  async function remove(c: CallbackSummary): Promise<void> {
    const ok = await ask({ title: t('cb.confirmDelete'), body: c.name || information(c), confirm: t('common.delete'), danger: true })
    if (!ok) return
    await api.deleteCallback(c.id)
    await app.loadCallbacks()
  }

  // 本物のリクエストが飛ぶので、宛先を見せてから送る
  async function test(c: CallbackSummary): Promise<void> {
    const body = t('cb.confirmTest', { method: c.httpMethod, url: c.url })
    if (!(await ask({ title: t('cb.confirmTestTitle'), body, confirm: t('cb.test') }))) return
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

  // メニューは選んだら閉じる
  function fromMenu(e: MouseEvent, run: () => void): void {
    ;(e.currentTarget as HTMLElement).closest('details')?.removeAttribute('open')
    run()
  }

  const edit = (id: number | null, copy?: number) =>
    (app.callbackView = copy === undefined ? { kind: 'edit', id } : { kind: 'edit', id, copy })
</script>

<PageHeader title={t('nav.callbacks')} count={app.callbacks.length} intro={t('cb.introSigfox')}>
  {#snippet actions()}
    <button class="primary" onclick={() => edit(null)}>
      <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M8 3v10M3 8h10" /></svg>{t('cb.new')}
    </button>
  {/snippet}
</PageHeader>

<div class="table-wrap">
  {#if app.callbacks.length}
    <table class="callbacks">
      <thead>
        <tr>
          <th title={t('cb.bidirHint')}>{t('cb.colDownlink')}</th><th>{t('cbForm.type')}</th><th>{t('cbForm.channel')}</th>
          <th>Information</th><th>{t('cb.colLast')}</th><th>{t('cb.colEnabled')}</th><th></th>
        </tr>
      </thead>
      <tbody>
        {#each app.callbacks as c (c.id)}
          {@const sent = isSent(c)}
          <tr class:disabled={!c.enabled}>
            <td class="pick">
              {#if c.callbackSubtype === 'BIDIR'}
                <input type="checkbox" class="radio" checked={c.downlink} aria-label={t('cb.chooseDownlink')} title={t('cb.chooseDownlink')}
                  onchange={() => update(c, { downlink: !c.downlink })} />
              {/if}
            </td>
            <td><div class="type"><b>{c.callbackType}</b>{#if c.callbackSubtype}<span>{c.callbackSubtype}</span>{/if}</div></td>
            <td class="mono">{c.channel}</td>
            <td><div class="info">
              {#if c.name}<strong>{c.name}</strong>{/if}
              <span class="mono summary" title={information(c)}>{information(c)}</span>
              <span class="meta">
                {#if c.channel !== 'EMAIL' && c.httpMethod !== 'GET'}<span class="badge neutral soft">{c.contentType}</span>{/if}
                <span class="badge neutral soft">{t('cb.target', { devices: c.devices || t('cb.allDevices') })}</span>
                {#if !sent}<span class="badge warn dot">{t('cb.notSent')}</span>{/if}
              </span>
            </div></td>
            <td>
              {#if c.lastLog}
                <button class="ghost last" onclick={() => (app.callbackView = { kind: 'history', id: c.id })}>
                  <Badge badge={logBadge(c.lastLog)} /><span class="when">{fmtTime(c.lastLog.time)}</span>
                </button>
              {:else}
                <span class="muted">—</span>
              {/if}
            </td>
            <td>
              <input type="checkbox" class="switch" checked={c.enabled} aria-label={t('cb.colEnabled')}
                onchange={() => update(c, { enabled: !c.enabled })} />
            </td>
            <td>
              <div class="actions">
                <button class="small" disabled={!sent || testing === c.id} onclick={() => test(c)}>{t('cb.test')}</button>
                <button class="small" onclick={() => edit(c.id)}>{t('common.edit')}</button>
                <details class="menu">
                  <summary class="button small" aria-label={t('cb.more')}>⋯</summary>
                  <div class="menu-items">
                    <button class="ghost" onclick={(e) => fromMenu(e, () => (app.callbackView = { kind: 'history', id: c.id }))}>{t('cb.history')}</button>
                    <button class="ghost" onclick={(e) => fromMenu(e, () => edit(null, c.id))}>{t('cb.duplicate')}</button>
                    <button class="ghost danger" onclick={(e) => fromMenu(e, () => void remove(c))}>{t('common.delete')}</button>
                  </div>
                </details>
              </div>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <EmptyState title={t('cb.empty')} body={t('cb.intro')} />
  {/if}
</div>
