<script lang="ts">
  import { app } from '../lib/appState.svelte';
  import { percent } from '../lib/format';

  const bootstrap = $derived(app.bootstrap);
  const nextLesson = $derived(
    bootstrap?.nextLessonId ? app.findLesson(bootstrap.nextLessonId) : undefined,
  );
  const currentPath = $derived(
    bootstrap?.modules.find((module) =>
      module.lessons.some((lesson) => lesson.id === bootstrap?.nextLessonId),
    ),
  );
</script>

{#if bootstrap}
  <section class="welcome">
    <h1>Welcome back</h1>
    <p class="lead">
      {#if currentPath}
        You are working through <strong>{currentPath.title}</strong>.
      {:else}
        Everything in the core curriculum is finished. Well done.
      {/if}
    </p>

    <div class="metrics">
      <div class="metric">
        <span class="value">{bootstrap.completedCoreLessons}</span>
        <span class="label">of {bootstrap.coreLessonCount} core lessons</span>
        <div class="bar">
          <span
            style={`width:${percent(bootstrap.completedCoreLessons, bootstrap.coreLessonCount)}%`}
          ></span>
        </div>
      </div>
      <div class="metric">
        <span class="value">{bootstrap.masteryPercent}%</span>
        <span class="label">average mastery</span>
      </div>
      <div class="metric">
        <span class="value">{bootstrap.reviewLessonIds.length}</span>
        <span class="label">lessons worth revisiting</span>
      </div>
    </div>

    <div class="actions">
      {#if bootstrap.nextLessonId}
        <button
          type="button"
          class="primary"
          onclick={() =>
            app.navigate({ name: 'lesson', lessonId: bootstrap.nextLessonId as string })}
        >
          Continue: {nextLesson?.title ?? 'next lesson'}
        </button>
      {/if}
      <button type="button" onclick={() => app.navigate({ name: 'practice' })}>
        Open Free Practice
      </button>
      {#if bootstrap.reviewLessonIds.length > 0}
        <button
          type="button"
          onclick={() =>
            app.navigate({ name: 'lesson', lessonId: bootstrap.reviewLessonIds[0] as string })}
        >
          Review weak commands
        </button>
      {/if}
    </div>
  </section>

  {#if bootstrap.reviewLessonIds.length > 0}
    <section>
      <h2>Recommended review</h2>
      <ul class="review">
        {#each bootstrap.reviewLessonIds as lessonId (lessonId)}
          <li>
            <button type="button" class="link" onclick={() => app.navigate({ name: 'lesson', lessonId })}>
              {app.findLesson(lessonId)?.title ?? lessonId}
            </button>
            <span class="muted">{app.findLesson(lessonId)?.module ?? ''}</span>
          </li>
        {/each}
      </ul>
    </section>
  {/if}

  <section>
    <h2>Course map</h2>
    <ol class="map">
      {#each bootstrap.modules.filter((module) => module.pack === 'core') as module (module.id)}
        <li>
          <button type="button" class="module" onclick={() => app.navigate({ name: 'learn' })}>
            <span class="number">{module.number}</span>
            <span class="title">{module.title}</span>
            <span class="count">
              {module.completedLessons}/{module.lessons.length}
            </span>
          </button>
        </li>
      {/each}
    </ol>
  </section>
{/if}

<style>
  .welcome h1 {
    margin: 0;
    font-size: 1.5rem;
  }

  .lead {
    color: var(--muted);
    margin: 4px 0 18px;
  }

  .metrics {
    display: flex;
    gap: 28px;
    flex-wrap: wrap;
    margin-bottom: 18px;
  }

  .metric {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 180px;
  }

  .value {
    font-size: 1.6rem;
    font-weight: 600;
  }

  .label {
    color: var(--muted);
    font-size: 0.8rem;
  }

  .bar {
    height: 4px;
    background: var(--surface-3);
    border-radius: 2px;
    overflow: hidden;
    margin-top: 6px;
  }

  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
  }

  .actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }

  h2 {
    font-size: 0.85rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
  }

  .review {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .review li {
    display: flex;
    gap: 10px;
    align-items: baseline;
    font-size: 0.9rem;
  }

  .map {
    list-style: none;
    padding: 0;
    margin: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
    gap: 6px;
  }

  .module {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    text-align: left;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 8px 10px;
    font: inherit;
    color: inherit;
    cursor: pointer;
  }

  .module:hover {
    border-color: var(--accent);
  }

  .number {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    min-width: 1.5ch;
  }

  .title {
    flex: 1 1 auto;
  }

  .count {
    color: var(--muted);
    font-size: 0.8rem;
  }

  .link {
    background: none;
    border: none;
    color: var(--accent);
    padding: 0;
    font: inherit;
    cursor: pointer;
  }

  .muted {
    color: var(--muted);
    font-size: 0.8rem;
  }
</style>
