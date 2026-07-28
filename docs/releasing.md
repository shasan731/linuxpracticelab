# Release process

Linux Practice Lab uses semantic versions and signed Git tags are recommended when the maintainer
has a signing key. A tag matching `v*` is the only trigger for the release workflow.

## Before tagging

1. Update versions in the workspace, desktop package, Tauri configuration, and runtime manager.
2. Update `CHANGELOG.md`.
3. Run the application, curriculum, Rust, shell, JSON, and real QEMU boot checks.
4. Confirm `runtime/qemu-manifest.json` pins verified binary and source checksums.
5. Confirm the [GPL corresponding-source checklist](licensing/qemu-source-offer.md#checklist-before-publishing-a-release).
6. Build and test both the installer and portable archive on supported Windows.

## Publish

```powershell
git tag -a v0.1.0 -m "Linux Practice Lab v0.1.0"
git push origin main
git push origin v0.1.0
```

The release workflow validates version consistency, builds and boot-tests the Debian guest,
assembles the verified Windows runtime, builds the installer and portable package, generates an
SBOM and checksums, and publishes the GitHub release.

## Verify

- Download the published files and verify them against `SHA256SUMS`.
- Smoke-test the installer and portable package on a clean supported Windows user account.
- Confirm the release includes the exact QEMU source archive, license bundle, SBOM, and checksums.
- Confirm the release notes and `latest` pointer are correct.

Never attach a locally modified runtime to a release without regenerating its checksum inventory
and corresponding-source package.
