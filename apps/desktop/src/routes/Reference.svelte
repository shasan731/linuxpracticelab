<script lang="ts">
  // Searchable command reference, built from the curriculum rather than maintained separately,
  // so a command can never appear in a lesson without a reference entry or the reverse.

  import { api } from '../lib/ipc';
  import { app, describeError } from '../lib/appState.svelte';
  import type { CommandEntry } from '../lib/types';

  let entries = $state<CommandEntry[]>([]);
  let filter = $state('');
  let selected = $state<CommandEntry | undefined>();

  const visible = $derived(
    entries.filter((entry) => entry.command.includes(filter.trim().toLowerCase())),
  );

  $effect(() => {
    if (entries.length === 0) {
      void (async () => {
        try {
          entries = await api.commandReference();
        } catch (error) {
          app.notify(describeError(error), 'error');
        }
      })();
    }
  });
</script>

<header class="head">
  <div>
    <h1>Command reference</h1>
    <p class="muted">
      {entries.length} commands appear in the curriculum. Every entry links to the lessons that
      use it.
    </p>
  </div>
  <input type="search" placeholder="Search commands" bind:value={filter} aria-label="Search commands" />
</header>

<div class="columns">
  <ul class="list">
    {#each visible as entry (entry.command)}
      <li>
        <button
          type="button"
          class:active={selected?.command === entry.command}
          onclick={() => (selected = entry)}
        >
          <code>{entry.command}</code>
          <span class="muted">{entry.lessons.length}</span>
        </button>
      </li>
    {:else}
      <li class="muted">Nothing matches that search.</li>
    {/each}
  </ul>

  <section class="detail">
    {#if !selected}
      <p class="muted">Pick a command to see where it is taught.</p>
    {:else}
      <h2><code>{selected.command}</code></h2>
      <p class="muted">
        Practise this command in Free Practice, or open one of the lessons below. Reading
        <code>{selected.command} --help</code> and <code>man {selected.command}</code> inside the
        terminal is part of the curriculum in module 1.
      </p>

      <h3>Lessons using it</h3>
      <ul class="lessons">
        {#each selected.lessons as [lessonId, title] (lessonId)}
          <li>
            <button type="button" class="link" onclick={() => app.navigate({ name: 'lesson', lessonId })}>
              {title}
            </button>
          </li>
        {/each}
      </ul>

      <h3>Try it</h3>
      <p class="muted">
        Free Practice gives you a real shell with no lesson checks attached. Nothing you type there
        can affect Windows.
      </p>
      <button type="button" onclick={() => app.navigate({ name: 'practice' })}>
        Open Free Practice
      </button>
    {/if}
  </section>
</div>

<style>
  .head {
    display: flex;
    justify-content: space-between;
    align-items: flex-end;
    gap: 16px;
  }

  h1 {
    margin: 0;
    font-size: 1.4rem;
  }

  input[type='search'] {
    min-width: 240px;
  }

  .columns {
    display: grid;
    grid-template-columns: 260px 1fr;
    gap: 20px;
    flex: 1 1 auto;
    min-height: 0;
  }

  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    overflow-y: auto;
    border-right: 1px solid var(--border);
    padding-right: 10px;
  }

  .list button {
    display: flex;
    justify-content: space-between;
    width: 100%;
    background: none;
    border: none;
    border-radius: 4px;
    padding: 4px 8px;
    font: inherit;
    color: inherit;
    cursor: pointer;
    text-align: left;
  }

  .list button:hover {
    background: var(--surface-2);
  }

  .list button.active {
    background: var(--accent-soft);
    color: var(--accent);
  }

  .detail {
    overflow-y: auto;
    max-width: 80ch;
  }

  .detail h2 {
    margin-top: 0;
  }

  .detail h3 {
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
    margin: 18px 0 6px;
  }

  .lessons {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .link {
    background: none;
    border: none;
    color: var(--accent);
    padding: 0;
    font: inherit;
    cursor: pointer;
    text-align: left;
  }

  .muted {
    color: var(--muted);
    font-size: 0.85rem;
  }
</style>
