<script lang="ts">
  // The Linux terminal.
  //
  // xterm.js renders and handles input; the guest's Bash does everything else. Keystrokes go
  // straight through, which is why Ctrl+C, Ctrl+R, Ctrl+Z and tab completion behave exactly as
  // they do on a real machine rather than being reimplemented here.
  //
  // The toolbar lives inside this component rather than being driven from outside, so the
  // terminal owns its own state and callers just place `<Terminal />` where they want it.

  import { onMount } from 'svelte';
  import { Terminal } from '@xterm/xterm';
  import { FitAddon } from '@xterm/addon-fit';
  import { SearchAddon } from '@xterm/addon-search';
  import '@xterm/xterm/css/xterm.css';
  import { api, onTerminalClosed, onTerminalOutput } from '../lib/ipc';
  import { app, describeError } from '../lib/appState.svelte';
  import { commandFromPromptLine } from '../lib/format';

  interface Props {
    /// Hidden in the lesson player, where screen space belongs to the lesson.
    showToolbar?: boolean;
  }
  let { showToolbar = true }: Props = $props();

  let container: HTMLDivElement | undefined = $state();
  let terminal: Terminal | undefined;
  let fit: FitAddon | undefined;
  let search: SearchAddon | undefined;
  let searchTerm = $state('');
  let showSearch = $state(false);
  let fontSize = $state(14);

  async function sendSize(): Promise<void> {
    if (!terminal) return;
    // A serial console carries no window size, so the guest is told explicitly. Failures are
    // ignored: the guest may still be booting, and the next resize will succeed.
    try {
      await api.terminalResize(terminal.rows, terminal.cols);
    } catch {
      /* not fatal */
    }
  }

  function changeFontSize(delta: number): void {
    fontSize = Math.min(24, Math.max(10, fontSize + delta));
    if (terminal) {
      terminal.options.fontSize = fontSize;
      fit?.fit();
      void sendSize();
    }
  }

  async function copySelection(): Promise<void> {
    const selection = terminal?.getSelection();
    if (!selection) {
      app.notify('Select some text in the terminal first.');
      return;
    }
    try {
      await navigator.clipboard.writeText(selection);
    } catch (error) {
      app.notify(`Could not copy: ${describeError(error)}`, 'error');
    }
  }

  async function paste(): Promise<void> {
    try {
      const text = await navigator.clipboard.readText();
      if (text) await api.terminalWrite(new TextEncoder().encode(text));
    } catch (error) {
      app.notify(`Could not paste: ${describeError(error)}`, 'error');
    }
  }

  function transcript(): string {
    if (!terminal) return '';
    const buffer = terminal.buffer.active;
    const lines: string[] = [];
    for (let index = 0; index < buffer.length; index += 1) {
      lines.push(buffer.getLine(index)?.translateToString(true) ?? '');
    }
    // Trailing blanks are just the unused part of the scrollback.
    while (lines.length > 0 && lines[lines.length - 1]?.trim() === '') lines.pop();
    return lines.join('\n');
  }

  async function downloadTranscript(): Promise<void> {
    try {
      // Redaction happens on the host so it cannot be bypassed from here.
      const redacted = await api.exportTranscript(transcript());
      const blob = new Blob([redacted], { type: 'text/plain;charset=utf-8' });
      const url = URL.createObjectURL(blob);
      const link = document.createElement('a');
      link.href = url;
      link.download = 'linux-practice-lab-session.txt';
      link.click();
      URL.revokeObjectURL(url);
      app.notify('Session transcript saved. Anything that looked like a password was masked.');
    } catch (error) {
      app.notify(describeError(error), 'error');
    }
  }

  async function restartShell(): Promise<void> {
    // exit ends the login shell; the guest's getty immediately starts a fresh one.
    try {
      await api.terminalWrite(new TextEncoder().encode('exit\n'));
    } catch (error) {
      app.notify(describeError(error), 'error');
    }
  }

  function findNext(): void {
    if (searchTerm) search?.findNext(searchTerm);
  }

  onMount(() => {
    const instance = new Terminal({
      fontSize,
      fontFamily: 'Cascadia Mono, Consolas, "DejaVu Sans Mono", monospace',
      // Bounded so a runaway `yes` cannot grow the renderer's memory without limit.
      scrollback: 4000,
      cursorBlink: true,
      convertEol: false,
      allowProposedApi: true,
      // No web-links addon on purpose: spec 8.5 forbids clickable file:// links, and
      // validating arbitrary protocols is not worth the risk in an application whose whole
      // point is running commands whose output nobody has vetted.
      theme: {
        background: '#0f1116',
        foreground: '#d7dae0',
        cursor: '#7cc4ff',
        selectionBackground: '#2f3a4d',
      },
    });
    fit = new FitAddon();
    search = new SearchAddon();
    instance.loadAddon(fit);
    instance.loadAddon(search);
    if (container) instance.open(container);
    fit.fit();
    terminal = instance;

    instance.onData((data) => {
      if (data === '\r') {
        // Read the line the cursor is on at the moment Enter is pressed. Doing it this way
        // means history capture never has to model line editing, so Ctrl+U, arrow keys and
        // history recall are all reflected correctly.
        const buffer = instance.buffer.active;
        const line = buffer.getLine(buffer.cursorY + buffer.baseY)?.translateToString(true);
        const command = commandFromPromptLine(line ?? '');
        if (command) void api.recordCommand(command).catch(() => undefined);
      }
      void api.terminalWrite(new TextEncoder().encode(data)).catch((error) => {
        app.notify(describeError(error), 'error');
      });
    });

    const outputPromise = onTerminalOutput((bytes) => instance.write(bytes));
    const closedPromise = onTerminalClosed(() => {
      instance.writeln('\r\n\x1b[33mThe Linux terminal has disconnected.\x1b[0m');
      void app.pollVm();
    });

    const observer = new ResizeObserver(() => {
      fit?.fit();
      void sendSize();
    });
    if (container) observer.observe(container);
    void sendSize();

    return () => {
      observer.disconnect();
      void outputPromise.then((unlisten) => unlisten());
      void closedPromise.then((unlisten) => unlisten());
      instance.dispose();
      terminal = undefined;
    };
  });
</script>

<div class="terminal-shell">
  {#if showToolbar}
    <div class="toolbar" role="toolbar" aria-label="Terminal controls">
      <button type="button" onclick={copySelection}>Copy</button>
      <button type="button" onclick={paste}>Paste</button>
      <button type="button" onclick={() => terminal?.clear()}>Clear display</button>
      <button type="button" onclick={() => terminal?.reset()}>Reset terminal</button>
      <button type="button" onclick={restartShell}>Restart shell</button>
      <button type="button" onclick={() => app.startSession()}>Restart VM</button>
      <span class="divider" aria-hidden="true"></span>
      <button type="button" onclick={() => changeFontSize(-1)} aria-label="Decrease font size">
        A−
      </button>
      <button type="button" onclick={() => changeFontSize(1)} aria-label="Increase font size">
        A+
      </button>
      <button type="button" onclick={() => (showSearch = !showSearch)}>Search output</button>
      <button type="button" onclick={downloadTranscript}>Download transcript</button>
    </div>
  {/if}

  {#if showSearch}
    <div class="search-bar">
      <input
        type="search"
        placeholder="Search output"
        bind:value={searchTerm}
        onkeydown={(event) => {
          if (event.key === 'Enter') findNext();
          if (event.key === 'Escape') showSearch = false;
        }}
        aria-label="Search terminal output"
      />
      <button type="button" onclick={findNext}>Next</button>
      <button type="button" onclick={() => (showSearch = false)}>Close</button>
    </div>
  {/if}

  <div class="terminal" bind:this={container} role="application" aria-label="Linux terminal"></div>
</div>

<style>
  .terminal-shell {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    background: #0f1116;
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
  }

  .toolbar,
  .search-bar {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    align-items: center;
    padding: 6px 8px;
    background: #1b1f27;
    border-bottom: 1px solid #2a2f3a;
  }

  .toolbar button,
  .search-bar button {
    font: inherit;
    font-size: 0.75rem;
    background: #0f1116;
    color: #d7dae0;
    border: 1px solid #2a2f3a;
    border-radius: 4px;
    padding: 2px 8px;
    cursor: pointer;
  }

  .toolbar button:hover,
  .search-bar button:hover {
    border-color: #3a4252;
  }

  .divider {
    width: 1px;
    height: 18px;
    background: #2a2f3a;
    margin: 0 4px;
  }

  .search-bar input {
    flex: 1 1 auto;
    background: #0f1116;
    color: #d7dae0;
    border: 1px solid #2a2f3a;
    border-radius: 4px;
    padding: 4px 8px;
  }

  .terminal {
    flex: 1 1 auto;
    min-height: 0;
    padding: 8px;
  }
</style>
