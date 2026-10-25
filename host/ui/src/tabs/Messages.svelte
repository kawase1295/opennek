<script lang="ts">
  import { api } from '../lib/api'
  import type { Message } from '../lib/api.gen'
  import { app } from '../lib/app.svelte'
  import { authBadge, dlBadge } from '../lib/badges'
  import { ask } from '../lib/confirm.svelte'
  import { decodedText, fmtTime, spaced } from '../lib/format'
  import { t } from '../lib/i18n'
  import Badge from '../components/Badge.svelte'
  import DeviceLabel from '../components/DeviceLabel.svelte'
  import EmptyState from '../components/EmptyState.svelte'
  import PageHeader from '../components/PageHeader.svelte'
  import MessageDialog from './MessageDialog.svelte'

  let detail: MessageDialog

  async function clear(): Promise<void> {
    const dev = app.deviceFilter
    const title = dev ? t('msg.confirmClearDevice', { device: dev }) : t('msg.confirmClearAll')
    if (!(await ask({ title, confirm: t('msg.clear'), danger: true }))) return
    await api.clearMessages(dev)
    await app.loadMessages()
  }

  const open = (m: Message) => detail.open(m)
</script>

<PageHeader title={t('nav.messages')} count={app.hasMore ? `${app.messages.length}+` : app.messages.length} intro={t('msg.intro')}>
  {#snippet actions()}
    <select bind:value={app.deviceFilter} onchange={() => app.loadMessages()} aria-label={t('msg.device')}>
      <option value="">{t('msg.allDevices')}</option>
      {#each app.devices as d (d.id)}
        <option value={d.id}>{d.id}{d.name ? ` (${d.name})` : ''}</option>
      {/each}
    </select>
    <a class="button" href={api.messagesCsvUrl(app.deviceFilter)} download>
      <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden="true"><path d="M8 2v8m0 0 3-3m-3 3L5 7M3 13h10" /></svg>CSV
    </a>
    <button class="danger" onclick={clear}>{t('msg.clear')}</button>
  {/snippet}
</PageHeader>
<div class="table-wrap">
  {#if app.messages.length}
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
                <Badge badge={dlBadge(m.downlinkStatus)} />{#if m.downlink}<span class="decoded">{spaced(m.downlink)}</span>{/if}
              {:else if m.oob}
                <span class="badge neutral soft">OOB</span>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <EmptyState title={t('msg.emptyTitle')} body={t('msg.empty')} />
  {/if}
</div>
{#if app.hasMore}<button id="msg-more" onclick={() => app.loadMessages(true)}>{t('msg.more')}</button>{/if}

<MessageDialog bind:this={detail} />
