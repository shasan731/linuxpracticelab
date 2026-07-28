<script lang="ts">
  import { app } from '../lib/appState.svelte';
</script>

<h1>Help</h1>

<section>
  <h2>What this application is</h2>
  <p>
    A small, real Linux computer runs inside this window. It is a complete Debian system with its
    own kernel, its own filesystem and its own processes. When you type a command, that command
    runs on Linux for real. Nothing is simulated or faked.
  </p>
</section>

<section>
  <h2>Can I damage my Windows computer?</h2>
  <p>
    No. The Linux system cannot see your Windows drives, your documents, your registry or your
    network. It has no access to PowerShell or Command Prompt. Even
    <code>sudo rm -rf /</code> only destroys the disposable Linux environment, which you can
    rebuild in seconds.
  </p>
  <p class="muted">
    That is why the curriculum lets you break things: the consequences are real inside Linux and
    nonexistent outside it.
  </p>
</section>

<section>
  <h2>What happened when my environment stopped booting</h2>
  <p>
    A Linux system needs certain files to start: the init program, the shell, the C library and
    the device nodes under <code>/dev</code>. If those are deleted, the kernel has nothing to hand
    control to, so the system stops before you ever see a prompt.
  </p>
  <p>
    Your shell stopped responding because the shell binary itself, or a library it depends on, was
    removed. Windows was unaffected because it was never reachable from inside Linux.
  </p>
  <p>
    On a real server, three things reduce this risk: not working as root by default, keeping
    backups you have actually tested restoring, and checking what a command will match before
    running it with a wildcard. This application gives you snapshots for the same reason.
  </p>
</section>

<section>
  <h2>Passwords used in the lab</h2>
  <p>
    The practice account is <code>student</code> and its password is <code>linuxlab</code>. It is
    written down here on purpose: it is a practice system, and lessons that need
    <code>sudo</code> will ask you for it.
  </p>
</section>

<section>
  <h2>Why is it slow to start?</h2>
  <p>{app.bootstrap?.acceleration ?? ''}</p>
  <p class="muted">
    If your computer has hardware virtualisation available, the lab uses it automatically. If not,
    everything still works, just more slowly to start. You are never asked to change a Windows
    setting.
  </p>
</section>

<section>
  <h2>Keyboard shortcuts inside the terminal</h2>
  <dl>
    <div><dt>Ctrl+C</dt><dd>Stop the command that is running</dd></div>
    <div><dt>Ctrl+D</dt><dd>Signal end of input, or exit the shell</dd></div>
    <div><dt>Ctrl+Z</dt><dd>Suspend the current command</dd></div>
    <div><dt>Ctrl+R</dt><dd>Search backwards through your command history</dd></div>
    <div><dt>Ctrl+A / Ctrl+E</dt><dd>Jump to the start or end of the line</dd></div>
    <div><dt>Ctrl+U / Ctrl+K</dt><dd>Delete to the start or end of the line</dd></div>
    <div><dt>Tab</dt><dd>Complete a command, file or directory name</dd></div>
  </dl>
  <p class="muted">
    These are handled by Bash inside Linux, not by this application, so they behave exactly as
    they would on any other Linux machine.
  </p>
</section>

<section>
  <h2>Your data</h2>
  <p>
    Progress, notes and settings are stored in a local database on this computer. There is no
    account, no server and no telemetry. Command history is not saved unless you turn it on in
    Settings, and exported transcripts have anything resembling a credential masked first.
  </p>
</section>

<style>
  h1 {
    margin: 0;
    font-size: 1.4rem;
  }

  h2 {
    font-size: 0.95rem;
    margin: 0 0 4px;
  }

  section {
    max-width: 80ch;
  }

  p {
    margin: 0 0 8px;
    line-height: 1.6;
  }

  dl {
    display: grid;
    gap: 2px;
    margin: 6px 0;
    font-size: 0.88rem;
  }

  dl div {
    display: flex;
    gap: 12px;
  }

  dt {
    min-width: 14ch;
    color: var(--muted);
  }

  dd {
    margin: 0;
  }

  .muted {
    color: var(--muted);
    font-size: 0.88rem;
  }
</style>
