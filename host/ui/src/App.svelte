<script lang="ts">
  import { onMount } from 'svelte'
  import { app } from './lib/app.svelte'
  import { toast } from './lib/toast.svelte'
  import ConfirmDialog from './components/ConfirmDialog.svelte'
  import Header from './components/Header.svelte'
  import Callbacks from './tabs/Callbacks.svelte'
  import Devices from './tabs/Devices.svelte'
  import HostLog from './tabs/HostLog.svelte'
  import Messages from './tabs/Messages.svelte'
  import Settings from './tabs/Settings.svelte'

  onMount(() => void app.start())
</script>

<Header />
<main>
  {#if app.tab === 'messages'}<Messages />
  {:else if app.tab === 'devices'}<Devices />
  {:else if app.tab === 'callbacks'}<Callbacks />
  {:else if app.tab === 'log'}<HostLog />
  {:else}<Settings />{/if}
</main>
<div id="toast" class:show={toast.shown} role="status">{toast.text}</div>
<ConfirmDialog />
