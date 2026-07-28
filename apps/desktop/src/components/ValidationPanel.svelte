<script lang="ts">
  // Shows what the last check found, one line per validator.
  //
  // Every check is listed, not just the failures, because seeing "the directory exists" tick
  // and "the file exists" fail is what tells a learner they are half done rather than wrong.

  import { failureCategoryLabel } from '../lib/format';
  import type { CheckResult } from '../lib/types';

  interface Props {
    result: CheckResult | undefined;
  }
  let { result }: Props = $props();
</script>

{#if !result}
  <p class="muted">Run Check when you think the task is done, and the results appear here.</p>
{:else}
  <div class="summary" data-passed={result.validation.passed} data-authoring={result.authoringError}>
    <strong>{result.headline}</strong>
    {#if result.category && !result.validation.passed}
      <span class="category">{failureCategoryLabel(result.category)}</span>
    {/if}
  </div>

  {#if result.guidance}
    <p class="guidance">{result.guidance}</p>
  {/if}

  {#if result.diagnosis}
    <p class="diagnosis">{result.diagnosis}</p>
  {/if}

  <ul class="checks">
    {#each result.validation.outcomes as outcome, index (index)}
      <li data-state={outcome.errored ? 'errored' : outcome.passed ? 'passed' : 'failed'}>
        <span class="mark" aria-hidden="true">
          {outcome.errored ? '!' : outcome.passed ? '✓' : '✕'}
        </span>
        <span class="body">
          <span class="message">{outcome.message}</span>
          {#if outcome.expected || outcome.observed}
            <span class="detail">
              {#if outcome.expected}expected <code>{outcome.expected}</code>{/if}
              {#if outcome.expected && outcome.observed}, {/if}
              {#if outcome.observed}found <code>{outcome.observed}</code>{/if}
            </span>
          {/if}
        </span>
      </li>
    {/each}
  </ul>

  {#if !result.validation.passed && result.validation.completionPercent > 0}
    <p class="muted">
      {result.validation.completionPercent}% of the checks for this task are satisfied.
    </p>
  {/if}
{/if}

<style>
  .summary {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border-radius: 4px;
    border-left: 3px solid var(--muted);
    background: var(--surface-2);
    margin-bottom: 8px;
  }

  .summary[data-passed='true'] {
    border-left-color: var(--good);
  }

  .summary[data-authoring='true'] {
    border-left-color: var(--warn);
  }

  .category {
    font-size: 0.75rem;
    color: var(--muted);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 1px 8px;
  }

  .guidance,
  .diagnosis {
    margin: 0 0 10px;
    font-size: 0.85rem;
    max-width: 90ch;
  }

  .diagnosis {
    border-left: 3px solid var(--accent);
    padding-left: 10px;
    color: var(--text);
  }

  .checks {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .checks li {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    font-size: 0.85rem;
  }

  .mark {
    width: 1em;
    flex: 0 0 auto;
    font-weight: 700;
  }

  li[data-state='passed'] .mark {
    color: var(--good);
  }
  li[data-state='failed'] .mark {
    color: var(--bad);
  }
  li[data-state='errored'] .mark {
    color: var(--warn);
  }

  .body {
    display: flex;
    flex-direction: column;
  }

  .detail {
    color: var(--muted);
    font-size: 0.78rem;
  }

  .muted {
    color: var(--muted);
    font-size: 0.85rem;
  }
</style>
