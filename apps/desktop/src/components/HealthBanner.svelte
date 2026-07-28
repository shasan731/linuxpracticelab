<script lang="ts">
  // Startup health findings. Blocking findings appear first and always name their fix, because
  // a health check that only reports a problem is worse than none at all.

  import { api } from '../lib/ipc';
  import { app, describeError } from '../lib/appState.svelte';
  import type { RecoveryAction } from '../lib/types';

  const findings = $derived(app.bootstrap?.health.findings ?? []);
  let busy = $state(false);

  const actionLabels: Record<RecoveryAction, string> = {
    none: '',
    'verify-runtime-files': 'Verify runtime files',
    'reinstall-runtime': 'Reinstall runtime',
    'repair-user-overlay': 'Check practice environment',
    'reset-practice-environment': 'Reset practice environment',
    'restore-last-snapshot': 'Restore last snapshot',
    'free-disk-space': 'Recheck disk space',
    'export-diagnostic-report': 'Export diagnostics',
  };

  async function runAction(action: RecoveryAction): Promise<void> {
    busy = true;
    try {
      switch (action) {
        case 'verify-runtime-files':
        case 'reinstall-runtime': {
          if (action === 'reinstall-runtime') {
            await api.installRuntime(true);
          }
          const problems = await api.verifyRuntime();
          app.notify(
            problems.length === 0
              ? 'All runtime files are intact.'
              : problems.join(' '),
            problems.length === 0 ? 'info' : 'error',
          );
          break;
        }
        case 'reset-practice-environment': {
          await api.factoryResetPractice();
          app.notify('The practice environment will be rebuilt the next time you open it.');
          break;
        }
        case 'free-disk-space':
        case 'repair-user-overlay': {
          const health = await api.healthCheck();
          if (app.bootstrap) app.bootstrap.health = health;
          app.notify(
            health.findings.length === 0 ? 'Everything checks out.' : 'Some problems remain.',
          );
          break;
        }
        default:
          break;
      }
    } catch (error) {
      app.notify(describeError(error), 'error');
    } finally {
      busy = false;
    }
  }
</script>

{#if findings.length > 0}
  <section class="health" aria-label="Environment health">
    {#each findings as finding (finding.title + finding.detail)}
      <div class="finding" data-severity={finding.severity}>
        <div>
          <strong>{finding.title}</strong>
          <p>{finding.detail}</p>
        </div>
        {#if finding.action !== 'none'}
          <button type="button" disabled={busy} onclick={() => runAction(finding.action)}>
            {actionLabels[finding.action]}
          </button>
        {/if}
      </div>
    {/each}
  </section>
{/if}

<style>
  .health {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .finding {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 10px 14px;
    border-radius: 6px;
    border: 1px solid var(--border);
    background: var(--surface-2);
  }

  .finding[data-severity='blocking'] {
    border-color: var(--bad);
  }

  .finding[data-severity='warning'] {
    border-color: var(--warn);
  }

  p {
    margin: 2px 0 0;
    color: var(--muted);
    font-size: 0.85rem;
    max-width: 80ch;
  }

  button {
    margin-left: auto;
    flex: 0 0 auto;
  }
</style>
