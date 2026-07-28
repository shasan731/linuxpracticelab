<script lang="ts">
  import { app } from '../lib/appState.svelte';
  import { masteryLabel, needsRevisiting } from '../lib/lessonFlow';
  import type { ProgressionMode } from '../lib/types';

  let filter = $state('');
  let showOptional = $state(false);

  const modules = $derived(
    (app.bootstrap?.modules ?? []).filter((module) => showOptional || module.pack === 'core'),
  );

  function matches(text: string): boolean {
    const needle = filter.trim().toLowerCase();
    return needle === '' || text.toLowerCase().includes(needle);
  }

  const modes: { value: ProgressionMode; label: string; hint: string }[] = [
    {
      value: 'guided-path',
      label: 'Guided path',
      hint: 'Lessons unlock in order once you pass the ones they build on.',
    },
    {
      value: 'open-library',
      label: 'Open library',
      hint: 'Everything is open. You are warned when a lesson assumes earlier work.',
    },
    {
      value: 'assessment',
      label: 'Assessment',
      hint: 'No hints and no worked examples, for checking what you actually know.',
    },
  ];
</script>

<header class="head">
  <div>
    <h1>Learn</h1>
    <p class="muted">
      {app.bootstrap?.coreLessonCount ?? 0} lessons in the core curriculum.
    </p>
  </div>
  <input type="search" placeholder="Filter lessons or commands" bind:value={filter} aria-label="Filter lessons" />
</header>

<fieldset class="modes">
  <legend>How lessons unlock</legend>
  {#each modes as mode (mode.value)}
    <label title={mode.hint}>
      <input
        type="radio"
        name="mode"
        value={mode.value}
        checked={app.mode === mode.value}
        onchange={() => app.setMode(mode.value)}
      />
      {mode.label}
    </label>
  {/each}
  <label class="optional">
    <input type="checkbox" bind:checked={showOptional} />
    Include optional course packs
  </label>
</fieldset>

{#each modules as module (module.id)}
  {@const visible = module.lessons.filter(
    (lesson) => matches(lesson.title) || lesson.commands.some(matches),
  )}
  {#if visible.length > 0}
    <section class="module">
      <header>
        <h2>
          <span class="number">Module {module.number}</span>
          {module.title}
        </h2>
        <span class="count">{module.completedLessons}/{module.lessons.length} complete</span>
      </header>
      <p class="summary">{module.summary}</p>

      <ul>
        {#each visible as lesson (lesson.id)}
          <li>
            <button
              type="button"
              class="lesson"
              class:locked={!lesson.unlocked}
              onclick={() => app.navigate({ name: 'lesson', lessonId: lesson.id })}
            >
              <span class="lesson-title">{lesson.title}</span>
              <span class="badges">
                <span class="type">{lesson.type.replace('-', ' ')}</span>
                {#if lesson.mastery}
                  <span class="mastery" class:weak={needsRevisiting(lesson.mastery)}>
                    {masteryLabel(lesson.mastery)}
                  </span>
                {/if}
                {#if !lesson.unlocked}
                  <span class="lock" title={`Needs ${lesson.missingPrerequisites.join(', ')}`}>
                    Locked
                  </span>
                {:else if lesson.missingPrerequisites.length > 0}
                  <span class="warn" title={`Builds on ${lesson.missingPrerequisites.join(', ')}`}>
                    Builds on earlier lessons
                  </span>
                {/if}
              </span>
            </button>
          </li>
        {/each}
      </ul>
    </section>
  {/if}
{/each}

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
    min-width: 260px;
  }

  .modes {
    display: flex;
    gap: 16px;
    align-items: center;
    flex-wrap: wrap;
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 8px 12px;
  }

  legend {
    font-size: 0.75rem;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    padding: 0 6px;
  }

  .modes label {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 0.85rem;
  }

  .optional {
    margin-left: auto;
    color: var(--muted);
  }

  .module header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
  }

  .module h2 {
    font-size: 1rem;
    margin: 0;
    display: flex;
    gap: 10px;
    align-items: baseline;
  }

  .number {
    color: var(--muted);
    font-size: 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .count,
  .summary {
    color: var(--muted);
    font-size: 0.8rem;
  }

  .summary {
    margin: 2px 0 8px;
    max-width: 90ch;
  }

  ul {
    list-style: none;
    margin: 0 0 20px;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .lesson {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    width: 100%;
    text-align: left;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 5px;
    padding: 7px 10px;
    font: inherit;
    color: inherit;
    cursor: pointer;
  }

  .lesson:hover {
    border-color: var(--accent);
  }

  /* Locked lessons stay clickable: the player explains what is missing and offers Open
     Library, which is better than a dead control. */
  .lesson.locked .lesson-title {
    color: var(--muted);
  }

  .badges {
    display: flex;
    gap: 6px;
    align-items: center;
    flex-shrink: 0;
  }

  .type,
  .mastery,
  .lock,
  .warn {
    font-size: 0.7rem;
    border-radius: 999px;
    padding: 1px 8px;
    border: 1px solid var(--border);
    color: var(--muted);
  }

  .mastery {
    border-color: var(--good);
    color: var(--good);
  }

  .mastery.weak {
    border-color: var(--warn);
    color: var(--warn);
  }

  .lock {
    border-color: var(--muted);
  }

  .warn {
    border-color: var(--warn);
    color: var(--warn);
  }

  .muted {
    color: var(--muted);
  }
</style>
