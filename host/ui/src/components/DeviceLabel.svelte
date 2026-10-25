<script lang="ts">
  import { app } from '../lib/app.svelte'

  let { id }: { id: string } = $props()
  const name = $derived(app.deviceName(id))
</script>

{id}
{#if name}<span class="muted">{name}</span>{/if}
