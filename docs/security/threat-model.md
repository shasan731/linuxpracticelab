# Security model

The product invites a learner to run destructive commands as root and to deliberately destroy the
system they are working on. That is the whole point, and it is only defensible because the
boundary around it holds. This document states what that boundary is, what it is not, and how each
part of it is enforced and tested.

## The one promise

> Nothing a learner types inside Linux can affect Windows.

Everything below exists to keep that true.

## What the guest cannot reach, and why

| Not reachable | Enforced by |
| --- | --- |
| Windows drives and user folders | No `virtfs`, `9p` or `fsdev` argument is ever passed. Asserted by a unit test that greps the generated argument vector. |
| The Windows registry, PowerShell, `cmd.exe` | The guest is a separate machine with a separate kernel. There is no host-command channel in either direction. |
| Windows environment variables | QEMU is spawned with `env_clear()` and only `SystemRoot`, `windir`, `TEMP`, `TMP` and `PATH` restored. |
| The physical network and the learner's LAN | Offline and internal-lab modes attach `-nic none`. Restricted-internet mode uses user-mode NAT only; `bridge` and `tap` are asserted absent by test. |
| USB devices, GPUs, any physical hardware | No `usb-host`, no `vfio`, no passthrough of any kind. |
| The host filesystem via the terminal | The terminal is a serial console into the guest. Bytes in, bytes out. |
| The host filesystem via the File Tree panel | The agent refuses any path outside an allow-list of guest roots, checked after canonicalisation so `/home/..` is rejected. |

## What the guest deliberately *can* do

- Run as root, install packages, edit `/etc/passwd`, break its own boot, fill its own disk.
- Delete every file it can see, including the ones it needs to start again.

These are features. The mitigation is not restriction but disposability: every writable layer is a
qcow2 overlay stacked on a read-only base image, and discarding it is a file deletion.

## Trust boundaries

```
  Windows host
  ├── Desktop application ................ trusted
  │     ├── Lesson packages .............. UNTRUSTED input, schema + registry validated
  │     └── QEMU process ................. contained in a Job Object, no host access
  └── Guest (Debian) ..................... UNTRUSTED, fully disposable
        ├── linuxlab-agent (root) ........ trusted within the guest only
        └── The learner's shell .......... hostile by assumption
```

The interesting boundary is the innermost one. The agent runs as root in the guest and can mark
tasks complete, so anything the learner runs must not be able to drive it. Three things enforce
that:

1. **The channel is a virtio-serial character device**, not a network socket. It is reachable from
   the host process that created it, and there is no guest-side listener to connect to.
2. **Every request carries a 256-bit token** generated per VM run, compared in constant time. A
   frame with the wrong token is dropped.
3. **The token never lives in the shipped image.** It is passed on the kernel command line and
   moved to a root-only file at boot.

### A limitation we are explicit about

`/proc/cmdline` is world-readable, so a determined learner can read the control token out of it.
`linuxlab-boot` cannot change that; the kernel owns that file.

We accept this because of what the token actually authorises: preparing, validating and resetting
lessons *inside the same disposable guest*, plus reading diagnostics from it. The worst outcome is
that a learner marks their own exercises complete on their own machine, having worked out how to
read a kernel command line — which is arguably a pass in itself. The token grants nothing on the
host, and no host action is gated on it.

If lessons ever became gradeable for credit, the token would need to move to a channel the guest
cannot read, for example a second virtio-serial port written after boot.

## Lesson packages are untrusted input

A lesson package is JSON that names validators and shell commands. It is treated as untrusted
because it may have come from anywhere:

- Validated against the JSON Schema on load; a malformed package fails the whole catalogue load
  rather than being skipped, so a curriculum hole is visible.
- Every validator is checked against the embedded registry — unknown tag, unimplemented tag,
  missing parameter, misspelled parameter, wrong type — **twice**: on the host before sending, and
  in the guest before executing.
- A validator that cannot run reports an *errored* outcome, which never counts as a pass. A broken
  lesson cannot mark itself complete.
- Lesson ids and fixture names are sanitised before being used in a path, on both sides. Unit
  tests cover `../../etc/shadow` and `...` for each.

Lesson-authored shell commands do run as root — inside the guest. That is acceptable there and
would not be on the host, which is why no equivalent capability exists host-side.

## Process containment

QEMU is created **suspended**, placed in a Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`,
and only then resumed. The ordering matters: it closes the window in which a crash between spawn
and containment would orphan a virtual machine. No breakaway flag is set, so QEMU cannot place a
child outside the job.

Because the job handle is owned by the application process, the kernel terminates the guest even
if the application is killed outright. On the next launch, a recorded PID is only acted on if the
running image actually matches our QEMU — PIDs are recycled, and killing an unrelated process
would be a serious bug. There is a test for exactly that.

## Data handling

- No network calls exist in the application. There is no telemetry, no account and no backend.
- Command history is **off by default** and, when enabled, subject to a retention policy that is
  enforced on change as well as at startup, so reducing it deletes what was already stored.
- Exported transcripts are redacted **on the host**, not in the frontend, so the redaction cannot
  be bypassed. Keys matched: `password=`, `passwd=`, `token=`, `secret=`, `api_key=`, `apikey=`,
  `authorization:`, `auth_token=`, `private_key=`.
- The redactor is a single forward pass with ASCII case-insensitive matching, chosen so that
  lowercasing a copy cannot desynchronise the byte offsets and leak a value.

The documented lab password `linuxlab` is not treated as a secret. It appears in lessons on
purpose, and masking it would make the `sudo` lessons impossible to follow in an export.

## Frontend hardening

- Content Security Policy: `default-src 'self'`, no remote origins, `frame-src 'none'`,
  `object-src 'none'`.
- Tauri capabilities grant only events, three window operations and three dialog operations. The
  filesystem, shell and process plugins are **not** enabled.
- No clickable `file://` links in the terminal, and the web-links addon is deliberately not
  loaded. In an application whose entire purpose is running unvetted commands, turning their
  output into clickable links is not a risk worth taking for the convenience.
- Prototype freezing is on.

## Automated security tests

Implemented as unit tests, run by CI:

| Property | Where |
| --- | --- |
| No host directory sharing or device passthrough in the QEMU arguments | `crates/vm-manager/src/qemu.rs` |
| Every socket bound to `127.0.0.1` | `crates/vm-manager/src/qemu.rs` |
| No bridged or tap networking in any mode | `crates/vm-manager/src/qemu.rs` |
| The base image is never opened writable | `crates/vm-manager/src/qemu.rs` |
| Path traversal in a lesson id cannot escape the data directory | `crates/vm-manager/src/overlay.rs` |
| Path traversal in a lesson id cannot escape the lessons directory | `guest/linuxlab-agent/src/validators/mod.rs` |
| Directory browsing is confined to the allow-list, after canonicalisation | `guest/linuxlab-agent/src/handlers.rs` |
| An unknown or unimplemented validator errors and never passes | `guest/linuxlab-agent/src/validators/mod.rs` |
| Every implemented validator has a handler | `guest/linuxlab-agent/src/validators/mod.rs` |
| Unit names cannot inject `systemctl` flags | `guest/linuxlab-agent/src/validators/service.rs` |
| Token comparison rejects mismatches and length differences | `guest/linuxlab-agent/src/main.rs` |
| An empty control token is refused rather than allowed | `guest/linuxlab-agent/src/main.rs` |
| Solutions and answers are absent from the lesson payload | `apps/desktop/src-tauri/src/dto.rs` |
| Credentials are masked, and the mask is not re-masked | `crates/progress-store/src/redact.rs` |
| Stale-process cleanup will not kill an unrelated PID | `crates/vm-manager/src/process.rs` |

## Not covered

- **A QEMU vulnerability.** A guest-to-host escape through an emulated device would defeat the
  boundary. Mitigations: a minimal device model (`microvm`), no display, no audio, no USB, no
  passthrough, and a pinned, checksum-verified QEMU version that can be updated quickly.
- **A malicious lesson package from outside the repository.** Validated structurally, but a
  package that passes validation can still run arbitrary commands in the guest. There is no
  signing mechanism for third-party lesson packs, and one would be required before supporting them.
- **A compromised host.** Out of scope. Nothing an unprivileged application can do helps once the
  host is owned.
