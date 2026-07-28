<script lang="ts">
  import { api } from '../lib/ipc';
  import { app, describeError } from '../lib/appState.svelte';
  import { formatDuration, formatKibibytes } from '../lib/format';
  import type { GuestDiagnostics } from '../lib/types';

  let diagnostics = $state<GuestDiagnostics | undefined>();
  let loading = $state(false);

  async function refresh(): Promise<void> {
    loading = true;
    try {
      diagnostics = await api.guestDiagnostics();
    } catch (error) {
      app.notify(describeError(error), 'error');
    } finally {
      loading = false;
    }
  }

  // Load once the machine is ready, and again whenever it becomes ready.
  $effect(() => {
    if (app.vmReady && !diagnostics && !loading) void refresh();
  });
</script>

<div class="header">
  <h3>Environment</h3>
  <button type="button" disabled={!app.vmReady || loading} onclick={refresh}>
    {loading ? 'Reading…' : 'Refresh'}
  </button>
</div>

{#if !app.vmReady}
  <p class="muted">Linux is not running, so there is nothing to report yet.</p>
{:else if !diagnostics}
  <p class="muted">Reading the environment…</p>
{:else}
  <dl>
    <div><dt>Host name</dt><dd>{diagnostics.hostname}</dd></div>
    <div><dt>Kernel</dt><dd>{diagnostics.kernel}</dd></div>
    <div><dt>Up for</dt><dd>{formatDuration(diagnostics.uptimeSeconds)}</dd></div>
    <div>
      <dt>Load average</dt>
      <dd>{diagnostics.loadAverage.map((value) => value.toFixed(2)).join(' · ')}</dd>
    </div>
    <div>
      <dt>Memory</dt>
      <dd>
        {formatKibibytes(diagnostics.memoryTotalKb - diagnostics.memoryAvailableKb)} used of
        {formatKibibytes(diagnostics.memoryTotalKb)}
      </dd>
    </div>
    <div>
      <dt>Root filesystem</dt>
      <!-- Bytes and inodes are shown separately because either can run out on its own, and
           the two failures look completely different from inside the guest. -->
      <dd>{diagnostics.rootDiskUsedPercent}% of space, {diagnostics.rootInodesUsedPercent}% of inodes</dd>
    </div>
    <div>
      <dt>Working directory</dt>
      <dd>{diagnostics.currentDirectory ?? 'unknown'}</dd>
    </div>
    <div>
      <dt>Listening ports</dt>
      <dd>{diagnostics.listeningPorts.length > 0 ? diagnostics.listeningPorts.join(', ') : 'none'}</dd>
    </div>
    <div>
      <dt>Failed services</dt>
      <dd class:bad={diagnostics.failedUnits.length > 0}>
        {diagnostics.failedUnits.length > 0 ? diagnostics.failedUnits.join(', ') : 'none'}
      </dd>
    </div>
  </dl>
{/if}

<style>
  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 8px;
  }

  h3 {
    margin: 0;
    font-size: 0.85rem;
  }

  dl {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
    gap: 4px 20px;
    margin: 0;
    font-size: 0.85rem;
  }

  dl div {
    display: flex;
    gap: 8px;
  }

  dt {
    color: var(--muted);
    min-width: 12ch;
  }

  dd {
    margin: 0;
  }

  .bad {
    color: var(--bad);
  }

  .muted {
    color: var(--muted);
    font-size: 0.85rem;
  }
</style>
