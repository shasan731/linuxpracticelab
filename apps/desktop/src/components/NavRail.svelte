<script lang="ts">
  import { app, type Route } from '../lib/appState.svelte';

  const items: { label: string; route: Route; hint: string }[] = [
    { label: 'Home', route: { name: 'home' }, hint: 'Continue where you left off' },
    { label: 'Learn', route: { name: 'learn' }, hint: 'Browse the curriculum' },
    { label: 'Practice', route: { name: 'practice' }, hint: 'A sandbox you can break' },
    { label: 'Scenario Labs', route: { name: 'scenarios' }, hint: 'Fix a broken system' },
    { label: 'Command Reference', route: { name: 'reference' }, hint: 'Look a command up' },
    { label: 'Progress', route: { name: 'progress' }, hint: 'What you have learned' },
    { label: 'Achievements', route: { name: 'achievements' }, hint: 'Milestones' },
    { label: 'Settings', route: { name: 'settings' }, hint: 'Preferences and recovery' },
    { label: 'Help', route: { name: 'help' }, hint: 'How this application works' },
  ];

  function isActive(route: Route): boolean {
    // The lesson player is reached from Learn, so Learn stays highlighted while inside it.
    if (app.route.name === 'lesson' && route.name === 'learn') return true;
    return app.route.name === route.name;
  }
</script>

<nav aria-label="Main">
  <div class="brand">
    <span class="mark" aria-hidden="true">▚</span>
    <span>Linux Practice Lab</span>
  </div>

  <ul>
    {#each items as item (item.label)}
      <li>
        <button
          type="button"
          class:active={isActive(item.route)}
          aria-current={isActive(item.route) ? 'page' : undefined}
          title={item.hint}
          onclick={() => app.navigate(item.route)}
        >
          {item.label}
        </button>
      </li>
    {/each}
  </ul>

  {#if app.bootstrap}
    <p class="version">
      Version {app.bootstrap.appVersion}
      <br />
      Runtime {app.bootstrap.runtimeVersion}
    </p>
  {/if}
</nav>

<style>
  nav {
    grid-column: 1;
    grid-row: 1 / span 2;
    background: var(--surface-2);
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    padding: 16px 12px;
    gap: 16px;
    overflow-y: auto;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 600;
    padding: 0 8px;
  }

  .mark {
    color: var(--accent);
    font-size: 1.2rem;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  button {
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    border-radius: 6px;
    color: var(--text);
    padding: 8px 10px;
    font: inherit;
    cursor: pointer;
  }

  button:hover {
    background: var(--surface-3);
  }

  button.active {
    background: var(--accent-soft);
    color: var(--accent);
    font-weight: 600;
  }

  button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  .version {
    margin-top: auto;
    padding: 0 10px;
    color: var(--muted);
    font-size: 0.75rem;
    line-height: 1.5;
  }
</style>
