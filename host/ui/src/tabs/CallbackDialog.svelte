<script lang="ts">
  import { tick } from 'svelte'
  import { api } from '../lib/api'
  import type { Callback, CallbackInput } from '../lib/api.gen'
  import { app } from '../lib/app.svelte'
  import { DEFAULT_BODY, formatHeaders, parseHeaders } from '../lib/callbacks'
  import { t } from '../lib/i18n'
  import { errorText, showToast } from '../lib/toast.svelte'

  const VARIABLES = [
    '{device}', '{deviceName}', '{time}', '{data}', '{seqNumber}', '{ack}',
    '{rssi}', '{snr}', '{station}', '{nbFrames}', '{oob}', '{customData#name}',
  ]

  let dialog: HTMLDialogElement
  let bodyField: HTMLTextAreaElement
  let id = $state<number | null>(null)
  let form = $state<Omit<CallbackInput, 'headers'>>(blank())
  let headers = $state('')
  let error = $state('')

  function blank(): Omit<CallbackInput, 'headers'> {
    return {
      name: '', enabled: true, devices: '', method: 'POST', url: '',
      contentType: 'application/json', body: DEFAULT_BODY, downlink: false,
    }
  }

  export function open(c: Callback | null): void {
    id = c?.id ?? null
    if (c) {
      const { id: _, headers: h, ...rest } = c
      form = rest
      headers = formatHeaders(h)
    } else {
      form = blank()
      headers = ''
    }
    error = ''
    dialog.showModal()
  }

  // 変数を押すと本文のカーソル位置に入れる
  async function insert(variable: string): Promise<void> {
    bodyField.setRangeText(variable, bodyField.selectionStart, bodyField.selectionEnd, 'end')
    form.body = bodyField.value
    await tick()
    bodyField.focus()
  }

  async function save(e: SubmitEvent): Promise<void> {
    e.preventDefault()
    try {
      await api.saveCallback(id, { ...form, headers: parseHeaders(headers) })
      dialog.close()
      showToast(t('common.saved'))
      await app.loadCallbacks()
    } catch (err) {
      error = errorText(err)
    }
  }
</script>

<dialog bind:this={dialog}>
  <form class="form" onsubmit={save}>
    <h3>{t('cbForm.title')}</h3>
    <label><span>{t('cbForm.name')}</span><input bind:value={form.name} maxlength="64" /></label>
    <div class="row">
      <label><span>{t('cbForm.method')}</span>
        <select bind:value={form.method}><option>POST</option><option>PUT</option><option>GET</option></select></label>
      <label class="grow">URL<input bind:value={form.url} type="url" required placeholder={'https://example.com/uplink?device={device}'} /></label>
    </div>
    <label><span>{t('cbForm.devices')}</span><input bind:value={form.devices} placeholder={t('cbForm.devicesPlaceholder')} /></label>
    <label><span>{t('cbForm.headers')}</span><textarea bind:value={headers} rows="2" placeholder="Authorization: Bearer xxxx"></textarea></label>
    <label>Content-Type<input bind:value={form.contentType} /></label>
    <label><span>{t('cbForm.body')}</span><textarea bind:this={bodyField} bind:value={form.body} rows="5" class="mono"></textarea>
      <small class="vars"><span>{t('cbForm.vars')}</span>
        {#each VARIABLES as v (v)}
          <button type="button" class="var" onclick={() => insert(v)}><code>{v}</code></button>{' '}
        {/each}
      </small></label>
    <label class="check"><input type="checkbox" bind:checked={form.downlink} /><span>{@html t('cbForm.downlink')}</span></label>
    <label class="check"><input type="checkbox" bind:checked={form.enabled} /><span>{t('cbForm.enabled')}</span></label>
    <p class="error">{error}</p>
    <div class="actions">
      <button type="button" onclick={() => dialog.close()}>{t('common.cancel')}</button>
      <button class="primary">{t('common.save')}</button>
    </div>
  </form>
</dialog>
