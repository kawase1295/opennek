<script lang="ts">
  import { api } from '../lib/api'
  import type { Device } from '../lib/api.gen'
  import { app } from '../lib/app.svelte'
  import { fmtTime, spaced } from '../lib/format'
  import { t } from '../lib/i18n'
  import DeviceDialog from './DeviceDialog.svelte'

  let editor: DeviceDialog

  async function remove(d: Device): Promise<void> {
    if (!confirm(t('dev.confirmDelete', { device: d.id }))) return
    await api.deleteDevice(d.id)
    await app.loadDevices()
  }
</script>

<div class="toolbar">
  <span class="muted">{t('dev.intro')}</span>
  <span class="spacer"></span>
  <button class="primary" onclick={() => editor.open(null)}>{t('common.add')}</button>
</div>
<div class="table-wrap">
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
          <td><code>{d.id}</code></td>
          <td>{#if d.name}{d.name}{:else}<span class="muted">—</span>{/if}</td>
          <td>
            {#if d.key}<span class="badge ok">{t('dev.registered')}</span>
            {:else}<span class="badge muted">{t('dev.unregistered')}</span>{/if}
          </td>
          <td>
            {t(`dlMode.${d.downlinkMode}`)}
            {#if d.downlinkMode === 'static'}<code>{d.downlinkData.includes('{') ? d.downlinkData : spaced(d.downlinkData)}</code>{/if}
          </td>
          <td>{fmtTime(d.lastSeen)}</td>
          <td>{d.lastSeq ?? '—'}</td>
          <td>
            <button class="small" onclick={() => editor.open(d)}>{t('common.edit')}</button>
            <button class="small danger" onclick={() => remove(d)}>{t('common.delete')}</button>
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
  {#if app.devices.length === 0}<p class="empty">{t('dev.empty')}</p>{/if}
</div>

<DeviceDialog bind:this={editor} />
