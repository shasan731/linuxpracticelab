<script lang="ts">
  import { api } from '../lib/ipc';
  import { app, describeError } from '../lib/appState.svelte';

  let retention = $state('none');
  let theme = $state('dark');
  let busy = $state(false);
  let verifyResult = $state<string[] | undefined>();
  let loaded = $state(false);

  const retentionOptions = [
    { value: 'none', label: 'Do not save', hint: 'Nothing you type is written to disk.' },
    { value: 'session', label: 'Save this session only', hint: 'Cleared when you next start up.' },
    { value: 'days:30', label: 'Save for 30 days', hint: 'Older entries are deleted automatically.' },
    { value: 'forever', label: 'Save indefinitely', hint: 'Kept until you change this setting.' },
  ];

  async function save(key: string, value: string): Promise<void> {
    try {
      await api.setSetting(key, value);
      app.notify('Saved.');
    } catch (error) {
      app.notify(describeError(error), 'error');
    }
  }

  async function verify(): Promise<void> {
    busy = true;
    try {
      verifyResult = await api.verifyRuntime();
    } catch (error) {
      app.notify(describeError(error), 'error');
    } finally {
      busy = false;
    }
  }

  function applyTheme(next: string): void {
    theme = next;
    document.documentElement.dataset.theme = next;
    void save('appearance.theme', next);
  }

  $effect(() => {
    if (loaded) return;
    loaded = true;
    void (async () => {
      try {
        retention = (await api.getSetting('history.retention')) ?? 'none';
        const storedTheme = (await api.getSetting('appearance.theme')) ?? 'dark';
        theme = storedTheme;
        document.documentElement.dataset.theme = storedTheme;
      } catch {
        // Defaults are fine; a missing setting is not an error worth reporting.
      }
    })();
  });
</script>

<h1>Settings</h1>

<section>
  <h2>Command history</h2>
  <p class="muted">
    Terminal history lives inside the Linux guest either way. This setting controls whether the
    application also keeps a copy on Windows for the History panel.
  </p>
  <div class="options">
    {#each retentionOptions as option (option.value)}
      <label title={option.hint}>
        <input
          type="radio"
          name="retention"
          value={option.value}
          checked={retention === option.value}
          onchange={() => {
            retention = option.value;
            void save('history.retention', option.value);
          }}
        />
        <span>
          <strong>{option.label}</strong>
          <span class="muted">{option.hint}</span>
        </span>
      </label>
    {/each}
  </div>
  <p class="muted">
    Exported transcripts always have anything that looks like a password, token or key masked
    before it leaves the application.
  </p>
</section>

<section>
  <h2>Appearance</h2>
  <div class="options row">
    <label>
      <input type="radio" name="theme" checked={theme === 'dark'} onchange={() => applyTheme('dark')} />
      Dark
    </label>
    <label>
      <input type="radio" name="theme" checked={theme === 'light'} onchange={() => applyTheme('light')} />
      Light
    </label>
  </div>
</section>

<section>
  <h2>Environment</h2>
  <p class="muted">{app.bootstrap?.acceleration ?? ''}</p>
  <dl>
    <div><dt>Application</dt><dd>{app.bootstrap?.appVersion ?? ''}</dd></div>
    <div><dt>Runtime</dt><dd>{app.bootstrap?.runtimeVersion ?? ''}</dd></div>
    <div><dt>Guest image</dt><dd>{app.vm?.imageVersion ?? 'not started yet'}</dd></div>
    <div><dt>Guest kernel</dt><dd>{app.vm?.guestKernel ?? 'not started yet'}</dd></div>
  </dl>
</section>

<section>
  <h2>Recovery</h2>
  <div class="actions">
    <button type="button" disabled={busy} onclick={verify}>Verify runtime files</button>
    <button type="button" disabled={busy} onclick={() => app.startSession()}>Restart Linux</button>
    <button
      type="button"
      disabled={busy}
      onclick={async () => {
        busy = true;
        try {
          await api.factoryResetPractice();
          app.notify('Free Practice will be rebuilt next time you open it.');
        } catch (error) {
          app.notify(describeError(error), 'error');
        } finally {
          busy = false;
        }
      }}
    >
      Reset practice environment
    </button>
  </div>

  {#if verifyResult}
    {#if verifyResult.length === 0}
      <p class="good">Every runtime file is present and matches what was installed.</p>
    {:else}
      <ul class="problems">
        {#each verifyResult as problem (problem)}
          <li>{problem}</li>
        {/each}
      </ul>
    {/if}
  {/if}
</section>

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
    margin: 0 0 6px;
  }

  section {
    max-width: 80ch;
  }

  .options {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 8px 0;
  }

  .options.row {
    flex-direction: row;
    gap: 18px;
  }

  .options label {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    font-size: 0.9rem;
  }

  .options label span {
    display: flex;
    flex-direction: column;
  }

  dl {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
    gap: 4px 20px;
    font-size: 0.85rem;
    margin: 0;
  }

  dl div {
    display: flex;
    gap: 8px;
  }

  dt {
    color: var(--muted);
    min-width: 11ch;
  }

  dd {
    margin: 0;
  }

  .actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }

  .problems {
    color: var(--bad);
    font-size: 0.85rem;
    margin-top: 8px;
  }

  .good {
    color: var(--good);
    font-size: 0.85rem;
  }

  .muted {
    color: var(--muted);
    font-size: 0.85rem;
  }
</style>
