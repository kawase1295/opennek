<script lang="ts">
  import { tick } from 'svelte'
  import { app } from '../lib/app.svelte'
  import { fmtClock, fmtTime } from '../lib/format'
  import { t } from '../lib/i18n'

  let box = $state<HTMLDivElement>()
  // 一番下にいる間は新しい行に付いていく (ターミナルと同じ)。上へスクロールしたら止まる
  let follows = true

  function onScroll(): void {
    if (box) follows = box.scrollHeight - box.scrollTop - box.clientHeight < 4
  }

  $effect(() => {
    void app.hostLog.length
    void tick().then(() => {
      if (box && follows) box.scrollTop = box.scrollHeight
    })
  })
</script>

{#if app.hostLog.length}
  <div class="host-log mono" bind:this={box} onscroll={onScroll}>
    {#each app.hostLog as l, i (i)}
      <div><time title={fmtTime(l.time)}>{fmtClock(l.time)}</time>{l.line}</div>
    {/each}
  </div>
{:else}
  <p class="empty">{t('hostLog.empty')}</p>
{/if}
