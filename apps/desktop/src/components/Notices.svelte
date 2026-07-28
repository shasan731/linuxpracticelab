<script lang="ts">
  import { app } from '../lib/appState.svelte';
</script>

{#if app.notices.length > 0}
  <div class="notices" role="status" aria-live="polite">
    {#each app.notices as notice (notice.id)}
      <div class="notice" data-tone={notice.tone}>
        <p>{notice.text}</p>
        <button type="button" onclick={() => app.dismiss(notice.id)} aria-label="Dismiss">×</button>
      </div>
    {/each}
  </div>
{/if}

<style>
  .notices {
    position: fixed;
    right: 24px;
    bottom: 48px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: 44ch;
    z-index: 10;
  }

  .notice {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    background: var(--surface-3);
    border: 1px solid var(--border);
    border-left: 3px solid var(--muted);
    border-radius: 6px;
    padding: 10px 12px;
    box-shadow: 0 6px 18px rgb(0 0 0 / 0.25);
  }

  .notice[data-tone='error'] {
    border-left-color: var(--bad);
  }

  .notice[data-tone='info'] {
    border-left-color: var(--accent);
  }

  p {
    margin: 0;
    font-size: 0.85rem;
    line-height: 1.45;
  }

  button {
    margin-left: auto;
    background: none;
    border: none;
    color: var(--muted);
    font-size: 1.1rem;
    line-height: 1;
    cursor: pointer;
  }
</style>
