<script lang="ts">
  import type { Badge } from '../lib/badges'

  let { badge }: { badge: Badge | null } = $props()
</script>

{#if badge}<span class="badge {badge.kind}">{badge.label}</span>{/if}
