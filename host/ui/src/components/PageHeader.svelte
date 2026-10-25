<script lang="ts">
  import type { Snippet } from 'svelte'

  let {
    title,
    count,
    intro,
    actions,
  }: { title: string; count?: number | string; intro?: string; actions?: Snippet } = $props()
</script>

<div class="page-head">
  <div class="title">
    <h2>{title}{#if count !== undefined}<span class="count">{count}</span>{/if}</h2>
    {#if intro}<p>{intro}</p>{/if}
  </div>
  {#if actions}<div class="toolbar">{@render actions()}</div>{/if}
</div>
