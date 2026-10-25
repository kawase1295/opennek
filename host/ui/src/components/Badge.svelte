<script lang="ts">
  import type { Badge } from '../lib/badges'

  let { badge }: { badge: Badge | null } = $props()
</script>

{#if badge}<span class="badge {badge.tone} {badge.look}">{badge.label}</span>{/if}
