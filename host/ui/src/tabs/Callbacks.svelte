<script lang="ts">
  import { app } from '../lib/app.svelte'
  import CallbackForm from './callbacks/CallbackForm.svelte'
  import CallbackHistory from './callbacks/CallbackHistory.svelte'
  import CallbackList from './callbacks/CallbackList.svelte'
</script>

{#if app.callbackView.kind === 'edit'}
  {#key app.callbackView}<CallbackForm id={app.callbackView.id} copy={app.callbackView.copy} />{/key}
{:else if app.callbackView.kind === 'history'}
  {#key app.callbackView}<CallbackHistory id={app.callbackView.id} />{/key}
{:else}
  <CallbackList />
{/if}
