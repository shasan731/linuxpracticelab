<script lang="ts">
  import { api } from '../lib/ipc';
  import { app, describeError } from '../lib/appState.svelte';
  import { formatTimestamp } from '../lib/format';
  import type { ProgressReport } from '../lib/types';

  let report = $state<ProgressReport | undefined>();

  // Achievement definitions live here rather than in the database, so adding one does not need
  // a migration and an old profile picks up new achievements retroactively.
  const definitions: { id: string; title: string; description: string }[] = [
    { id: 'first-lesson', title: 'First steps', description: 'Pass your first lesson.' },
    { id: 'module-0', title: 'Oriented', description: 'Finish the orientation module.' },
    { id: 'ten-mastered', title: 'Ten mastered', description: 'Master ten lessons with no hints.' },
    { id: 'pipeline-builder', title: 'Pipeline builder', description: 'Finish the pipelines module.' },
    { id: 'permission-fixer', title: 'Permission fixer', description: 'Repair a permissions incident.' },
    { id: 'service-restorer', title: 'Service restorer', description: 'Bring a failed service back.' },
    { id: 'survivor', title: 'Survivor', description: 'Destroy the environment and restore it.' },
    { id: 'capstone', title: 'Administrator', description: 'Complete the final capstone.' },
  ];

  const unlocked = $derived(new Map(report?.achievements ?? []));

  $effect(() => {
    if (!report) {
      void (async () => {
        try {
          report = await api.progressReport();
        } catch (error) {
          app.notify(describeError(error), 'error');
        }
      })();
    }
  });
</script>

<h1>Achievements</h1>
<p class="muted">Milestones recorded on this computer. They are a record, not a requirement.</p>

<ul>
  {#each definitions as definition (definition.id)}
    {@const at = unlocked.get(definition.id)}
    <li class:unlocked={at !== undefined}>
      <span class="mark" aria-hidden="true">{at !== undefined ? '★' : '☆'}</span>
      <span class="body">
        <strong>{definition.title}</strong>
        <span class="muted">{definition.description}</span>
        {#if at !== undefined}
          <span class="when">Unlocked {formatTimestamp(at)}</span>
        {/if}
      </span>
    </li>
  {/each}
</ul>

<style>
  h1 {
    margin: 0;
    font-size: 1.4rem;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
    gap: 8px;
  }

  li {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 10px 12px;
    opacity: 0.65;
  }

  li.unlocked {
    opacity: 1;
    border-color: var(--good);
  }

  .mark {
    color: var(--warn);
    font-size: 1.1rem;
  }

  .body {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .when {
    font-size: 0.72rem;
    color: var(--good);
  }

  .muted {
    color: var(--muted);
    font-size: 0.82rem;
  }
</style>
