# Third-party notices

This notice groups the components distributed with Linux Practice Lab by licence family. The
generated runtime manifest is the exact file-by-file inventory; regenerate the dependency lists
with the `windows-build` workflow's SBOM step. The components shipped are a smaller set than those
used to build the application.

## Bundled binaries

| Component | Licence | Notes |
| --- | --- | --- |
| QEMU (`qemu-system-x86_64.exe`, `qemu-img.exe`) | GPL-2.0-only | Unmodified. See [qemu-source-offer.md](qemu-source-offer.md). |
| GLib (`libglib-2.0-0.dll` and friends) | LGPL-2.1-or-later | Unmodified, dynamically linked by QEMU. |
| zlib (`zlib1.dll`) | Zlib | Unmodified. |
| Zstandard (`libzstd.dll`) | BSD-3-Clause or GPL-2.0 | Unmodified. |
| GNU gettext runtime (`libintl-8.dll`) | LGPL-2.1-or-later | Unmodified. |
| libiconv (`libiconv-2.dll`) | LGPL-2.1-or-later | Unmodified. |
| PCRE2 (`libpcre2-8-0.dll`) | BSD-3-Clause | Unmodified. |
| winpthreads (`libwinpthread-1.dll`) | MIT and BSD-3-Clause | Part of mingw-w64. |
| GCC runtime (`libgcc_s_seh-1.dll`, `libstdc++-6.dll`, `libssp-0.dll`) | GPL-3.0-or-later **with the GCC Runtime Library Exception** | The exception is what permits distribution alongside GPL-2.0 QEMU. |

## Guest image

| Component | Licence |
| --- | --- |
| Linux kernel | GPL-2.0-only, with the syscall exception note |
| Debian base system | Per-package; `debian-copyright.txt` is generated during the image build |
| GNU coreutils, bash, grep, sed, gawk, findutils | GPL-3.0-or-later |
| util-linux, procps, psmisc | GPL-2.0-or-later and LGPL variants |
| systemd | LGPL-2.1-or-later |
| OpenSSH | BSD-style and public domain |
| nftables, iproute2 | GPL-2.0-only |
| ShellCheck | GPL-3.0-or-later |
| Python 3 | PSF-2.0 |

`debian-copyright.txt` must be regenerated for each shipped image. It is assembled from every
installed package's `/usr/share/doc/<package>/copyright`, which is the authoritative text.

## Application dependencies

### Frontend

| Component | Licence |
| --- | --- |
| Svelte | MIT |
| Vite | MIT |
| xterm.js (`@xterm/xterm` and addons) | MIT |
| Tauri JavaScript API | MIT or Apache-2.0 |

### Host

| Component | Licence |
| --- | --- |
| Tauri | MIT or Apache-2.0 |
| serde, serde_json | MIT or Apache-2.0 |
| tokio | MIT |
| rusqlite | MIT |
| SQLite (bundled through rusqlite) | Public domain |
| anyhow, thiserror | MIT or Apache-2.0 |
| tracing | MIT |
| sha2, hex | MIT or Apache-2.0 |
| zstd Rust bindings | MIT |
| rand | MIT or Apache-2.0 |
| windows-sys | MIT or Apache-2.0 |
| regex (guest agent) | MIT or Apache-2.0 |

## Why the application itself is GPL-2.0-or-later

Not because the licence of a separate process propagates — QEMU is invoked as its own process over
a documented protocol, which is generally understood not to create a derivative work. The reason is
practical: the application is useless without the QEMU it ships, the two are distributed together
as one product, and a licence argument is a poor thing to hand a learner. Choosing the stricter
reading costs nothing here.

## Obligations, and where each is met

| Obligation | Where |
| --- | --- |
| Include the GPL-2.0 text | `LICENSE`, and `licences/` inside the installer |
| Offer corresponding source for QEMU | [qemu-source-offer.md](qemu-source-offer.md), shipped in every release |
| Include LGPL notices for dynamically linked libraries | This file, shipped in `licences/` |
| State that QEMU is unmodified | [qemu-source-offer.md](qemu-source-offer.md) |
| Per-package copyright for the guest image | `debian-copyright.txt`, generated at image build |
| Do not misrepresent origin | The About screen names QEMU and Debian explicitly |

Licence compliance is not deferred to release packaging. The release workflow fails if the source
offer is absent, so the obligation cannot be forgotten under deadline pressure.
