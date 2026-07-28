# Contributing

Thanks for helping improve Linux Practice Lab. Bug reports, lesson corrections, accessibility
improvements, documentation, and focused code changes are welcome.

## Before opening a change

- Search existing issues and pull requests first.
- Use an issue for a substantial feature or architecture change before investing in an
  implementation.
- Keep pull requests focused. Separate curriculum, runtime, and unrelated UI changes when
  practical.
- Never commit built VM images, QEMU binaries, installers, local databases, control tokens, or
  learner data.

## Development setup

Host development requires Windows 10 22H2 or Windows 11, Node.js 20+, Rust 1.88+, and WebView2.

```powershell
npm ci
npm run verify
cargo test --workspace --exclude linuxlab-agent
```

The guest agent and image builder are Linux components:

```bash
cargo test -p linuxlab-agent
sudo guest/image-builder/build-rootfs.sh
```

Building the guest requires Linux root privileges. See the
[README](README.md#development), [architecture notes](docs/architecture/), and
[lesson authoring guide](docs/lesson-authoring/guide.md) before changing those areas.

## Quality gates

Run the checks relevant to your change before opening a pull request:

```powershell
npm run lessons:generate
npm run lessons:validate
npm run check --workspace apps/desktop
npm run test --workspace apps/desktop
cargo fmt --all -- --check
cargo test --workspace --exclude linuxlab-agent
```

On Linux, also run `cargo test -p linuxlab-agent`. Changes to VM startup, the guest image, runtime
packaging, or the control channel need the real QEMU boot test supplied by CI.

Generated lesson files must be regenerated and committed with the generator change. A validator
declared implemented must have a real guest handler; the coverage tests deliberately fail
otherwise.

## Pull requests

Describe what changed, why, how it was tested, and any learner-visible or security implications.
Screenshots are useful for UI changes. By contributing, you agree that your contribution is
licensed under GPL-2.0-or-later.

Please follow the [Code of Conduct](CODE_OF_CONDUCT.md).
