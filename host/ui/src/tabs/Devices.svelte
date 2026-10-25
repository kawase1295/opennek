<script lang="ts">
  import { api } from '../lib/api'
  import type { Device } from '../lib/api.gen'
  import { app } from '../lib/app.svelte'
  import { ask } from '../lib/confirm.svelte'
  import { fmtTime, spaced } from '../lib/format'
  import { t } from '../lib/i18n'
  import EmptyState from '../components/EmptyState.svelte'
  import PageHeader from '../components/PageHeader.svelte'
  import DeviceDialog from './DeviceDialog.svelte'

  let editor: DeviceDialog

  async function remove(d: Device): Promise<void> {
    if (!(await ask({ title: t('dev.confirmDelete', { device: d.id }), confirm: t('common.delete'), danger: true }))) return
    await api.deleteDevice(d.id)
    await app.loadDevices()
  }
</script>

<PageHeader title={t('nav.devices')} count={app.devices.length} intro={t('dev.intro')}>
  {#snippet actions()}
    <button class="primary" onclick={() => editor.open(null)}>{t('common.add')}</button>
  {/snippet}
</PageHeader>
<div class="table-wrap">
  {#if app.devices.length}
  <table>
    <thead>
      <tr>
        <th>ID</th><th>{t('dev.name')}</th><th>{t('dev.key')}</th><th>{t('dev.downlink')}</th>
        <th>{t('dev.lastSeen')}</th><th>{t('dev.lastSeq')}</th><th></th>
      </tr>
    </thead>
    <tbody>
      {#each app.devices as d (d.id)}
        <tr>
          <td class="mono">{d.id}</td>
          <td>{#if d.name}{d.name}{:else}<span class="muted">—</span>{/if}</td>
          <td>
            {#if d.key}<span class="badge ok dot">{t('dev.registered')}</span>
            {:else}<span class="badge neutral soft">{t('dev.unregistered')}</span>{/if}
          </td>
          <td>
            {t(`dlMode.${d.downlinkMode}`)}
            {#if d.downlinkMode === 'static'}<code>{d.downlinkData.includes('{') ? d.downlinkData : spaced(d.downlinkData)}</code>{/if}
          </td>
          <td>{fmtTime(d.lastSeen)}</td>
          <td>{d.lastSeq ?? '—'}</td>
          <td>
            <div class="actions">
              <button class="small" onclick={() => editor.open(d)}>{t('common.edit')}</button>
              <button class="small danger" onclick={() => remove(d)}>{t('common.delete')}</button>
            </div>
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
  {:else}
    <EmptyState title={t('dev.empty')} />
  {/if}
</div>

<DeviceDialog bind:this={editor} />
