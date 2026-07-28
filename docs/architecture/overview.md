# Architecture

## Why a real virtual machine

A simulated shell is easier to build and cheaper to run, and it is the wrong choice. Every
simulator eventually teaches something that is not true: an option it does not implement, an error
message that differs, a permission model that is approximated. The learner then meets a real server
and discovers their mental model has holes in exactly the places the simulator took shortcuts.

Running a real Debian kernel and user space costs a few hundred megabytes and a few seconds of
startup. In exchange, `man` pages are the real ones, `systemctl` really is systemd, permission
errors come from the kernel, and a mistake has real consequences. Nothing has to be maintained in
step with upstream because nothing is reimplemented.

## The layers

```
┌───────────────────────────────────────────────────────────────────┐
│ Svelte 5 frontend                                                 │
│   Lesson player · xterm.js terminal · panels · Free Practice      │
└────────────────────────────┬──────────────────────────────────────┘
                             │ Tauri IPC (typed commands, no plugins)
┌────────────────────────────▼──────────────────────────────────────┐
│ Rust host application                                             │
│   lesson-engine   catalogue, progression, hints, feedback         │
│   progress-store  local SQLite, retention, redaction              │
│   runtime-manager layout, integrity, startup health               │
│   vm-manager      QEMU arguments, QMP, Job Object, overlays       │
└──────┬──────────────────────────────┬─────────────────────────────┘
       │ loopback chardev             │ loopback TCP
┌──────▼──────────────┐      ┌────────▼──────────┐
│ Serial console      │      │ QMP               │
│ → xterm.js          │      │ pause/stop/status │
└──────┬──────────────┘      └────────┬──────────┘
┌──────▼───────────────────────────────▼─────────────────────────────┐
│ QEMU: microvm, 1 vCPU, 256 MB, no display, no USB, no passthrough  │
│ ┌────────────────────────────────────────────────────────────────┐ │
│ │ Debian 13 guest                                                │ │
│ │   bash (serial console, autologin as student)                   │ │
│ │   linuxlab-agent (root, virtio-serial, token-authenticated)     │ │
│ │   network namespaces: the internal laboratory network           │ │
│ └────────────────────────────────────────────────────────────────┘ │
└────────────────────────────────────────────────────────────────────┘
```

## Startup sequence

Deliberately ordered so a failure is reported before anything expensive or destructive happens:

1. Resolve the layout under `%LOCALAPPDATA%`; create the writable directories.
2. Start logging, then take the session lock. A lock already present means the previous run
   crashed, which the health report surfaces as a warning — never as a block.
3. Verify runtime files. Presence and size on every launch; full SHA-256 on first run, after an
   upgrade, and on request. Hashing a 3 GB image on every start would blow the cold-start budget.
4. Load the lesson catalogue. A malformed package or a prerequisite cycle fails loudly.
5. Open SQLite, apply migrations, enforce the history retention policy.
6. Show the window.

**The virtual machine does not start here.** It starts when the learner opens a lesson or Free
Practice, which keeps a cold launch fast and avoids spending a guest boot on someone who only
wanted the command reference.

## Starting a session

1. Kill a stale QEMU from a previous crash — but only if the running image really is our QEMU.
2. Create or recreate the overlay. Free Practice persists; a lesson overlay is recreated every
   time, which is what makes a lesson start from a known checkpoint.
3. Reserve three loopback ports and generate a 256-bit control token.
4. Spawn QEMU suspended, contain it in a Job Object, resume it.
5. Attach the console immediately, so the learner watches Linux boot instead of an empty panel.
6. Poll the agent until it answers, then mark the session ready. The budget is generous because
   software translation is slow, and slowness must not be reported as failure.

## Storage

```
debian-base.raw        read-only, shipped, verified, never opened writable
       │
       ├── free-practice.qcow2      persistent sandbox
       │
       └── lesson-<id>.qcow2        recreated on every lesson start
```

A lesson overlay is backed by the read-only base, **not** by Free Practice. A lesson must not be
affected by whatever the learner has done to their sandbox, and it must not be able to damage it.

Snapshots are plain file copies rather than internal qcow2 snapshots, so a snapshot stays
restorable when the live overlay is corrupt — which is precisely the situation snapshots exist
for. Restore moves the live file aside first, so a failure part-way through still leaves something
to go back to.

## The control channel

Newline-delimited JSON over a virtio-serial port that QEMU exposes as a loopback socket.

A character device rather than a TCP port because the offline curriculum has **no guest
networking at all**. A guest-reachable port would require a NIC, which would undermine the offline
promise and make the networking lessons non-deterministic. So the same mechanism serves both the
offline and the networked modules.

The host serialises requests through a mutex — the agent handles one frame at a time, and
correlating interleaved replies would buy nothing — and retries once transparently if the channel
has dropped, which happens whenever the terminal is restarted.

## Validation

The registry in `lessons/schema/validators.json` is the single definition of what a check is
called and what parameters it takes. It is embedded into `shared-types` with `include_str!` and
used by:

- `scripts/validate-lessons.ts`, at authoring time
- the host, before sending anything to the guest
- the guest agent, before touching the system
- a coverage test that fails if a validator is declared implemented but has no handler

The agent has 70 validators across seven categories. Where a fact can be read from `/proc` or
`/etc`, it is read directly rather than parsed out of a tool's output — `/proc/net/tcp` for
listening sockets rather than `ss`, `/etc/passwd` for accounts rather than `getent` — because
parsing human-readable output is fragile, locale-dependent, and breaks in exactly the lessons
where the learner has deliberately damaged the system.

Three details are worth calling out because they are easy to get wrong:

- **`/proc/<pid>/stat` is parsed by finding the last `)`.** The comm field can contain spaces and
  parentheses, so splitting the line on whitespace is a real bug that appears only for oddly named
  processes.
- **The agent excludes its own process tree from every process match.** Otherwise a validator
  looking for `sleep 300` would match the `runuser` invocation the validator itself just made.
- **Journal queries are scoped to the current attempt.** A message left by a previous try must not
  let a later attempt pass.

## Why microvm

`microvm` has a reduced device model and boots the kernel directly, with no firmware and no
bootloader — which is most of the difference between a 3-second start and a 20-second one. The cost
is that virtio devices are MMIO rather than PCI, so the device names differ; getting that wrong
gives a guest with no disk, which looks like a corrupt image rather than a wrong flag. `q35` is
kept as a fallback for anything microvm cannot express, and the argument builder handles both.

The reduced machine keeps its programmable interval timer enabled. Without that timer, current
Debian kernels have no early clock-calibration source and stop before discovering the virtio
devices. Software translation uses QEMU's single-thread TCG mode because the lab has one guest
vCPU; on Windows, multi-thread TCG can starve the emulated timer during that same early boot.

`panic=-1` with `-no-reboot` means a fatal guest fault makes QEMU exit, which the host reads as
"the environment is no longer bootable" and turns into an offer to restore a snapshot — rather
than a hung console the learner has to interpret.

## Progression

Three modes coexist, and the invariant is that none of them can make a lesson permanently
unreachable:

- **Guided path** unlocks a lesson when its prerequisites are passed. A lesson that only reached
  *needs review* still counts as passed for unlocking, because hints must never block progression.
- **Open library** warns but never blocks.
- **Assessment** strips hints, syntax and worked examples.

Mastery is computed per task and rolled up by taking the **worst** task in the lesson: a learner
who needed the answer for one task has not mastered the lesson. Revealing the solution outranks
hint counting, because looking at the answer means the lesson should come back around regardless
of how few hints were opened first. And stored mastery only ever improves, so practising a
mastered lesson can never cost credit already earned.
