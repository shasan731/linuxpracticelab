<script lang="ts">
  // Browses the guest filesystem. The host restricts this to a few roots, so a path outside
  // them comes back as a refusal rather than a listing.

  import { api } from '../lib/ipc';
  import { app, describeError } from '../lib/appState.svelte';
  import { formatBytes } from '../lib/format';
  import type { DirEntryInfo } from '../lib/types';

  let path = $state('/home/student');
  let entries = $state<DirEntryInfo[]>([]);
  let includeHidden = $state(false);
  let error = $state<string | undefined>();
  let loading = $state(false);

  async function load(target: string): Promise<void> {
    loading = true;
    error = undefined;
    try {
      entries = await api.listDirectory(target, includeHidden);
      path = target;
    } catch (caught) {
      error = describeError(caught);
      entries = [];
    } finally {
      loading = false;
    }
  }

  function parentOf(current: string): string {
    const trimmed = current.replace(/\/+$/, '');
    const index = trimmed.lastIndexOf('/');
    return index <= 0 ? '/' : trimmed.slice(0, index);
  }

  function joinPath(base: string, name: string): string {
    return base.endsWith('/') ? `${base}${name}` : `${base}/${name}`;
  }

  $effect(() => {
    if (app.vmReady && entries.length === 0 && !loading && !error) void load(path);
  });
</script>

<div class="header">
  <h3>File tree</h3>
  <code class="path">{path}</code>
  <label>
    <input type="checkbox" bind:checked={includeHidden} onchange={() => load(path)} />
    Show hidden
  </label>
  <button type="button" disabled={!app.vmReady || loading} onclick={() => load(parentOf(path))}>
    Up
  </button>
  <button type="button" disabled={!app.vmReady || loading} onclick={() => load(path)}>
    Refresh
  </button>
</div>

{#if !app.vmReady}
  <p class="muted">Linux is not running yet.</p>
{:else if error}
  <p class="error">{error}</p>
{:else if entries.length === 0}
  <p class="muted">{loading ? 'Reading…' : 'This directory is empty.'}</p>
{:else}
  <table>
    <thead>
      <tr>
        <th scope="col">Name</th>
        <th scope="col">Mode</th>
        <th scope="col">Owner</th>
        <th scope="col">Size</th>
      </tr>
    </thead>
    <tbody>
      {#each entries as entry (entry.name)}
        <tr>
          <td>
            {#if entry.fileType === 'directory'}
              <button type="button" class="link" onclick={() => load(joinPath(path, entry.name))}>
                {entry.name}/
              </button>
            {:else}
              <span class:symlink={entry.fileType === 'symlink'}>
                {entry.name}{#if entry.linkTarget}<span class="muted"> → {entry.linkTarget}</span>{/if}
              </span>
            {/if}
          </td>
          <td><code>{entry.mode}</code></td>
          <td>{entry.owner}:{entry.group}</td>
          <td class="numeric">
            {entry.fileType === 'directory' ? '—' : formatBytes(entry.size)}
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
{/if}

<style>
  .header {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    margin-bottom: 8px;
  }

  h3 {
    margin: 0;
    font-size: 0.85rem;
  }

  .path {
    color: var(--muted);
    font-size: 0.8rem;
  }

  label {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 0.8rem;
    color: var(--muted);
  }

  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.82rem;
  }

  th {
    text-align: left;
    color: var(--muted);
    font-weight: 500;
    border-bottom: 1px solid var(--border);
    padding: 2px 6px;
  }

  td {
    padding: 2px 6px;
    border-bottom: 1px solid var(--surface-2);
  }

  .numeric {
    text-align: right;
  }

  .symlink {
    color: var(--accent);
  }

  .link {
    background: none;
    border: none;
    color: var(--accent);
    padding: 0;
    font: inherit;
    cursor: pointer;
  }

  .error {
    color: var(--bad);
    font-size: 0.85rem;
  }

  .muted {
    color: var(--muted);
    font-size: 0.85rem;
  }
</style>
