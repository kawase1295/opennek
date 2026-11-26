<script lang="ts">
  import { api, type UplinkInput } from '../lib/api'
  import { app } from '../lib/app.svelte'
  import { t } from '../lib/i18n'
  import { errorText, showToast } from '../lib/toast.svelte'

  let dialog: HTMLDialogElement
  let form = $state({ device: '', data: '', ack: false, key: '' })
  let error = $state('')
  let sending = $state(false)

  export function open(): void {
    form = { device: app.deviceFilter || (app.devices[0]?.id ?? ''), data: '', ack: false, key: '' }
    error = ''
    dialog.showModal()
  }

  async function send(e: SubmitEvent): Promise<void> {
    e.preventDefault()
    const input: UplinkInput = { device: form.device, data: form.data, ack: form.ack }
    if (form.key) input.key = form.key
    sending = true
    try {
      await api.simulateUplink(input)
      dialog.close()
      showToast(t('sim.sent'))
    } catch (err) {
      error = errorText(err)
    } finally {
      sending = false
    }
  }
</script>

<dialog bind:this={dialog}>
  <form class="form" onsubmit={send}>
    <h3>{t('msg.simulate')}</h3>
    <p class="muted">{t('sim.intro')}</p>
    <label><span>{t('sim.device')}</span>
      <input class="mono" bind:value={form.device} list="sim-devices" maxlength="8" pattern="[0-9A-Fa-f]{'{8}'}" required />
      <datalist id="sim-devices">
        {#each app.devices as d (d.id)}<option value={d.id}>{d.name}</option>{/each}
      </datalist></label>
    <label><span>{t('sim.data')}</span>
      <input class="mono" bind:value={form.data} maxlength="24" pattern={'([0-9A-Fa-f]{2})*'} placeholder="0102" /></label>
    <label class="check"><input type="checkbox" bind:checked={form.ack} /><span class="check-text"><span>{t('sim.ack')}</span>
      <small>{t('sim.ackHint')}</small></span></label>
    <label><span>{t('sim.key')}</span>
      <input class="mono" bind:value={form.key} maxlength="32" pattern="[0-9A-Fa-f]{'{32}'}" />
      <small>{t('sim.keyHint')}</small></label>
    <p class="error">{error}</p>
    <div class="actions">
      <button type="button" onclick={() => dialog.close()}>{t('common.cancel')}</button>
      <button class="primary" disabled={sending}>{t('sim.send')}</button>
    </div>
  </form>
</dialog>
