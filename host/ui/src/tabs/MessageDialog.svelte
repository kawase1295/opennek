<script lang="ts">
  import { api } from '../lib/api'
  import type { CallbackLog, Message } from '../lib/api.gen'
  import { app } from '../lib/app.svelte'
  import { authBadge, dlBadge, logBadge } from '../lib/badges'
  import { fmtTime, spaced } from '../lib/format'
  import { t } from '../lib/i18n'
  import Badge from '../components/Badge.svelte'
  import DeviceLabel from '../components/DeviceLabel.svelte'

  let dialog: HTMLDialogElement
  let m = $state<Message | null>(null)
  let logs = $state<CallbackLog[]>([])

  export async function open(message: Message): Promise<void> {
    m = message
    logs = await api.callbackLogs({ message: message.id })
    dialog.showModal()
  }
</script>

<dialog bind:this={dialog} id="msg-dialog">
  {#if m}
    <div class="form">
      <h3>{t('detail.title')}</h3>
      <dl class="kv">
        <dt>ID</dt><dd>{m.id}</dd>
        <dt>{t('kv.time')}</dt><dd>{fmtTime(m.time)}</dd>
        <dt>{t('kv.device')}</dt><dd><DeviceLabel id={m.deviceId} /></dd>
        <dt>Seq</dt><dd>{m.seq}</dd>
        <dt>{t('kv.data')}</dt><dd><code>{spaced(m.data)}</code></dd>
        <dt>{t('kv.decoded')}</dt><dd>{#if m.decoded}<code>{JSON.stringify(m.decoded)}</code>{:else}—{/if}</dd>
        <dt>RSSI</dt><dd>{m.rssi} dBm</dd>
        <dt>{t('kv.frames')}</dt><dd>{m.nbFrames}/3</dd>
        <dt>{t('kv.auth')}</dt><dd><Badge badge={authBadge(m.auth)} /></dd>
        <dt>{t('kv.downlink')}</dt>
        <dd>
          {#if m.ack}
            <Badge badge={dlBadge(m.downlinkStatus)} />
            {#if m.downlink}<code>{spaced(m.downlink)}</code>{/if}
          {:else}
            {t('kv.noDownlink')}
          {/if}
        </dd>
      </dl>
      <h4>{t('detail.callbacks')}</h4>
      <div>
        {#each logs as l (l.id)}
          <div>
            <strong>{app.callbackName(l.callbackId)}</strong>
            <Badge badge={logBadge(l)} />
            <span class="muted">{l.durationMs} ms</span>
            {#if l.error && l.status}<span class="muted">{l.error}</span>{/if}
            {#if l.response}<pre>{l.response}</pre>{/if}
          </div>
        {:else}
          <p class="muted">{t('msg.noLogs')}</p>
        {/each}
      </div>
      <div class="actions"><button onclick={() => dialog.close()}>{t('common.close')}</button></div>
    </div>
  {/if}
</dialog>
