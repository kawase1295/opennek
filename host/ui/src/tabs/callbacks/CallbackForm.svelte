<script lang="ts">
  import { onMount, tick } from 'svelte'
  import { api } from '../../lib/api'
  import type { CallbackInput, CallbackSubtype, CallbackType } from '../../lib/api.gen'
  import { app } from '../../lib/app.svelte'
  import {
    blankCallback,
    copyOf,
    headersOf,
    isSent,
    rowsOf,
    SUBTYPES,
    TYPES,
    withSubtype,
    withType,
    type HeaderRow,
  } from '../../lib/callbacks'
  import { t } from '../../lib/i18n'
  import { errorText, showToast } from '../../lib/toast.svelte'
  import PageHeader from '../../components/PageHeader.svelte'

  // id: 編集するコールバック (null なら新規)。copy: 複製の元
  let { id, copy }: { id: number | null; copy?: number | undefined } = $props()

  const CHANNELS = ['URL', 'BATCH_URL', 'EMAIL'] as const
  const METHODS = ['POST', 'PUT', 'GET'] as const
  const CONTENT_TYPES = ['application/json', 'text/plain', 'application/x-www-form-urlencoded'] as const
  const VARIABLES = [
    '{device}', '{deviceName}', '{time}', '{data}', '{seqNumber}', '{ack}', '{oob}',
    '{rssi}', '{snr}', '{station}', '{nbFrames}', '{customData#name}',
  ]

  let form = $state<CallbackInput>(blankCallback())
  let headers = $state<HeaderRow[]>([])
  let error = $state('')
  let saving = $state(false)
  // 変数を入れる先: 最後に触った Url pattern・ヘッダーの値・本文
  let target: HTMLInputElement | HTMLTextAreaElement | null = null

  onMount(() => {
    const source = app.callbacks.find((c) => c.id === (id ?? copy))
    if (source) {
      const { id: _, lastLog: __, ...input } = source
      form = id === null ? copyOf(input) : input
      headers = rowsOf(input.headers)
    }
  })

  const remember = (e: FocusEvent) => (target = e.currentTarget as HTMLInputElement | HTMLTextAreaElement)

  async function insert(variable: string): Promise<void> {
    if (!target) return
    target.setRangeText(variable, target.selectionStart ?? target.value.length, target.selectionEnd ?? target.value.length, 'end')
    target.dispatchEvent(new Event('input', { bubbles: true }))
    await tick()
    target.focus()
  }

  const setType = (type: CallbackType) => (form = withType(form, type))
  const setSubtype = (subtype: CallbackSubtype) => (form = withSubtype(form, subtype))
  const back = () => (app.callbackView = { kind: 'list' })

  async function save(e: SubmitEvent): Promise<void> {
    e.preventDefault()
    saving = true
    try {
      await api.saveCallback(id, { ...form, headers: headersOf(headers) })
      showToast(t('common.saved'))
      await app.loadCallbacks()
      back()
    } catch (err) {
      error = errorText(err)
    } finally {
      saving = false
    }
  }
</script>

<PageHeader title={id === null ? t('cb.new') : t('cb.editTitle')}>
  {#snippet actions()}
    <button class="ghost" onclick={back}>← {t('cb.back')}</button>
  {/snippet}
</PageHeader>

<div class="callback-form">
  <form class="card form" onsubmit={save}>
    <div class="fields">
      <label class="f"><span class="l">{t('cbForm.type')}</span>
        <span class="c pair">
          <select value={form.callbackType} onchange={(e) => setType(e.currentTarget.value as CallbackType)} aria-label="Type">
            {#each TYPES as v (v)}<option value={v}>{v}</option>{/each}
          </select>
          {#if SUBTYPES[form.callbackType].length}
            <select value={form.callbackSubtype} onchange={(e) => setSubtype(e.currentTarget.value as CallbackSubtype)} aria-label="Subtype">
              {#each SUBTYPES[form.callbackType] as v (v)}<option value={v}>{v}</option>{/each}
            </select>
          {/if}
        </span>
      </label>
      <label class="f"><span class="l">{t('cbForm.channel')}</span>
        <span class="c pair"><select bind:value={form.channel}>{#each CHANNELS as v (v)}<option value={v}>{v}</option>{/each}</select></span>
      </label>
      <div class="f"><span class="l">{t('cbForm.sendDuplicate')}</span>
        <label class="c check"><input type="checkbox" bind:checked={form.sendDuplicate} /><small>{t('cbForm.sendDuplicateHint')}</small></label>
      </div>
      <label class="f"><span class="l">{t('cbForm.payloadConfig')}</span>
        <span class="c"><input class="mono" bind:value={form.payloadConfig} placeholder="temp::int:16:little-endian hum::uint:8" />
          <small>{t('cbForm.payloadConfigHint')}</small></span>
      </label>
    </div>

    {#if form.channel === 'URL'}
      <div class="fields"><span class="legend">URL</span>
        <label class="f"><span class="l">{t('cbForm.urlPattern')}</span>
          <span class="c"><input class="mono" type="url" bind:value={form.url} onfocus={remember} required placeholder={'https://example.com/uplink?device={device}'} /></span>
        </label>
        <label class="f"><span class="l">{t('cbForm.httpMethod')}</span>
          <span class="c pair"><select bind:value={form.httpMethod}>{#each METHODS as v (v)}<option value={v}>{v}</option>{/each}</select></span>
        </label>
        <div class="f"><span class="l">{t('cbForm.sendSni')}</span>
          <label class="c check"><input type="checkbox" bind:checked={form.sendSni} /><small>{t('cbForm.sendSniHint')}</small></label>
        </div>
        <div class="f"><span class="l">{t('cbForm.headers')}</span>
          <span class="c">
            {#each headers as h, i (i)}
              <span class="header-row">
                <input class="mono" bind:value={h.name} placeholder={t('cbForm.headerName')} aria-label={t('cbForm.headerName')} />
                <input class="mono" bind:value={h.value} onfocus={remember} placeholder={t('cbForm.headerValue')} aria-label={t('cbForm.headerValue')} />
                <button type="button" class="small ghost" aria-label={t('common.delete')} onclick={() => headers.splice(i, 1)}>×</button>
              </span>
            {/each}
            <button type="button" class="small" onclick={() => headers.push({ name: '', value: '' })}>{t('cbForm.addHeader')}</button>
          </span>
        </div>
        {#if form.httpMethod !== 'GET'}
          <label class="f"><span class="l">{t('cbForm.contentType')}</span>
            <span class="c pair"><select bind:value={form.contentType}>{#each CONTENT_TYPES as v (v)}<option value={v}>{v}</option>{/each}</select></span>
          </label>
          <label class="f"><span class="l">{t('cbForm.body')}</span>
            <span class="c"><textarea class="mono" rows="6" bind:value={form.bodyTemplate} onfocus={remember}></textarea></span>
          </label>
        {/if}
      </div>
    {:else if form.channel === 'BATCH_URL'}
      <div class="fields"><span class="legend">BATCH_URL</span>
        <label class="f"><span class="l">{t('cbForm.urlPattern')}</span>
          <span class="c"><input class="mono" type="url" bind:value={form.url} onfocus={remember} required /></span>
        </label>
        <label class="f"><span class="l">{t('cbForm.linePattern')}</span>
          <span class="c"><input class="mono" bind:value={form.linePattern} onfocus={remember} placeholder={'{device};{data};'} /></span>
        </label>
        <div class="f"><span class="l">{t('cbForm.sendSni')}</span>
          <label class="c check"><input type="checkbox" bind:checked={form.sendSni} /><small>{t('cbForm.sendSniHint')}</small></label>
        </div>
      </div>
    {:else}
      <div class="fields"><span class="legend">EMAIL</span>
        <label class="f"><span class="l">{t('cbForm.recipient')}</span>
          <span class="c"><input type="email" bind:value={form.recipient} required /></span>
        </label>
        <label class="f"><span class="l">{t('cbForm.subject')}</span>
          <span class="c"><input bind:value={form.subject} onfocus={remember} /></span>
        </label>
        <label class="f"><span class="l">{t('cbForm.message')}</span>
          <span class="c"><textarea rows="5" bind:value={form.message} onfocus={remember}></textarea></span>
        </label>
      </div>
    {/if}

    <div class="f vars-row"><span class="l"></span>
      <span class="c">
        <span class="vars">
          {#each VARIABLES as v (v)}
            <button type="button" class="var" onclick={() => insert(v)}><code>{v}</code></button>{' '}
          {/each}
        </span>
        <small>{t('cbForm.varsHint')}</small>
      </span>
    </div>

    <div class="fields"><span class="legend">{t('cbForm.opennek')}</span>
      <label class="f"><span class="l">{t('cbForm.name')}</span>
        <span class="c"><input bind:value={form.name} maxlength="64" /><small>{t('cbForm.nameHint')}</small></span>
      </label>
      <label class="f"><span class="l">{t('cbForm.devices')}</span>
        <span class="c"><input class="mono" bind:value={form.devices} placeholder={t('cbForm.devicesPlaceholder')} /><small>{t('cbForm.devicesHint')}</small></span>
      </label>
      <div class="f"><span class="l">{t('cbForm.enabled')}</span>
        <label class="c check"><input type="checkbox" bind:checked={form.enabled} /></label>
      </div>
    </div>

    <p class="error">{error}</p>
    <div class="actions">
      <button type="button" onclick={back}>{t('common.cancel')}</button>
      <button class="primary" disabled={saving}>{id === null ? t('cbForm.create') : t('common.save')}</button>
    </div>
  </form>

  <aside class="side">
    {#if form.callbackSubtype === 'BIDIR'}<div class="card note"><strong>BIDIR</strong><p>{t('cb.bidirHint')}</p></div>{/if}
    {#if !isSent(form)}<div class="card note warn"><strong>{t('cb.notSent')}</strong><p>{t('cb.notSentHint')}</p></div>{/if}
  </aside>
</div>
