<script lang="ts">
  // Free Practice: a persistent sandbox the learner is encouraged to break.

  import Terminal from '../components/Terminal.svelte';
  import EnvironmentPanel from '../components/EnvironmentPanel.svelte';
  import { api } from '../lib/ipc';
  import { app, describeError } from '../lib/appState.svelte';
  import { formatTimestamp } from '../lib/format';
  import type { SnapshotRow } from '../lib/types';

  let snapshots = $state<SnapshotRow[]>([]);
  let snapshotName = $state('');
  let busy = $state(false);
  let dangerousMode = $state(false);
  let showDangerousConfirm = $state(false);

  async function loadSnapshots(): Promise<void> {
    try {
      snapshots = await api.listSnapshots();
    } catch (error) {
      app.notify(describeError(error), 'error');
    }
  }

  async function start(): Promise<void> {
    busy = true;
    try {
      await app.startSession();
    } finally {
      busy = false;
    }
  }

  async function createSnapshot(): Promise<void> {
    const name = snapshotName.trim() || `snapshot-${snapshots.length + 1}`;
    busy = true;
    try {
      await api.createSnapshot(name);
      snapshotName = '';
      await loadSnapshots();
      app.notify(`Saved a snapshot called ${name}.`);
    } catch (error) {
      app.notify(describeError(error), 'error');
    } finally {
      busy = false;
    }
  }

  async function restore(id: number, name: string): Promise<void> {
    busy = true;
    try {
      await api.restoreSnapshot(id);
      app.notify(`Restored ${name}. Start Linux again to use it.`);
      await app.pollVm();
    } catch (error) {
      app.notify(describeError(error), 'error');
    } finally {
      busy = false;
    }
  }

  async function factoryReset(): Promise<void> {
    busy = true;
    try {
      await api.factoryResetPractice();
      await loadSnapshots();
      app.notify(
        'Free Practice will be rebuilt from the clean image next time you start it. A snapshot was kept first.',
      );
      await app.pollVm();
    } catch (error) {
      app.notify(describeError(error), 'error');
    } finally {
      busy = false;
    }
  }

  async function enableDangerousMode(): Promise<void> {
    // A recovery snapshot is taken automatically before anything destructive is allowed.
    busy = true;
    try {
      await api.createSnapshot('before-dangerous-mode');
      await loadSnapshots();
      dangerousMode = true;
      showDangerousConfirm = false;
      app.notify('Dangerous Mode is on. A recovery snapshot was created first.');
    } catch (error) {
      app.notify(describeError(error), 'error');
    } finally {
      busy = false;
    }
  }

  $effect(() => {
    if (snapshots.length === 0) void loadSnapshots();
  });
</script>

<header class="head">
  <div>
    <h1>Free Practice</h1>
    <p class="muted">
      A real Debian system that keeps your changes between sessions. Nothing here can reach your
      Windows files.
    </p>
  </div>
  {#if !app.vmReady}
    <button type="button" class="primary" disabled={busy} onclick={start}>Start Linux</button>
  {/if}
</header>

{#if app.vm?.state === 'unbootable'}
  <section class="broken" role="alert">
    <h2>The Linux practice environment is no longer bootable.</h2>
    <p>Windows was not affected. Pick how you would like to carry on.</p>
    <div class="actions">
      {#if snapshots.length > 0}
        {@const latest = snapshots[0]}
        {#if latest}
          <button type="button" disabled={busy} onclick={() => restore(latest[0], latest[1])}>
            Restore last snapshot
          </button>
        {/if}
      {/if}
      <button type="button" disabled={busy} onclick={factoryReset}>Create fresh environment</button>
      <button type="button" onclick={() => app.navigate({ name: 'help' })}>
        View what happened
      </button>
    </div>
  </section>
{/if}

<div class="workspace">
  <section class="terminal-area">
    <Terminal />
  </section>

  <aside>
    <h2>Snapshots</h2>
    <p class="muted">
      A snapshot is a copy of this environment you can come back to. Take one before you try
      something risky.
    </p>
    <div class="snapshot-form">
      <input
        type="text"
        placeholder="Snapshot name"
        bind:value={snapshotName}
        aria-label="Snapshot name"
      />
      <button type="button" disabled={busy} onclick={createSnapshot}>Save</button>
    </div>

    <ul class="snapshots">
      {#each snapshots as [id, name, , createdAt] (id)}
        <li>
          <div>
            <strong>{name}</strong>
            <span class="muted">{formatTimestamp(createdAt)}</span>
          </div>
          <button type="button" disabled={busy} onclick={() => restore(id, name)}>Restore</button>
        </li>
      {:else}
        <li class="muted">No snapshots yet.</li>
      {/each}
    </ul>

    <h2>Dangerous Mode</h2>
    {#if dangerousMode}
      <p class="danger-on">
        Dangerous Mode is on. Commands that destroy the system are permitted, and they only affect
        this disposable Linux environment.
      </p>
    {:else if showDangerousConfirm}
      <div class="confirm">
        <p>Dangerous Mode can make the Linux sandbox unusable.</p>
        <p>Windows files remain isolated.</p>
        <p>A recovery snapshot will be created automatically.</p>
        <div class="actions">
          <button type="button" disabled={busy} onclick={enableDangerousMode}>
            Turn it on
          </button>
          <button type="button" onclick={() => (showDangerousConfirm = false)}>Cancel</button>
        </div>
      </div>
    {:else}
      <p class="muted">
        Lets you delete system files, break the boot process, fill the disk and stop essential
        services, so you can see what happens and practise recovering.
      </p>
      <button type="button" onclick={() => (showDangerousConfirm = true)}>
        Turn on Dangerous Mode
      </button>
    {/if}

    <h2>Environment</h2>
    <EnvironmentPanel />

    <h2>Start over</h2>
    <button type="button" disabled={busy} onclick={factoryReset}>Factory reset Free Practice</button>
  </aside>
</div>

<style>
  .head {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 16px;
  }

  h1 {
    margin: 0;
    font-size: 1.4rem;
  }

  .workspace {
    display: grid;
    grid-template-columns: 1fr 320px;
    gap: 16px;
    flex: 1 1 auto;
    min-height: 420px;
  }

  .terminal-area {
    min-width: 0;
    min-height: 0;
    display: flex;
  }

  aside {
    overflow-y: auto;
    padding-left: 4px;
  }

  aside h2 {
    font-size: 0.78rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
    margin: 18px 0 6px;
  }

  aside h2:first-child {
    margin-top: 0;
  }

  .snapshot-form {
    display: flex;
    gap: 6px;
    margin-bottom: 8px;
  }

  .snapshot-form input {
    flex: 1 1 auto;
    min-width: 0;
  }

  .snapshots {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 0.85rem;
  }

  .snapshots li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .snapshots div {
    display: flex;
    flex-direction: column;
  }

  .broken {
    border: 1px solid var(--bad);
    border-radius: 6px;
    padding: 12px 14px;
  }

  .broken h2 {
    margin: 0 0 4px;
    font-size: 1rem;
  }

  .confirm {
    border: 1px solid var(--warn);
    border-radius: 6px;
    padding: 10px 12px;
    font-size: 0.85rem;
  }

  .confirm p {
    margin: 0 0 6px;
  }

  .danger-on {
    color: var(--warn);
    font-size: 0.85rem;
  }

  .actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    margin-top: 8px;
  }

  .muted {
    color: var(--muted);
    font-size: 0.85rem;
  }
</style>
