//! QEMU command line construction.
//!
//! This module is deliberately pure: it turns a [`VmConfig`] into an argument vector and
//! nothing else. That keeps the security-relevant decisions — no display, no host
//! filesystem sharing, no bridged networking, loopback-only sockets — testable without
//! launching a hypervisor.

use shared_types::{MachineType, NetworkMode, VmConfig};
use std::path::Path;

use crate::qemu_path;

/// Virtio device suffix. microvm exposes virtio over MMIO with no PCI bus, so the device
/// names differ from the q35 fallback. Getting this wrong means the guest boots with no
/// disk, which looks like a corrupt image rather than a wrong flag.
fn virtio_suffix(machine: MachineType) -> &'static str {
    match machine {
        MachineType::Microvm => "device",
        MachineType::Q35 => "pci",
    }
}

fn path_arg(path: &Path) -> String {
    // QEMU accepts forward slashes on Windows and this avoids backslash escaping inside
    // comma-separated -drive property lists.
    qemu_path::render(path)
}

fn extend_args(output: &mut Vec<String>, args: &[&str]) {
    output.extend(args.iter().map(|argument| argument.to_string()));
}

/// Name of the virtio-serial port the guest agent listens on. Appears in the guest as
/// `/dev/virtio-ports/org.linuxlab.agent`.
pub const AGENT_PORT_NAME: &str = "org.linuxlab.agent";

/// Kernel command line. `panic=-1` combined with `-no-reboot` means a fatal guest fault
/// makes QEMU exit, which the host reads as "environment is no longer bootable" and offers
/// snapshot restore, instead of the learner staring at a hung console.
pub fn kernel_cmdline(cfg: &VmConfig) -> String {
    let root_device = match cfg.machine {
        MachineType::Microvm => "root=/dev/vda",
        // NVMe is built directly into the Debian cloud kernel. q35's former virtio-blk
        // root depended on an initrd module and intermittently never appeared under TCG.
        MachineType::Q35 => "root=/dev/nvme0n1",
    };
    let mut parts = vec![
        "console=ttyS0,115200".to_string(),
        root_device.to_string(),
        // Both PCI and MMIO block devices are discovered asynchronously. Waiting avoids an
        // immediate VFS panic if the selected transport is a little late under TCG.
        "rootwait".to_string(),
        "rw".to_string(),
        "rootfstype=ext4".to_string(),
        "init=/lib/systemd/systemd".to_string(),
        "systemd.show_status=0".to_string(),
        "loglevel=3".to_string(),
        "quiet".to_string(),
        // Saves ~1s of entropy wait on a cold boot inside a VM.
        "random.trust_cpu=on".to_string(),
        "reboot=t".to_string(),
        "panic=-1".to_string(),
    ];
    parts.push(format!(
        "linuxlab.network={}",
        network_label(cfg.network_mode)
    ));
    // The control token reaches the guest here because it is generated per VM run and so
    // cannot live in the shipped image. linuxlab-boot moves it to a root-only file on boot.
    // It authorises nothing beyond this disposable guest.
    parts.push(format!("linuxlab.token={}", cfg.control_token));
    parts.join(" ")
}

fn network_label(mode: NetworkMode) -> &'static str {
    match mode {
        NetworkMode::Disabled => "offline",
        NetworkMode::InternalLab => "internal-lab",
        NetworkMode::RestrictedInternet => "restricted-internet",
    }
}

/// Builds the full QEMU argument vector, excluding argv[0].
pub fn build_args(cfg: &VmConfig) -> Vec<String> {
    let suffix = virtio_suffix(cfg.machine);
    let mut a: Vec<String> = Vec::new();

    // Ignore any qemu.conf the host may have; the learner's environment must not change
    // how the lab boots.
    extend_args(&mut a, &["-no-user-config", "-nodefaults"]);
    extend_args(&mut a, &["-machine", cfg.machine.as_qemu_arg()]);
    extend_args(&mut a, &["-accel", cfg.accel.as_qemu_arg()]);
    // `max` gives the guest every feature the accelerator can expose. Under TCG this is
    // still a synthetic CPU, so the guest never sees host-specific model details.
    extend_args(&mut a, &["-cpu", "max"]);
    a.push("-smp".into());
    a.push(cfg.cpus.max(1).to_string());
    a.push("-m".into());
    a.push(cfg.memory_mb.to_string());
    extend_args(&mut a, &["-rtc", "base=utc"]);

    // Host isolation: nothing to look at, nothing to plug in, nothing to share.
    // Note we do *not* pass -nographic: it would rebind the serial port to QEMU's stdio and
    // steal the console away from the loopback chardev that xterm.js reads.
    extend_args(&mut a, &["-display", "none"]);
    a.push("-no-reboot".into());

    match cfg.machine {
        MachineType::Microvm => {
            // Modern virtio-mmio transport; legacy mode confuses recent Debian kernels.
            extend_args(&mut a, &["-global", "virtio-mmio.force-legacy=false"]);
        }
        MachineType::Q35 => {
            // microvm has no VGA adapter to suppress; q35 does.
            extend_args(&mut a, &["-vga", "none"]);
        }
    }

    // Direct kernel boot: no firmware, no bootloader, no disk probing.
    a.push("-kernel".into());
    a.push(path_arg(&cfg.kernel));
    if let Some(initrd) = &cfg.initrd {
        a.push("-initrd".into());
        a.push(path_arg(initrd));
    }
    a.push("-append".into());
    a.push(kernel_cmdline(cfg));

    // Root disk: a qcow2 overlay whose backing file is the read-only base image. The base
    // image path is recorded in the overlay itself, so it is never named on the command
    // line and cannot be written through.
    a.push("-drive".into());
    a.push(format!(
        "id=root,file={},format=qcow2,if=none,cache=writeback,discard=unmap,detect-zeroes=unmap",
        path_arg(&cfg.overlay)
    ));
    a.push("-device".into());
    a.push(match cfg.machine {
        MachineType::Microvm => format!("virtio-blk-{suffix},drive=root"),
        MachineType::Q35 => "nvme,drive=root,serial=linuxlab-root".to_string(),
    });

    // Serial console into xterm.js, over loopback only.
    a.push("-chardev".into());
    a.push(format!(
        "socket,id=console,host=127.0.0.1,port={},server=on,wait=off,telnet=off",
        cfg.console_port
    ));
    extend_args(&mut a, &["-serial", "chardev:console"]);

    // Agent control channel as a virtio-serial port. Using a character device instead of a
    // guest TCP port is what lets the whole offline curriculum run with no guest NIC at all.
    a.push("-device".into());
    a.push(format!("virtio-serial-{suffix},id=vser0"));
    a.push("-chardev".into());
    a.push(format!(
        "socket,id=agent,host=127.0.0.1,port={},server=on,wait=off",
        cfg.agent_port
    ));
    a.push("-device".into());
    a.push(format!(
        "virtserialport,bus=vser0.0,chardev=agent,name={AGENT_PORT_NAME}"
    ));

    // Lifecycle control.
    a.push("-qmp".into());
    a.push(format!("tcp:127.0.0.1:{},server=on,wait=off", cfg.qmp_port));

    match cfg.network_mode {
        // The internal laboratory is built from network namespaces *inside* the guest, so
        // even network lessons need no host interface. That is the whole reason ping and
        // routing behave deterministically.
        NetworkMode::Disabled | NetworkMode::InternalLab => {
            a.push("-nic".into());
            a.push("none".into());
        }
        NetworkMode::RestrictedInternet => {
            // User-mode NAT: unprivileged, no bridge, no access to the physical LAN.
            a.push("-netdev".into());
            a.push("user,id=net0,ipv6=off,restrict=off".into());
            a.push("-device".into());
            a.push(format!("virtio-net-{suffix},netdev=net0"));
        }
    }

    a.push("-D".into());
    a.push(path_arg(&cfg.log_file));

    a
}

/// Renders the argument vector for the log and the Environment panel, with the control
/// token redacted. The token never appears in argv today, but this stays defensive because
/// the arguments are surfaced to the user in diagnostics.
pub fn describe(cfg: &VmConfig) -> String {
    build_args(cfg)
        .into_iter()
        .map(|arg| {
            if !cfg.control_token.is_empty() && arg.contains(&cfg.control_token) {
                arg.replace(&cfg.control_token, "<redacted>")
            } else {
                arg
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared_types::{AccelMode, VmConfig};
    use std::path::PathBuf;

    fn cfg() -> VmConfig {
        VmConfig {
            machine: MachineType::Microvm,
            accel: AccelMode::Tcg,
            cpus: 1,
            memory_mb: 256,
            kernel: PathBuf::from(r"C:\rt\vmlinuz"),
            initrd: Some(PathBuf::from(r"C:\rt\initrd.img")),
            base_image: PathBuf::from(r"C:\rt\debian-base.raw"),
            overlay: PathBuf::from(r"C:\data\free-practice.qcow2"),
            network_mode: NetworkMode::Disabled,
            qmp_port: 45001,
            console_port: 45002,
            agent_port: 45003,
            control_token: "tok-abc123".into(),
            log_file: PathBuf::from(r"C:\data\logs\vm.log"),
        }
    }

    fn joined(cfg: &VmConfig) -> String {
        build_args(cfg).join(" ")
    }

    #[test]
    fn no_graphics_no_user_config() {
        let s = joined(&cfg());
        assert!(s.contains("-display none"));
        assert!(s.contains("-no-user-config"));
        assert!(s.contains("-nodefaults"));
        // -nographic would recapture the serial console onto QEMU's stdio.
        assert!(!s.contains("-nographic"));
    }

    #[test]
    fn vga_is_suppressed_only_where_a_vga_adapter_exists() {
        let mut c = cfg();
        assert!(
            !joined(&c).contains("-vga"),
            "microvm has no VGA to suppress"
        );
        c.machine = MachineType::Q35;
        assert!(joined(&c).contains("-vga none"));
    }

    #[test]
    fn sockets_are_bound_to_loopback_only() {
        let args = build_args(&cfg());
        let sockets: Vec<&String> = args
            .iter()
            .filter(|a| a.contains("socket,") || a.starts_with("tcp:"))
            .collect();
        assert!(!sockets.is_empty(), "expected chardev/qmp sockets");
        for s in sockets {
            assert!(s.contains("127.0.0.1"), "socket not bound to loopback: {s}");
        }
    }

    #[test]
    fn offline_and_internal_lab_attach_no_nic() {
        for mode in [NetworkMode::Disabled, NetworkMode::InternalLab] {
            let mut c = cfg();
            c.network_mode = mode;
            let s = joined(&c);
            assert!(s.contains("-nic none"), "{mode:?} should have no NIC: {s}");
            assert!(!s.contains("netdev"), "{mode:?} must not add a netdev");
        }
    }

    #[test]
    fn restricted_internet_uses_user_mode_nat_never_a_bridge() {
        let mut c = cfg();
        c.network_mode = NetworkMode::RestrictedInternet;
        let s = joined(&c);
        assert!(s.contains("user,id=net0"));
        assert!(!s.contains("bridge"), "bridged networking is forbidden");
        assert!(!s.contains("tap"), "tap devices are forbidden");
    }

    #[test]
    fn never_shares_a_host_directory_or_passes_through_devices() {
        let mut c = cfg();
        c.network_mode = NetworkMode::RestrictedInternet;
        let s = joined(&c);
        for forbidden in [
            "virtfs",
            "9p",
            "virtio-9p",
            "fsdev",
            "usb-host",
            "vfio",
            "hostfwd",
            "-device usb",
        ] {
            assert!(
                !s.contains(forbidden),
                "forbidden argument present: {forbidden}"
            );
        }
    }

    #[test]
    fn microvm_uses_mmio_devices_and_q35_uses_pci() {
        let mut c = cfg();
        c.network_mode = NetworkMode::RestrictedInternet;
        let micro = joined(&c);
        assert!(micro.contains("virtio-blk-device"));
        assert!(micro.contains("virtio-serial-device"));
        assert!(micro.contains("virtio-net-device"));
        assert!(micro.contains("virtio-mmio.force-legacy=false"));

        c.machine = MachineType::Q35;
        let q35 = joined(&c);
        assert!(q35.contains("nvme,drive=root,serial=linuxlab-root"));
        assert!(q35.contains("virtio-serial-pci"));
        assert!(q35.contains("virtio-net-pci"));
        assert!(!q35.contains("virtio-mmio"));
    }

    #[test]
    fn each_machine_names_the_root_device_its_disk_transport_creates() {
        let micro = cfg();
        assert!(kernel_cmdline(&micro).contains("root=/dev/vda"));

        let mut q35 = cfg();
        q35.machine = MachineType::Q35;
        assert!(kernel_cmdline(&q35).contains("root=/dev/nvme0n1"));
    }

    #[test]
    fn base_image_is_not_named_on_the_command_line() {
        // The overlay records its own backing file. Passing the base image as a drive is
        // how you accidentally give the guest a writable handle to it.
        let s = joined(&cfg());
        assert!(!s.contains("debian-base.raw"));
        assert!(s.contains("free-practice.qcow2"));
    }

    #[test]
    fn exactly_one_serial_is_configured() {
        let args = build_args(&cfg());
        let serials = args.iter().filter(|a| *a == "-serial").count();
        assert_eq!(serials, 1, "duplicate -serial would detach the console");
    }

    #[test]
    fn windows_paths_are_normalised_for_qemu_property_lists() {
        let args = build_args(&cfg());
        let drive = args.iter().find(|a| a.starts_with("id=root")).unwrap();
        assert!(drive.contains("C:/data/free-practice.qcow2"), "{drive}");
        assert!(!drive.contains('\\'));
    }

    #[test]
    fn agent_channel_is_a_virtio_serial_port_not_a_guest_tcp_port() {
        let s = joined(&cfg());
        assert!(s.contains(&format!("name={AGENT_PORT_NAME}")));
        // A guest-reachable forwarded port would break the offline promise.
        assert!(!s.contains("guestfwd"));
    }

    #[test]
    fn cmdline_makes_a_guest_panic_exit_qemu() {
        let c = cfg();
        let s = joined(&c);
        assert!(s.contains("-no-reboot"));
        assert!(kernel_cmdline(&c).contains("panic=-1"));
    }

    #[test]
    fn cmdline_waits_for_the_asynchronously_discovered_root_disk() {
        let cmdline = kernel_cmdline(&cfg());
        assert!(cmdline.split_whitespace().any(|part| part == "rootwait"));
    }

    #[test]
    fn cmdline_tells_the_guest_which_network_mode_it_is_in() {
        let mut c = cfg();
        assert!(kernel_cmdline(&c).contains("linuxlab.network=offline"));
        c.network_mode = NetworkMode::InternalLab;
        assert!(kernel_cmdline(&c).contains("linuxlab.network=internal-lab"));
    }

    #[test]
    fn zero_cpus_is_clamped_to_one() {
        let mut c = cfg();
        c.cpus = 0;
        let args = build_args(&c);
        let idx = args.iter().position(|a| a == "-smp").unwrap();
        assert_eq!(args[idx + 1], "1");
    }

    #[test]
    fn the_control_token_is_handed_to_the_guest_on_the_kernel_command_line() {
        let c = cfg();
        assert!(kernel_cmdline(&c).contains("linuxlab.token=tok-abc123"));
    }

    #[test]
    fn describe_redacts_the_control_token_wherever_it_appears() {
        // The token is genuinely in argv via -append, so diagnostics must scrub it.
        let c = cfg();
        let described = describe(&c);
        assert!(!described.contains("tok-abc123"), "{described}");
        assert!(
            described.contains("linuxlab.token=<redacted>"),
            "{described}"
        );
    }
}
