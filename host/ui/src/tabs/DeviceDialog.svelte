<script lang="ts">
  import { api, type DeviceInput } from '../lib/api'
  import type { Device } from '../lib/api.gen'
  import { app } from '../lib/app.svelte'
  import { t } from '../lib/i18n'
  import { errorText, showToast } from '../lib/toast.svelte'

  let dialog: HTMLDialogElement
  let editing = $state(false)
  let id = $state('')
  let form = $state<DeviceInput>(blank())
  let error = $state('')

  function blank(): DeviceInput {
    return { name: '', key: '', payloadConfig: '', downlinkMode: 'none', downlinkData: '' }
  }

  export function open(d: Device | null): void {
    editing = d !== null
    id = d?.id ?? ''
    form = d
      ? { name: d.name, key: d.key, payloadConfig: d.payloadConfig, downlinkMode: d.downlinkMode, downlinkData: d.downlinkData }
      : blank()
    error = ''
    dialog.showModal()
  }

  async function save(e: SubmitEvent): Promise<void> {
    e.preventDefault()
    try {
      await api.saveDevice(id, { ...form, downlinkData: form.downlinkMode === 'static' ? form.downlinkData : '' })
      dialog.close()
      showToast(t('common.saved'))
      await app.loadDevices()
      await app.loadMessages()
    } catch (err) {
      error = errorText(err)
    }
  }
</script>

<dialog bind:this={dialog}>
  <form class="form" onsubmit={save}>
    <h3>{t('devForm.title')}</h3>
    <label><span>{t('devForm.id')}</span><input bind:value={id} readonly={editing} maxlength="8" pattern="[0-9A-Fa-f]{'{8}'}" required /></label>
    <label><span>{t('devForm.name')}</span><input bind:value={form.name} maxlength="64" /></label>
    <label><span>{t('devForm.key')}</span><input bind:value={form.key} maxlength="32" pattern="[0-9A-Fa-f]{'{32}'}" placeholder={t('devForm.keyPlaceholder')} /></label>
    <label><span>{t('devForm.payload')}</span>
      <input bind:value={form.payloadConfig} placeholder="temp::int:16:little-endian hum::uint:8 door:0:bool:7" />
      <small>{t('devForm.payloadHint')}</small></label>
    <label><span>{t('devForm.downlink')}</span>
      <select bind:value={form.downlinkMode}>
        <option value="none">{t('devForm.dlNone')}</option>
        <option value="static">{t('devForm.dlStatic')}</option>
        <option value="callback">{t('devForm.dlCallback')}</option>
      </select></label>
    {#if form.downlinkMode === 'static'}
      <label><span>{t('devForm.static')}</span>
        <input bind:value={form.downlinkData} maxlength="48" pattern={'([0-9A-Fa-f]{2}|\\{(time|tapid|rssi)\\})+'} placeholder={'{time}0102{rssi}'} required />
        <small>{@html t('devForm.staticHint')}</small></label>
    {/if}
    <p class="error">{error}</p>
    <div class="actions">
      <button type="button" onclick={() => dialog.close()}>{t('common.cancel')}</button>
      <button class="primary">{t('common.save')}</button>
    </div>
  </form>
</dialog>
