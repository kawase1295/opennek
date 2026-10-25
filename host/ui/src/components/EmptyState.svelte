<script lang="ts">
  let { title, body, boxed = false }: { title: string; body?: string; boxed?: boolean } = $props()
</script>

<div class="empty-state" class:boxed>
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden="true">
    <circle cx="12" cy="12" r="2.5" />
    <path d="M6.3 6.3a8 8 0 0 0 0 11.4M17.7 6.3a8 8 0 0 1 0 11.4M3.5 3.5a12 12 0 0 0 0 17M20.5 3.5a12 12 0 0 1 0 17" />
  </svg>
  <h4>{title}</h4>
  {#if body}<p>{body}</p>{/if}
</div>
