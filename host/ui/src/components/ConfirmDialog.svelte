<script lang="ts">
  import { answer, pending } from '../lib/confirm.svelte'
  import { t } from '../lib/i18n'

  let dialog: HTMLDialogElement
  const q = $derived(pending.question)

  $effect(() => {
    if (q && !dialog.open) dialog.showModal()
    if (!q && dialog.open) dialog.close()
  })
</script>

<!-- Esc や背景で閉じたときは「いいえ」 -->
<dialog bind:this={dialog} class="confirm" onclose={() => pending.question && answer(false)}>
  {#if q}
    <div class="body">
      <h3>{q.title}</h3>
      {#if q.body}<p>{q.body}</p>{/if}
    </div>
    <div class="actions">
      <button onclick={() => answer(false)}>{t('common.cancel')}</button>
      <button class="primary" class:danger={q.danger} onclick={() => answer(true)}>{q.confirm}</button>
    </div>
  {/if}
</dialog>
