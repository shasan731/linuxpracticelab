<script lang="ts">
  import { api } from '../lib/ipc';
  import { app, describeError } from '../lib/appState.svelte';
  import { failureCategoryLabel, formatDuration } from '../lib/format';
  import type { FailureCategory, ProgressReport } from '../lib/types';

  let report = $state<ProgressReport | undefined>();

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

<h1>Progress</h1>
<p class="muted">Everything here is stored on this computer only.</p>

{#if !report}
  <p class="muted">Reading your progress…</p>
{:else}
  <div class="grid">
    <div class="card"><span class="value">{report.lessonsPassed}</span><span>lessons passed</span></div>
    <div class="card"><span class="value">{report.lessonsMastered}</span><span>mastered</span></div>
    <div class="card"><span class="value">{report.needsReview}</span><span>need review</span></div>
    <div class="card"><span class="value">{report.hintsUsed}</span><span>hints used</span></div>
    <div class="card">
      <span class="value">{formatDuration(report.practiceSeconds)}</span>
      <span>in Free Practice</span>
    </div>
  </div>

  <section>
    <h2>Commands you can use confidently</h2>
    {#if report.commandsMastered.length === 0}
      <p class="muted">Pass a lesson and the commands it covers appear here.</p>
    {:else}
      <p class="commands">
        {#each report.commandsMastered as command (command)}
          <code>{command}</code>
        {/each}
      </p>
    {/if}
  </section>

  <section>
    <h2>Where you get stuck most</h2>
    {#if report.commonFailures.length === 0}
      <p class="muted">Nothing to report yet.</p>
    {:else}
      <ul class="failures">
        {#each report.commonFailures as [category, count] (category)}
          <li>
            <span>{failureCategoryLabel(category as FailureCategory)}</span>
            <span class="bar">
              <span
                style={`width:${Math.min(100, (count / (report.commonFailures[0]?.[1] ?? 1)) * 100)}%`}
              ></span>
            </span>
            <span class="count">{count}</span>
          </li>
        {/each}
      </ul>
      <p class="muted">
        These are the categories of mistake that came up most often. They are a better guide to
        what to revise than a raw score.
      </p>
    {/if}
  </section>
{/if}

<style>
  h1 {
    margin: 0;
    font-size: 1.4rem;
  }

  h2 {
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
    gap: 10px;
  }

  .card {
    display: flex;
    flex-direction: column;
    gap: 2px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 10px 12px;
    font-size: 0.8rem;
    color: var(--muted);
  }

  .value {
    font-size: 1.4rem;
    font-weight: 600;
    color: var(--text);
  }

  .commands {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .commands code {
    background: var(--surface-3);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 1px 7px;
    font-size: 0.8rem;
  }

  .failures {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 5px;
    max-width: 60ch;
  }

  .failures li {
    display: grid;
    grid-template-columns: 22ch 1fr 3ch;
    align-items: center;
    gap: 10px;
    font-size: 0.85rem;
  }

  .bar {
    height: 6px;
    background: var(--surface-3);
    border-radius: 3px;
    overflow: hidden;
  }

  .bar span {
    display: block;
    height: 100%;
    background: var(--warn);
  }

  .count {
    color: var(--muted);
    text-align: right;
  }

  .muted {
    color: var(--muted);
    font-size: 0.85rem;
  }
</style>
