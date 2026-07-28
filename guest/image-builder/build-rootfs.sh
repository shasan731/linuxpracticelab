#!/usr/bin/env bash
#
# Builds the read-only Debian 13 base image for Linux Practice Lab.
#
# Runs on Linux (the guest-image CI workflow), needs root for debootstrap and loop mounts, and
# produces a reproducible raw ext4 image plus its compressed form and checksums.
#
# Reproducibility matters for two reasons: the runtime manifest records a hash of this image
# and refuses to boot a mismatch, and a learner reporting a problem needs to be talking about
# the same bytes we are.

set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
GUEST_DIR="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
readonly GUEST_DIR
REPO_DIR="$(cd -- "${GUEST_DIR}/.." && pwd)"
readonly REPO_DIR

# Pinned so the image does not silently change under us. Bump deliberately, with a changelog
# entry, because it invalidates every shipped checksum.
readonly DEBIAN_SUITE="${DEBIAN_SUITE:-trixie}"
readonly DEBIAN_SNAPSHOT="${DEBIAN_SNAPSHOT:-20260711T000000Z}"
readonly DEBIAN_MIRROR="${DEBIAN_MIRROR:-https://snapshot.debian.org/archive/debian/${DEBIAN_SNAPSHOT}}"
readonly IMAGE_VERSION="${IMAGE_VERSION:-debian-13-trixie-2}"

readonly WORK_DIR="${WORK_DIR:-${GUEST_DIR}/image-builder/work}"
readonly OUT_DIR="${OUT_DIR:-${GUEST_DIR}/out}"
readonly ROOTFS="${WORK_DIR}/rootfs"
readonly IMAGE="${OUT_DIR}/debian-base.raw"

# 3 GiB gives room for the offline repository, the optional packages and a learner filling the
# disk on purpose during a disk-full lesson. It is sparse, so the shipped size is far smaller.
readonly IMAGE_SIZE_MB="${IMAGE_SIZE_MB:-3072}"

# Fixed timestamp for anything that would otherwise embed "now".
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-1783728000}"

log()  { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m warn\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31merror\033[0m %s\n' "$*" >&2; exit 1; }

cleanup() {
    local status=$?
    # Unmount in reverse order; ignore failures so cleanup never masks the real error.
    for mount in "${ROOTFS}/dev/pts" "${ROOTFS}/dev" "${ROOTFS}/proc" "${ROOTFS}/sys" "${ROOTFS}"; do
        if mountpoint -q "${mount}" 2>/dev/null; then
            umount -l "${mount}" 2>/dev/null || true
        fi
    done
    return "${status}"
}
trap cleanup EXIT

require_root() {
    [[ "${EUID}" -eq 0 ]] || die "this script needs root for debootstrap and loop mounts"
}

require_tools() {
    local missing=()
    for tool in debootstrap mkfs.ext4 zstd sha256sum chroot; do
        command -v "${tool}" >/dev/null 2>&1 || missing+=("${tool}")
    done
    (( ${#missing[@]} == 0 )) || die "missing required tools: ${missing[*]}"
}

read_package_list() {
    # Strips comments and blank lines, joins with commas for debootstrap.
    sed -e 's/#.*$//' -e '/^[[:space:]]*$/d' -e 's/[[:space:]]//g' "$1" | paste -sd, -
}

bootstrap_base() {
    log "bootstrapping Debian ${DEBIAN_SUITE} (minbase)"
    rm -rf "${ROOTFS}"
    mkdir -p "${ROOTFS}"

    # minbase keeps the image small; everything else is installed explicitly so the package
    # set is a decision rather than a default.
    debootstrap \
        --variant=minbase \
        --arch=amd64 \
        --include="$(read_package_list "${GUEST_DIR}/package-list/required.txt")" \
        "${DEBIAN_SUITE}" \
        "${ROOTFS}" \
        "${DEBIAN_MIRROR}"

    # Snapshot Release files eventually expire by design. The timestamped URL already pins
    # the exact bytes, so validity age is irrelevant and must not make later apt steps flaky.
    cat > "${ROOTFS}/etc/apt/apt.conf.d/99linuxlab-snapshot" <<'APT'
Acquire::Check-Valid-Until "false";
APT
}

mount_pseudo_filesystems() {
    mount -t proc proc "${ROOTFS}/proc"
    mount -t sysfs sys "${ROOTFS}/sys"
    mount -o bind /dev "${ROOTFS}/dev"
    mount -t devpts devpts "${ROOTFS}/dev/pts"
}

in_chroot() {
    # DEBIAN_FRONTEND stops packages asking questions with no terminal to answer them.
    chroot "${ROOTFS}" /usr/bin/env \
        DEBIAN_FRONTEND=noninteractive \
        LC_ALL=C \
        PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
        "$@"
}

install_optional_packages() {
    if [[ "${INCLUDE_OPTIONAL_PACKAGES:-0}" != "1" ]]; then
        log "skipping post-MVP optional packages"
        return 0
    fi
    log "installing optional packages"
    local packages
    packages="$(sed -e 's/#.*$//' -e '/^[[:space:]]*$/d' "${GUEST_DIR}/package-list/optional.txt" | tr '\n' ' ')"
    # shellcheck disable=SC2086  # deliberate word splitting into separate package arguments
    in_chroot apt-get install -y --no-install-recommends ${packages}
}

create_users() {
    log "creating lab users"
    # Fixed uids keep file ownership stable across image rebuilds, which matters because
    # lesson fixtures ship with owners baked in.
    in_chroot bash -s <<'CHROOT'
set -Eeuo pipefail

add_user() {
    local name="$1" uid="$2" comment="$3"
    if ! id -u "${name}" >/dev/null 2>&1; then
        useradd --create-home --uid "${uid}" --shell /bin/bash --comment "${comment}" "${name}"
    fi
}

add_user student    1000 "Practice account"
add_user instructor 1001 "Lesson author account"
add_user serviceuser 1002 "Owns lab services"
add_user analyst    1003 "Log analysis account"
add_user guest      1004 "Unprivileged visitor"

# The lab password is documented in lessons that need sudo, so it is deliberately not secret.
for account in student instructor serviceuser analyst guest; do
    echo "${account}:linuxlab" | chpasswd
done

# root has no usable password: everything privileged goes through sudo, which is what the
# curriculum teaches.
passwd --lock root

groupadd --force developers
CHROOT
}

configure_system() {
    log "applying overlay files"
    # Overlay files are the checked-in guest configuration: prompt, motd, systemd units,
    # sudoers policy, apt pinning for the offline repository.
    cp -a "${GUEST_DIR}/overlay-files/." "${ROOTFS}/"
    # Git preserves executable bits on Linux, but Windows bind mounts commonly present every
    # file as mode 0777. Normalise security-sensitive guest configuration explicitly so a
    # local Docker build is identical to CI and systemd never accepts world-writable units.
    find \
        "${ROOTFS}/etc/apt/sources.list.d" \
        "${ROOTFS}/etc/profile.d" \
        "${ROOTFS}/etc/systemd/system" \
        "${ROOTFS}/usr/local/lib/linuxlab" \
        -type d -exec chmod 0755 {} +
    chmod 0644 \
        "${ROOTFS}/etc/motd" \
        "${ROOTFS}/etc/apt/sources.list.d/linuxlab-offline.list" \
        "${ROOTFS}/etc/profile.d/linuxlab.sh" \
        "${ROOTFS}/etc/systemd/system/linuxlab-agent.service" \
        "${ROOTFS}/etc/systemd/system/linuxlab-boot.service" \
        "${ROOTFS}/etc/systemd/system/ssh.service.d/linuxlab-host-keys.conf"
    chmod 0755 "${ROOTFS}/usr/local/lib/linuxlab/linuxlab-boot"

    log "configuring the base system"
    in_chroot bash -s <<'CHROOT'
set -Eeuo pipefail

echo linuxlab > /etc/hostname
cat > /etc/hosts <<'HOSTS'
127.0.0.1   localhost linuxlab
::1         localhost ip6-localhost ip6-loopback
HOSTS

# Debian's compact vim-tiny package installs `vi` and `vim.tiny`, but not the `vim` spelling
# used by the curriculum. Keep the small editor while making the taught command available.
ln -sfn /usr/bin/vim.tiny /usr/local/bin/vim

# The serial console is the terminal the learner sees. Auto-login as student so opening the
# app lands straight on a prompt with no password step.
mkdir -p /etc/systemd/system/serial-getty@ttyS0.service.d
cat > /etc/systemd/system/serial-getty@ttyS0.service.d/autologin.conf <<'UNIT'
[Service]
ExecStart=
ExecStart=-/sbin/agetty --autologin student --noclear --keep-baud 115200,38400,9600 %I $TERM
UNIT

systemctl enable serial-getty@ttyS0.service
systemctl enable linuxlab-agent.service
systemctl enable linuxlab-boot.service

# Trim boot time: none of these earn their seconds in a disposable practice VM.
systemctl disable apt-daily.timer apt-daily-upgrade.timer 2>/dev/null || true
systemctl disable man-db.timer 2>/dev/null || true
systemctl disable ssh.service ssh.socket 2>/dev/null || true
systemctl mask systemd-random-seed.service 2>/dev/null || true
systemctl mask systemd-timesyncd.service 2>/dev/null || true
systemctl mask e2scrub_all.timer 2>/dev/null || true

# No network at all in the default profile, so do not wait for one.
systemctl mask systemd-networkd-wait-online.service 2>/dev/null || true

# This generator only creates SSH sockets for virtual machines with AF_VSOCK. Linux Practice
# Lab deliberately has no vsock device, so masking it avoids a noisy failed probe on ttyS0.
mkdir -p /etc/systemd/system-generators
ln -sfn /dev/null /etc/systemd/system-generators/systemd-ssh-generator

# The root filesystem is the qcow2 overlay; fsck on every boot costs seconds for no benefit.
cat > /etc/fstab <<'FSTAB'
/dev/vda  /         ext4  defaults,noatime  0 0
tmpfs     /run      tmpfs defaults,nosuid,nodev,mode=755 0 0
tmpfs     /tmp      tmpfs defaults,nosuid,nodev 0 0
FSTAB

# man pages are part of the curriculum, so the database has to exist.
mandb --create --quiet 2>/dev/null || true
CHROOT
}

install_agent() {
    log "installing the LinuxLab agent"
    local agent_binary="${AGENT_BINARY:-${REPO_DIR}/target/x86_64-unknown-linux-gnu/release/linuxlab-agent}"
    if [[ ! -f "${agent_binary}" ]]; then
        # Fall back to a host-target build, which is what a local `cargo build --release` makes.
        agent_binary="${REPO_DIR}/target/release/linuxlab-agent"
    fi
    [[ -f "${agent_binary}" ]] || die "agent binary not found; build it with: cargo build --release -p linuxlab-agent"

    install -D -m 0755 "${agent_binary}" "${ROOTFS}/usr/local/lib/linuxlab/linuxlab-agent"
    install -d -m 0755 "${ROOTFS}/opt/linuxlab/bin"
    install -D -m 0755 "${GUEST_DIR}/network-labs/lab-net.sh" "${ROOTFS}/opt/linuxlab/bin/lab-net"
    install -D -m 0755 "${GUEST_DIR}/services/linuxlab-signal-trap" "${ROOTFS}/opt/linuxlab/bin/linuxlab-signal-trap"
    echo "${IMAGE_VERSION}" > "${ROOTFS}/opt/linuxlab/image-version"
}

build_offline_repository() {
    log "building the offline package repository"
    local repo="${ROOTFS}/opt/linuxlab/repository"
    mkdir -p "${repo}/pool"

    # Downloads the .deb files lessons install at run time, so `apt install` works offline.
    local lesson_packages=()
    mapfile -t lesson_packages < <(sed -e 's/#.*$//' -e '/^[[:space:]]*$/d' "${GUEST_DIR}/package-list/offline-repository.txt")
    if (( ${#lesson_packages[@]} == 0 )); then
        warn "no packages listed for the offline repository"
        return 0
    fi

    in_chroot bash -s "${lesson_packages[@]}" <<'CHROOT'
set -Eeuo pipefail
cd /opt/linuxlab/repository/pool
# --reinstall so a package already in the image is still fetched into the repository.
apt-get download "$@" 2>/dev/null || {
    echo "warning: some packages could not be downloaded into the offline repository" >&2
}
CHROOT

    in_chroot bash -s <<'CHROOT'
set -Eeuo pipefail
cd /opt/linuxlab/repository
# A flat repository is enough for a local file: source and needs no signing infrastructure;
# the sources.list entry marks it [trusted=yes] because it never leaves the image.
apt-get install -y --no-install-recommends dpkg-dev >/dev/null
dpkg-scanpackages --multiversion pool /dev/null > Packages 2>/dev/null
gzip -9 -c Packages > Packages.gz
apt-get purge -y dpkg-dev >/dev/null
apt-get autoremove -y >/dev/null
CHROOT
}

prepare_lesson_tree() {
    log "installing lesson assets"
    install -d -m 0755 "${ROOTFS}/opt/linuxlab/lessons"
    if [[ -d "${REPO_DIR}/lessons/fixtures" ]]; then
        # Fixtures are copied per lesson id so the agent's path confinement lines up with the
        # authoring layout, and are locked to root so learners cannot read the hidden cases.
        while IFS= read -r -d '' fixture_dir; do
            local lesson_id
            lesson_id="$(basename "${fixture_dir}")"
            install -d -m 0700 "${ROOTFS}/opt/linuxlab/lessons/${lesson_id}/fixtures"
            cp -a "${fixture_dir}/." "${ROOTFS}/opt/linuxlab/lessons/${lesson_id}/fixtures/"
        done < <(find "${REPO_DIR}/lessons/fixtures" -mindepth 1 -maxdepth 1 -type d -print0)
    fi
    if [[ -d "${REPO_DIR}/lessons/assets/setup" ]]; then
        cp -a "${REPO_DIR}/lessons/assets/setup/." "${ROOTFS}/opt/linuxlab/lessons/"
    fi
    chown -R root:root "${ROOTFS}/opt/linuxlab"
    chmod -R go-rwx "${ROOTFS}/opt/linuxlab/lessons"
}

collect_debian_licences() {
    log "collecting Debian package copyright notices"
    local notice="${OUT_DIR}/debian-copyright.txt"
    mkdir -p "${OUT_DIR}" "${ROOTFS}/opt/linuxlab/licences"
    : > "${notice}"

    while IFS= read -r -d '' copyright_file; do
        local relative="${copyright_file#"${ROOTFS}/"}"
        printf '\n===============================================================================\n' >> "${notice}"
        printf '%s\n' "${relative}" >> "${notice}"
        printf '===============================================================================\n\n' >> "${notice}"
        cat "${copyright_file}" >> "${notice}"
        printf '\n' >> "${notice}"
    done < <(find "${ROOTFS}/usr/share/doc" -mindepth 2 -maxdepth 2 -name copyright -type f -print0 | sort -z)

    [[ -s "${notice}" ]] || die "no Debian package copyright notices were found"
    install -m 0644 "${notice}" "${ROOTFS}/opt/linuxlab/licences/debian-copyright.txt"
}

shrink_image() {
    log "removing caches and build residue"
    in_chroot bash -s <<'CHROOT'
set -Eeuo pipefail
apt-get clean
rm -rf /var/lib/apt/lists/*
rm -rf /var/cache/apt/archives/*.deb
rm -rf /usr/share/doc/* /usr/share/info/*
# Keep man pages: they are curriculum material, unlike doc and info.
find /var/log -type f -exec truncate -s 0 {} +
rm -f /etc/machine-id /var/lib/dbus/machine-id
# An empty machine-id makes systemd generate a fresh one on first boot.
: > /etc/machine-id
# Host keys are regenerated on first boot so every install is not sharing one identity.
rm -f /etc/ssh/ssh_host_*
CHROOT
}

normalise_timestamps() {
    log "normalising filesystem timestamps"
    find "${ROOTFS}" -xdev -print0 |
        xargs -0 -r touch -h --date="@${SOURCE_DATE_EPOCH}"
}

pack_image() {
    log "creating the ext4 image"
    mkdir -p "${OUT_DIR}"
    rm -f "${IMAGE}" "${IMAGE}.zst"

    # Sparse allocation: the file reports 3 GiB but only occupies what is used.
    truncate -s "${IMAGE_SIZE_MB}M" "${IMAGE}"

    # -d populates from a directory with no loop mount and no root-owned mountpoint, and
    # -U/-E hash_seed make the result byte-reproducible.
    mkfs.ext4 \
        -F \
        -q \
        -L linuxlab \
        -U "b0b0b0b0-1111-2222-3333-444444444444" \
        -E "hash_seed=b0b0b0b0-1111-2222-3333-444444444444" \
        -O "^has_journal" \
        -d "${ROOTFS}" \
        "${IMAGE}"

    # The journal is re-enabled after population: mkfs with -d is faster without it, but a
    # practice VM that gets killed mid-write needs it.
    tune2fs -O has_journal "${IMAGE}" >/dev/null

    log "compressing with zstd"
    zstd -19 --long -T0 -q -o "${IMAGE}.zst" "${IMAGE}"

    log "writing checksums"
    ( cd "${OUT_DIR}" && sha256sum debian-base.raw debian-base.raw.zst > SHA256SUMS )
    local raw_sha raw_size
    raw_sha="$(awk '$2 == "debian-base.raw" { print $1 }' "${OUT_DIR}/SHA256SUMS")"
    raw_size="$(stat -c '%s' "${IMAGE}")"
    cat > "${OUT_DIR}/image-manifest.json" <<MANIFEST
{
  "imageVersion": "${IMAGE_VERSION}",
  "rawImage": {
    "path": "debian-base.raw",
    "sha256": "${raw_sha}",
    "sizeBytes": ${raw_size}
  }
}
MANIFEST

    printf '%s\n' "${IMAGE_VERSION}" > "${OUT_DIR}/image-version"

    log "done"
    ls -lh "${IMAGE}" "${IMAGE}.zst"
}

extract_kernel() {
    log "extracting the kernel for direct boot"
    # microvm boots the kernel directly, so it must be outside the image.
    local kernel initrd
    kernel="$(find "${ROOTFS}/boot" -name 'vmlinuz-*' | sort -V | tail -n1)"
    initrd="$(find "${ROOTFS}/boot" -name 'initrd.img-*' | sort -V | tail -n1)"

    if [[ -z "${kernel}" ]]; then
        # minbase has no kernel; install the cloud kernel, which is the smallest one with
        # virtio-mmio support built in.
        in_chroot apt-get install -y --no-install-recommends linux-image-cloud-amd64
        kernel="$(find "${ROOTFS}/boot" -name 'vmlinuz-*' | sort -V | tail -n1)"
        initrd="$(find "${ROOTFS}/boot" -name 'initrd.img-*' | sort -V | tail -n1)"
    fi
    [[ -n "${kernel}" ]] || die "no kernel found in the image"

    install -D -m 0644 "${kernel}" "${OUT_DIR}/vmlinuz"
    if [[ -n "${initrd}" ]]; then
        install -D -m 0644 "${initrd}" "${OUT_DIR}/initrd.img"
    else
        warn "no initrd found; the kernel must have virtio-blk and ext4 built in"
    fi
}

main() {
    require_root
    require_tools
    mkdir -p "${WORK_DIR}" "${OUT_DIR}"

    bootstrap_base
    mount_pseudo_filesystems
    install_optional_packages
    create_users
    configure_system
    install_agent
    build_offline_repository
    prepare_lesson_tree
    extract_kernel
    collect_debian_licences
    shrink_image
    cleanup
    normalise_timestamps
    pack_image
}

main "$@"
