<script lang="ts">
  // The lesson player: instructions on the left, a real terminal on the right, and panels
  // underneath. The layout follows spec 8.3, and the split is resizable because a learner
  // reading a long explanation and a learner debugging a service want very different splits.

  import Terminal from '../components/Terminal.svelte';
  import EnvironmentPanel from '../components/EnvironmentPanel.svelte';
  import FileTreePanel from '../components/FileTreePanel.svelte';
  import ValidationPanel from '../components/ValidationPanel.svelte';
  import { api } from '../lib/ipc';
  import { app, describeError, loadLesson } from '../lib/appState.svelte';
  import {
    canCheck,
    completedCount,
    currentTask,
    initialProgress,
    isLessonComplete,
    masteryLabel,
    mistakePrompt,
    solutionIsNext,
    taskPosition,
    type TaskProgressMap,
  } from '../lib/lessonFlow';
  import type { CheckResult, LessonView } from '../lib/types';

  interface Props {
    lessonId: string;
  }
  let { lessonId }: Props = $props();

  let lesson = $state<LessonView | undefined>();
  let progress = $state<TaskProgressMap>({});
  let result = $state<CheckResult | undefined>();
  let busy = $state(false);
  let starting = $state(false);
  let instructionsHidden = $state(false);
  let splitPercent = $state(46);
  let activePanel = $state<'environment' | 'files' | 'validation' | 'notes' | 'history'>(
    'validation',
  );
  let notes = $state('');

  const task = $derived(lesson ? currentTask(lesson, progress) : undefined);
  const taskProgress = $derived(task ? progress[task.id] : undefined);
  const complete = $derived(lesson ? isLessonComplete(lesson, progress) : false);
  const position = $derived(lesson && task ? taskPosition(lesson, task.id) : { index: 0, total: 0 });

  // Reload whenever the route points at a different lesson.
  $effect(() => {
    const id = lessonId;
    void (async () => {
      const loaded = await loadLesson(id);
      if (!loaded) return;
      lesson = loaded;
      progress = initialProgress(loaded);
      result = undefined;
      await ensureRunning(loaded);
    })();
  });

  async function ensureRunning(target: LessonView): Promise<void> {
    if (app.vm?.state === 'ready') {
      await preparePlayground(target.id);
      return;
    }
    starting = true;
    try {
      const started = await app.startSession(target.id);
      if (started) await preparePlayground(target.id);
    } finally {
      starting = false;
    }
  }

  async function preparePlayground(id: string): Promise<void> {
    try {
      const warnings = await api.prepareLesson(id);
      for (const warning of warnings) app.notify(warning, 'info');
    } catch (error) {
      app.notify(describeError(error), 'error');
    }
  }

  async function check(): Promise<void> {
    if (!lesson || !task) return;
    busy = true;
    try {
      const outcome = await api.checkTask(lesson.id, task.id);
      result = outcome;
      activePanel = 'validation';
      if (outcome.validation.passed) {
        progress[task.id] = { ...(progress[task.id] ?? { hints: [], hintsRevealed: 0 }), passed: true, attempted: true, hints: progress[task.id]?.hints ?? [], hintsRevealed: progress[task.id]?.hintsRevealed ?? 0 };
        if (outcome.lessonComplete) await app.refresh();
      } else {
        const existing = progress[task.id];
        progress[task.id] = {
          passed: false,
          attempted: true,
          hints: existing?.hints ?? [],
          hintsRevealed: existing?.hintsRevealed ?? 0,
          ...(existing?.solution ? { solution: existing.solution } : {}),
        };
      }
    } catch (error) {
      app.notify(describeError(error), 'error');
    } finally {
      busy = false;
    }
  }

  async function hint(): Promise<void> {
    if (!lesson || !task) return;
    try {
      const response = await api.revealHint(lesson.id, task.id);
      const existing = progress[task.id] ?? {
        passed: false,
        attempted: false,
        hints: [],
        hintsRevealed: 0,
      };
      if (response.kind === 'hint') {
        progress[task.id] = {
          ...existing,
          hints: [...existing.hints, response.text],
          hintsRevealed: existing.hintsRevealed + 1,
        };
      } else if (response.kind === 'solutionAvailable') {
        app.notify('You have seen every hint. Use Show solution if you are still stuck.');
      } else {
        app.notify(response.reason, 'info');
      }
    } catch (error) {
      app.notify(describeError(error), 'error');
    }
  }

  async function showSolution(): Promise<void> {
    if (!lesson || !task) return;
    try {
      const response = await api.revealSolution(lesson.id, task.id);
      if (response.reason) {
        app.notify(response.reason, 'info');
        return;
      }
      const existing = progress[task.id] ?? {
        passed: false,
        attempted: false,
        hints: [],
        hintsRevealed: 0,
      };
      progress[task.id] = { ...existing, solution: response.solution };
    } catch (error) {
      app.notify(describeError(error), 'error');
    }
  }

  async function resetTask(): Promise<void> {
    if (!lesson) return;
    busy = true;
    try {
      await api.resetLesson(lesson.id);
      result = undefined;
      app.notify('The lesson environment has been put back the way it started.');
    } catch (error) {
      app.notify(describeError(error), 'error');
    } finally {
      busy = false;
    }
  }

  async function restart(): Promise<void> {
    if (!lesson) return;
    busy = true;
    try {
      await api.restartLesson(lesson.id);
      progress = initialProgress(lesson);
      result = undefined;
      await app.pollVm();
    } catch (error) {
      app.notify(describeError(error), 'error');
    } finally {
      busy = false;
    }
  }

  function startDrag(event: PointerEvent): void {
    const container = (event.currentTarget as HTMLElement).parentElement;
    if (!container) return;
    const bounds = container.getBoundingClientRect();
    const move = (moveEvent: PointerEvent) => {
      const ratio = ((moveEvent.clientX - bounds.left) / bounds.width) * 100;
      // Clamped so neither panel can be dragged away entirely.
      splitPercent = Math.min(72, Math.max(24, ratio));
    };
    const stop = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', stop);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', stop);
  }
</script>

{#if !lesson}
  <p class="muted">Loading lesson…</p>
{:else}
  <header class="lesson-header">
    <div>
      <p class="breadcrumb">
        <button type="button" class="link" onclick={() => app.navigate({ name: 'learn' })}>
          Learn
        </button>
        <span aria-hidden="true">›</span>
        {app.findLesson(lesson.id)?.module ?? lesson.module}
        <span aria-hidden="true">›</span>
        {lesson.title}
      </p>
      <h1>{lesson.title}</h1>
    </div>
    <div class="header-right">
      {#if lesson.tasks.length > 0}
        <span class="progress-pill">
          Task {position.index} of {position.total}
          · {completedCount(lesson, progress)} done
        </span>
      {/if}
      <button type="button" onclick={() => (instructionsHidden = !instructionsHidden)}>
        {instructionsHidden ? 'Show instructions' : 'Hide instructions'}
      </button>
    </div>
  </header>

  <div class="split" style={`--split:${instructionsHidden ? 0 : splitPercent}%`}>
    {#if !instructionsHidden}
      <section class="instructions" aria-label="Lesson instructions">
        <h2>Objective</h2>
        <p>{lesson.purpose}</p>

        <h2>What Linux is doing</h2>
        <p>{lesson.mentalModel}</p>

        {#if lesson.syntax.length > 0}
          <h2>Syntax</h2>
          {#each lesson.syntax as line (line)}
            <pre>{line}</pre>
          {/each}
        {/if}

        {#if lesson.demonstration.length > 0}
          <h2>Example</h2>
          {#each lesson.demonstration as demo (demo.command)}
            <pre>{demo.command}</pre>
            {#if demo.explanation}<p class="muted">{demo.explanation}</p>{/if}
            {#if demo.output}<pre class="output">{demo.output}</pre>{/if}
          {/each}
        {/if}

        {#if task}
          <h2>Task</h2>
          {#if task.context}<p class="context">{task.context}</p>{/if}
          {#if mistakePrompt(task)}
            <pre class="broken">{mistakePrompt(task)}</pre>
          {/if}
          <p class="instruction">{task.instruction}</p>

          {#if task.requirements.length > 0}
            <h3>This is complete when</h3>
            <ul class="requirements">
              {#each task.requirements as requirement (requirement)}
                <li>{requirement}</li>
              {/each}
            </ul>
          {/if}

          {#if taskProgress && taskProgress.hints.length > 0}
            <h3>Hints</h3>
            <ol class="hints">
              {#each taskProgress.hints as text, index (index)}
                <li>{text}</li>
              {/each}
            </ol>
          {/if}

          {#if taskProgress?.solution}
            <div class="solution">
              <h3>Worked solution</h3>
              <pre>{taskProgress.solution}</pre>
              <p class="muted">Type it yourself in the terminal to continue.</p>
            </div>
          {/if}
        {:else if complete}
          <div class="finished">
            <h2>Lesson complete</h2>
            {#if result?.mastery}
              <p>Result: <strong>{masteryLabel(result.mastery)}</strong></p>
            {/if}
            {#if lesson.summary}
              {#if lesson.summary.remember && lesson.summary.remember.length > 0}
                <h3>Remember</h3>
                <ul>
                  {#each lesson.summary.remember as item (item)}<li>{item}</li>{/each}
                </ul>
              {/if}
              {#if lesson.summary.commonOptions && lesson.summary.commonOptions.length > 0}
                <h3>Common options</h3>
                <dl>
                  {#each lesson.summary.commonOptions as option (option.option)}
                    <dt><code>{option.option}</code></dt>
                    <dd>{option.meaning}</dd>
                  {/each}
                </dl>
              {/if}
              {#if lesson.summary.dangerous && lesson.summary.dangerous.length > 0}
                <h3>Be careful with</h3>
                <ul class="danger">
                  {#each lesson.summary.dangerous as item (item)}<li>{item}</li>{/each}
                </ul>
              {/if}
            {/if}
            <button type="button" onclick={() => app.navigate({ name: 'learn' })}>
              Back to the curriculum
            </button>
          </div>
        {/if}

        <div class="actions">
          {#if lesson.hintsAvailable && task}
            {#if solutionIsNext(task, taskProgress)}
              <button type="button" onclick={showSolution}>Show solution</button>
            {:else}
              <button type="button" onclick={hint}>Hint</button>
            {/if}
          {/if}
          <button
            type="button"
            class="primary"
            disabled={!canCheck({ vmReady: app.vmReady, task, progress: taskProgress, busy })}
            onclick={check}
          >
            {busy ? 'Checking…' : 'Check'}
          </button>
          <button type="button" disabled={busy || !app.vmReady} onclick={resetTask}>
            Reset task
          </button>
          <button type="button" disabled={busy} onclick={restart}>Restart lesson</button>
        </div>
      </section>

      <!-- A button rather than a div with a tabindex: it is focusable and operable from the
           keyboard for free, and the arrow keys give the same control as dragging. -->
      <button
        type="button"
        class="gutter"
        aria-label={`Resize the instruction panel, currently ${Math.round(splitPercent)} percent`}
        onpointerdown={startDrag}
        onkeydown={(event) => {
          if (event.key === 'ArrowLeft') splitPercent = Math.max(24, splitPercent - 2);
          if (event.key === 'ArrowRight') splitPercent = Math.min(72, splitPercent + 2);
        }}
      ></button>
    {/if}

    <section class="terminal-area" aria-label="Linux terminal">
      {#if starting}
        <p class="muted booting">Starting Linux…</p>
      {/if}
      <!-- The toolbar is hidden here: in a lesson the screen belongs to the lesson, and the
           full set of terminal controls lives in Free Practice. -->
      <Terminal showToolbar={false} />
    </section>
  </div>

  <nav class="panel-tabs" aria-label="Panels">
    {#each ['validation', 'environment', 'files', 'notes', 'history'] as const as name (name)}
      <button type="button" class:active={activePanel === name} onclick={() => (activePanel = name)}>
        {name === 'files' ? 'File tree' : name.charAt(0).toUpperCase() + name.slice(1)}
      </button>
    {/each}
  </nav>

  <section class="panel">
    {#if activePanel === 'validation'}
      <ValidationPanel {result} />
    {:else if activePanel === 'environment'}
      <EnvironmentPanel />
    {:else if activePanel === 'files'}
      <FileTreePanel />
    {:else if activePanel === 'notes'}
      <label class="notes-label" for="lesson-notes">
        Your notes for this lesson. These stay on this computer and are not checked.
      </label>
      <textarea id="lesson-notes" bind:value={notes} rows="6"></textarea>
    {:else}
      <ul class="history">
        {#each app.bootstrap?.recentCommands ?? [] as command, index (index)}
          <li><code>{command}</code></li>
        {:else}
          <li class="muted">
            Command history is off by default. Turn it on in Settings if you want it kept.
          </li>
        {/each}
      </ul>
    {/if}
  </section>
{/if}

<style>
  .lesson-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-end;
    gap: 16px;
  }

  .lesson-header h1 {
    margin: 4px 0 0;
    font-size: 1.35rem;
  }

  .breadcrumb {
    margin: 0;
    color: var(--muted);
    font-size: 0.8rem;
    display: flex;
    gap: 6px;
    align-items: center;
  }

  .header-right {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .progress-pill {
    font-size: 0.8rem;
    color: var(--muted);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 2px 10px;
  }

  .split {
    display: grid;
    grid-template-columns: var(--split) auto 1fr;
    gap: 0;
    flex: 1 1 auto;
    min-height: 380px;
  }

  .instructions {
    overflow-y: auto;
    padding-right: 16px;
    max-width: 78ch;
  }

  .instructions h2 {
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
    margin: 18px 0 6px;
  }

  .instructions h3 {
    font-size: 0.85rem;
    margin: 14px 0 4px;
  }

  .instruction {
    font-weight: 500;
  }

  .context {
    color: var(--muted);
  }

  pre {
    background: var(--surface-3);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 8px 10px;
    overflow-x: auto;
    font-size: 0.85rem;
    margin: 6px 0;
  }

  pre.output {
    background: var(--surface-2);
    color: var(--muted);
  }

  pre.broken {
    border-color: var(--warn);
  }

  .requirements,
  .hints {
    padding-left: 20px;
    font-size: 0.9rem;
    line-height: 1.6;
  }

  .solution {
    border: 1px solid var(--accent);
    border-radius: 6px;
    padding: 10px 12px;
    margin-top: 12px;
  }

  .danger {
    color: var(--warn);
  }

  .actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    margin: 20px 0 8px;
    position: sticky;
    bottom: 0;
    background: var(--surface-1);
    padding-top: 10px;
  }

  .gutter {
    width: 8px;
    padding: 0;
    cursor: col-resize;
    background: transparent;
    border: none;
    border-left: 1px solid var(--border);
    border-radius: 0;
  }

  .gutter:hover {
    border-left-color: var(--accent);
  }

  .gutter:focus-visible {
    outline: 2px solid var(--accent);
  }

  .terminal-area {
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding-left: 12px;
  }

  .booting {
    margin: 0 0 6px;
  }

  .panel-tabs {
    display: flex;
    gap: 4px;
    border-bottom: 1px solid var(--border);
  }

  .panel-tabs button {
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--muted);
    padding: 6px 10px;
    font: inherit;
    cursor: pointer;
  }

  .panel-tabs button.active {
    color: var(--text);
    border-bottom-color: var(--accent);
  }

  .panel {
    min-height: 140px;
    max-height: 260px;
    overflow-y: auto;
  }

  .notes-label {
    display: block;
    color: var(--muted);
    font-size: 0.8rem;
    margin-bottom: 6px;
  }

  textarea {
    width: 100%;
    font: inherit;
    background: var(--surface-2);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 8px;
  }

  .history {
    list-style: none;
    margin: 0;
    padding: 0;
    font-size: 0.85rem;
    line-height: 1.7;
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
  }
</style>
