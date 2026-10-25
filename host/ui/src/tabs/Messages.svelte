<script lang="ts">
  import { api } from '../lib/api'
  import type { Message } from '../lib/api.gen'
  import { app } from '../lib/app.svelte'
  import { authBadge, dlBadge } from '../lib/badges'
  import { decodedText, fmtTime, spaced } from '../lib/format'
  import { t } from '../lib/i18n'
  import Badge from '../components/Badge.svelte'
  import DeviceLabel from '../components/DeviceLabel.svelte'
  import MessageDialog from './MessageDialog.svelte'

  let detail: MessageDialog

  async function clear(): Promise<void> {
    const dev = app.deviceFilter
    if (!confirm(dev ? t('msg.confirmClearDevice', { device: dev }) : t('msg.confirmClearAll'))) return
    await api.clearMessages(dev)
    await app.loadMessages()
  }

  const open = (m: Message) => detail.open(m)
</script>

<div class="toolbar">
  <select bind:value={app.deviceFilter} onchange={() => app.loadMessages()}>
    <option value="">{t('msg.allDevices')}</option>
    {#each app.devices as d (d.id)}
      <option value={d.id}>{d.id}{d.name ? ` (${d.name})` : ''}</option>
    {/each}
  </select>
  <span class="spacer"></span>
  <a class="button" href={api.messagesCsvUrl(app.deviceFilter)} download>CSV</a>
  <button class="danger" onclick={clear}>{t('msg.clear')}</button>
</div>
<div class="table-wrap">
  <table id="msg-table">
    <thead>
      <tr>
        <th>{t('msg.time')}</th><th>{t('msg.device')}</th><th>Seq</th><th>{t('msg.data')}</th>
        <th>RSSI</th><th>{t('msg.frames')}</th><th>{t('msg.auth')}</th><th>{t('msg.downlink')}</th>
      </tr>
    </thead>
    <tbody>
      {#each app.messages as m (m.id)}
        <tr class:new={app.live.has(m.id)} onclick={() => open(m)}>
          <td>{fmtTime(m.time)}</td>
          <td><DeviceLabel id={m.deviceId} /></td>
          <td>{m.seq}</td>
          <td class="data">{spaced(m.data)}{#if m.decoded}<span class="decoded">{decodedText(m.decoded)}</span>{/if}</td>
          <td>{m.rssi} dBm</td>
          <td>{m.nbFrames}/3</td>
          <td><Badge badge={authBadge(m.auth)} /></td>
          <!-- データはバッジの下の段に置き、列の幅を広げない -->
          <td>
            {#if m.ack}
              <Badge badge={dlBadge(m.downlinkStatus)} />{#if m.downlink}<span class="decoded mono">{spaced(m.downlink)}</span>{/if}
            {:else if m.oob}
              <span class="badge muted">OOB</span>
            {/if}
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
  {#if app.messages.length === 0}<p class="empty">{t('msg.empty')}</p>{/if}
</div>
{#if app.hasMore}<button id="msg-more" onclick={() => app.loadMessages(true)}>{t('msg.more')}</button>{/if}

<MessageDialog bind:this={detail} />
