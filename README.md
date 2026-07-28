# Linux Practice Lab

[![Application tests](https://github.com/shasan731/linuxpracticelab/actions/workflows/app-tests.yml/badge.svg)](https://github.com/shasan731/linuxpracticelab/actions/workflows/app-tests.yml)
[![Latest release](https://img.shields.io/github/v/release/shasan731/linuxpracticelab)](https://github.com/shasan731/linuxpracticelab/releases/latest)
[![License](https://img.shields.io/github/license/shasan731/linuxpracticelab)](LICENSE)

Linux Practice Lab is an offline-first Windows desktop application for learning Linux in a real,
disposable Debian 13 virtual machine. It needs no WSL, VirtualBox, ISO installation, administrator
rights, or guest internet access.

The MVP is implemented end to end: the Tauri 2/Svelte 5 desktop app, QEMU lifecycle management,
first-run runtime installation, guest agent, state-based validation engine, progress storage,
reset and recovery controls, and all 71 lessons across the seven core modules.

## The central idea

A learner can type `mkdir reports`, `mkdir -p ~/reports`, or
`install -d /home/student/reports`. The application does not compare command text. It asks the
guest whether `/home/student/reports` exists and is a directory. Equivalent solutions pass, while
the suggested command run in the wrong directory fails.

The 70-validator registry lives in
[`lessons/schema/validators.json`](lessons/schema/validators.json). Authoring checks, host
pre-flight validation, and the guest agent all compile in that same file, so their understanding
of a task cannot drift silently.

## What is included

| Area | Implementation |
| --- | --- |
| Desktop | Tauri 2 host with a Svelte 5 interface and xterm.js terminal |
| Runtime | Pinned QEMU 9.2.0 Windows build, verified before packaging |
| Guest | Reproducible Debian 13 image built from a timestamped Debian snapshot |
| Isolation | qcow2 overlays, loopback-only control sockets, Windows Job Object containment |
| Validation | 70 state validators across files, processes, services, identity, networking, packages, and scripts |
| Curriculum | 71 MVP lessons in orientation, terminal literacy, navigation, file management, text, search, and streams |
| Persistence | Local SQLite progress, settings, notes, achievements, and optional command history |
| Distribution | NSIS installer plus a portable ZIP layout |

Modules beyond the seven-module MVP remain future course packs; they are not required for the
application described by the MVP specification.

## Download

Download the installer or portable ZIP from the
[latest GitHub release](https://github.com/shasan731/linuxpracticelab/releases/latest).

| Package | Best for |
| --- | --- |
| Windows installer (`.exe`) | A normal per-user installation with Start menu shortcuts |
| Portable ZIP | Running from a chosen folder or removable drive without installation |

The application supports 64-bit Windows 10 22H2 and Windows 11. Keep at least 4 GiB free for the
application, its expanded Debian image, and learner changes. The first runtime installation takes
longer than later starts because the verified 3 GiB base image must be materialized. The v0.1.0
build is unsigned, so Windows SmartScreen may ask you to confirm that you want to run it.

For portable use, extract the whole archive before starting `LinuxPracticeLab.exe`; do not run the
executable from inside the ZIP. Application data remains under `portable-data/` next to the
executable. Normal installations store progress separately from the replaceable runtime.

## Repository layout

```text
apps/desktop/           Svelte frontend and Tauri host application
crates/
  shared-types/         Shared protocol, lesson types, and validator registry
  vm-manager/           QEMU, QMP, overlays, acceleration, and process containment
  lesson-engine/        Catalogue, progression, hints, and feedback
  progress-store/       SQLite progress and privacy controls
  runtime-manager/      Runtime installation and integrity verification
guest/
  linuxlab-agent/       In-guest validation and control agent
  image-builder/        Debian image build
  overlay-files/        Guest configuration
  network-labs/         Deterministic namespace-based lab network
lessons/
  core/                 Seven core curriculum modules
  assets/setup/         Per-lesson setup/reset scripts
  schema/               Lesson schema and validator registry
runtime/                Pinned runtime metadata and assembled release payload
scripts/                Generation, validation, solution tests, and packaging
docs/                   Architecture, security, authoring, QA, and licensing
```

## Development

Required for host development:

- Windows 10 22H2 or Windows 11
- Node.js 20 or newer
- Rust 1.85 or newer
- WebView2 (present on supported Windows installations)

Install dependencies and run the normal checks:

```powershell
npm ci
npm run lessons:generate
npm run lessons:validate
npm run check --workspace apps/desktop
npm run test --workspace apps/desktop
cargo test --workspace --exclude linuxlab-agent
```

The guest agent targets Linux. Build and test it on Linux or in Docker:

```bash
cargo test -p linuxlab-agent
cargo build --release -p linuxlab-agent
```

### Build the guest

The image builder needs Linux root privileges. It uses the timestamped Debian snapshot declared
in `guest/image-builder/build-rootfs.sh`, creates a sparse 3 GiB raw image, and ships it compressed.

```bash
sudo guest/image-builder/build-rootfs.sh
```

The output includes the raw and compressed images, kernel, initrd, version metadata, hashes, and
the Debian package copyright bundle.

### Assemble and run the Windows application

Place the verified QEMU installer at `runtime/vendor/qemu-installer.exe` and guest artifacts under
`runtime/vendor/guest/`, then run:

```powershell
pwsh ./scripts/package-runtime.ps1
npm run tauri --workspace apps/desktop -- dev
```

The assembled installer payload is about 281 MiB. On first launch, the application copies and
verifies it in a staging directory, materializes the 3 GiB base image, verifies that image, and
atomically activates the runtime. Portable mode is enabled by placing `portable.mode` beside the
executable; its writable data then stays in `portable-data/` beside the app.

Build the release installer with:

```powershell
npm run tauri --workspace apps/desktop -- build
```

## Verification

The repository provides four CI workflows:

- application tests and cross-platform Rust checks;
- reproducible guest-image construction;
- a real QEMU boot plus authenticated guest-agent ping;
- Windows runtime assembly and desktop packaging.

`npm run lessons:solutions` is the curriculum release gate. Against a running guest it prepares
and resets every lesson, runs every suggested and alternate solution, and confirms that each
declared incorrect solution fails. Connection details are supplied through
`LINUXLAB_AGENT_PORT` and `LINUXLAB_CONTROL_TOKEN`.

## Design notes

**No guest NIC by default.** Offline lessons attach no network device. Network exercises use
namespaces inside the guest, keeping addresses and failures deterministic without exposing the
host LAN.

**Virtio-serial control channel.** The agent channel is not a guest TCP service. QEMU exposes the
character device through loopback-only host sockets, authenticated with a token generated for
each VM run.

**No host filesystem sharing.** QEMU receives no 9p, virtfs, USB passthrough, bridge, or host
directory. Learner changes land only in disposable or explicitly persistent qcow2 overlays.

**Acceleration is optional.** The app tries Windows Hypervisor Platform when available. If the
guest does not become ready within the accelerated boot budget, it automatically restarts with
QEMU software translation.

**Solutions stay out of the webview.** Suggested solutions, alternates, and correct review answers
are omitted from normal lesson payloads. Explicit reveal commands update mastery scoring.

**Reset is observable.** Lesson overlays are recreated from the verified base image. Free Practice
uses a separate persistent overlay and can be factory-reset independently.

## Documentation

- [Architecture](docs/architecture/)
- [Security model](docs/security/)
- [Lesson authoring guide](docs/lesson-authoring/)
- [QA matrix](docs/qa/)
- [Licensing and corresponding source](docs/licensing/)
- [Contributing](CONTRIBUTING.md)
- [Security policy](SECURITY.md)
- [Release process](docs/releasing.md)
- [Changelog](CHANGELOG.md)

## License

Linux Practice Lab is licensed under GPL-2.0-or-later; see [LICENSE](LICENSE). The bundled QEMU
runtime is GPL-2.0 software. Release artifacts include its license notices, exact binary
checksums, Debian package copyright material, and the corresponding-source offer described in
[`docs/licensing/qemu-source-offer.md`](docs/licensing/qemu-source-offer.md).
