<script lang="ts">
  // Scenario Labs are lessons of type `debugging`, `scenario` or `capstone`. They are surfaced
  // separately because a learner looking for troubleshooting practice does not want to scroll
  // the whole curriculum to find it.

  import { app } from '../lib/appState.svelte';
  import { masteryLabel } from '../lib/lessonFlow';

  const scenarioTypes = new Set(['debugging', 'scenario', 'capstone', 'assessment']);

  const scenarios = $derived(
    (app.bootstrap?.modules ?? []).flatMap((module) =>
      module.lessons
        .filter((lesson) => scenarioTypes.has(lesson.type))
        .map((lesson) => ({ lesson, module })),
    ),
  );
</script>

<h1>Scenario Labs</h1>
<p class="muted">
  Broken systems to diagnose and repair. Each one starts from a known state and is checked on the
  state you leave it in, not on the commands you use to get there.
</p>

{#if scenarios.length === 0}
  <p class="muted">
    Scenario labs arrive with the later modules. Work through the curriculum first, or open Free
    Practice to experiment.
  </p>
{:else}
  <ul>
    {#each scenarios as { lesson, module } (lesson.id)}
      <li>
        <button type="button" onclick={() => app.navigate({ name: 'lesson', lessonId: lesson.id })}>
          <span class="title">{lesson.title}</span>
          <span class="meta">
            <span class="module">{module.title}</span>
            <span class="type">{lesson.type}</span>
            <span class="difficulty" aria-label={`Difficulty ${lesson.estimatedDifficulty} of 5`}>
              {'●'.repeat(lesson.estimatedDifficulty)}{'○'.repeat(5 - lesson.estimatedDifficulty)}
            </span>
            {#if lesson.mastery}
              <span class="mastery">{masteryLabel(lesson.mastery)}</span>
            {/if}
          </span>
        </button>
      </li>
    {/each}
  </ul>
{/if}

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
    grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
    gap: 8px;
  }

  button {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 100%;
    text-align: left;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 10px 12px;
    font: inherit;
    color: inherit;
    cursor: pointer;
  }

  button:hover {
    border-color: var(--accent);
  }

  .title {
    font-weight: 500;
  }

  .meta {
    display: flex;
    gap: 10px;
    align-items: center;
    flex-wrap: wrap;
    font-size: 0.75rem;
    color: var(--muted);
  }

  .difficulty {
    letter-spacing: 1px;
  }

  .mastery {
    color: var(--good);
  }

  .muted {
    color: var(--muted);
    max-width: 90ch;
  }
</style>
