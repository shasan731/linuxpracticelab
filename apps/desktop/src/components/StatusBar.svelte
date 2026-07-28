<script lang="ts">
  import { app } from '../lib/appState.svelte';
  import { formatDuration, vmStateLabel, vmStateTone } from '../lib/format';

  const tone = $derived(app.vm ? vmStateTone(app.vm.state) : 'idle');
  const label = $derived(app.vm ? vmStateLabel(app.vm.state) : 'Linux is not running');
</script>

<footer>
  <span class="status" data-tone={tone}>
    <span class="dot" aria-hidden="true"></span>
    {label}
  </span>

  {#if app.vm?.bootMillis}
    <span class="detail">Started in {formatDuration(app.vm.bootMillis / 1000)}</span>
  {/if}

  {#if app.vm}
    <span class="detail">{app.vm.memoryMb} MB · {app.vm.machine}</span>
  {/if}

  {#if app.bootstrap}
    <span class="detail acceleration" title={app.bootstrap.acceleration}>
      {app.vm?.accel === 'whpx' ? 'Hardware accelerated' : 'Software translation'}
    </span>
  {/if}

  {#if app.vm?.detail}
    <span class="detail warn">{app.vm.detail}</span>
  {/if}

  <span class="spacer"></span>

  {#if app.vmReady}
    <button type="button" onclick={() => app.stopSession()}>Stop Linux</button>
  {/if}
</footer>

<style>
  footer {
    grid-column: 2;
    grid-row: 2;
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 6px 24px;
    border-top: 1px solid var(--border);
    background: var(--surface-2);
    font-size: 0.8rem;
    color: var(--muted);
  }

  .status {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--text);
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--muted);
  }

  .status[data-tone='good'] .dot {
    background: var(--good);
  }
  .status[data-tone='busy'] .dot {
    background: var(--warn);
    animation: pulse 1.2s ease-in-out infinite;
  }
  .status[data-tone='bad'] .dot {
    background: var(--bad);
  }

  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }

  .warn {
    color: var(--warn);
  }

  .acceleration {
    cursor: help;
    border-bottom: 1px dotted var(--border);
  }

  .spacer {
    flex: 1 1 auto;
  }

  button {
    font: inherit;
    background: none;
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--text);
    padding: 2px 10px;
    cursor: pointer;
  }
</style>
