# Changelog

All notable changes to Linux Practice Lab are recorded here. The project follows
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.0] - 2026-07-28

### Added

- Offline-first Windows desktop application built with Tauri 2, Rust, Svelte 5, and xterm.js.
- A real Debian 13 virtual machine with persistent Free Practice and disposable lesson overlays.
- 71 lessons across orientation, terminal literacy, filesystem navigation, file management, text,
  search and comparison, and pipes/redirection/streams.
- One shared 70-validator registry with guest-state validation instead of command-text matching.
- Runtime integrity verification, crash recovery, snapshots, factory reset, and QEMU acceleration
  fallback.
- Reproducible guest-image tooling, Windows installer and portable packaging, release checksums,
  corresponding QEMU source, and continuous integration.

### Fixed

- Set the Rust toolchain floor to 1.88, which supports the Edition 2024 dependency manifests and
  stabilized let-chain syntax used by the locked dependency graph.
- Made release checksums use the flat asset filenames users actually download from GitHub.
- Preserved Unix executable modes for guest, network, service, and lesson setup scripts.
- Kept the Windows-only hardware-acceleration constructor out of non-Windows builds so strict
  cross-platform Clippy checks remain warning-free.
- Made the Windows real-boot test helpers accept borrowed paths without unnecessary `PathBuf`
  coupling.
- Updated guest validator lookups to current, allocation-free Rust idioms required by strict
  Clippy checks.

[Unreleased]: https://github.com/shasan731/linuxpracticelab/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/shasan731/linuxpracticelab/releases/tag/v0.1.0
