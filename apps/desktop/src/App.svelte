<script lang="ts">
  import { onMount } from 'svelte';
  import { app } from './lib/appState.svelte';
  import NavRail from './components/NavRail.svelte';
  import StatusBar from './components/StatusBar.svelte';
  import Notices from './components/Notices.svelte';
  import HealthBanner from './components/HealthBanner.svelte';
  import Home from './routes/Home.svelte';
  import Learn from './routes/Learn.svelte';
  import LessonPlayer from './routes/LessonPlayer.svelte';
  import Practice from './routes/Practice.svelte';
  import ScenarioLabs from './routes/ScenarioLabs.svelte';
  import Reference from './routes/Reference.svelte';
  import Progress from './routes/Progress.svelte';
  import Achievements from './routes/Achievements.svelte';
  import Settings from './routes/Settings.svelte';
  import Help from './routes/Help.svelte';

  onMount(() => {
    void app.load();
    // Polls only while something is in flight; a ready or stopped machine needs no polling.
    const timer = setInterval(() => {
      const state = app.vm?.state;
      if (state === 'starting' || state === 'booting-guest' || state === 'stopping') {
        void app.pollVm();
      }
    }, 1500);
    return () => clearInterval(timer);
  });
</script>

<div class="layout">
  <NavRail />

  <main>
    {#if app.startupError}
      <div class="startup-error" role="alert">
        <h1>Linux Practice Lab could not start</h1>
        <p>{app.startupError}</p>
        <p class="muted">
          Your progress is stored separately and has not been affected. Reinstalling the runtime
          from Settings usually resolves this.
        </p>
      </div>
    {:else if app.loading}
      <p class="loading">Preparing the Linux environment and loading the curriculum…</p>
    {:else}
      <HealthBanner />
      {#if app.route.name === 'home'}
        <Home />
      {:else if app.route.name === 'learn'}
        <Learn />
      {:else if app.route.name === 'lesson'}
        <LessonPlayer lessonId={app.route.lessonId} />
      {:else if app.route.name === 'practice'}
        <Practice />
      {:else if app.route.name === 'scenarios'}
        <ScenarioLabs />
      {:else if app.route.name === 'reference'}
        <Reference />
      {:else if app.route.name === 'progress'}
        <Progress />
      {:else if app.route.name === 'achievements'}
        <Achievements />
      {:else if app.route.name === 'settings'}
        <Settings />
      {:else}
        <Help />
      {/if}
    {/if}
  </main>

  <Notices />
  <StatusBar />
</div>

<style>
  .layout {
    display: grid;
    grid-template-columns: 220px 1fr;
    grid-template-rows: 1fr auto;
    height: 100vh;
    overflow: hidden;
  }

  main {
    grid-column: 2;
    grid-row: 1;
    min-height: 0;
    overflow: auto;
    padding: 20px 24px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .loading {
    color: var(--muted);
  }

  .startup-error {
    max-width: 60ch;
  }

  .startup-error h1 {
    font-size: 1.3rem;
  }

  .muted {
    color: var(--muted);
  }
</style>
