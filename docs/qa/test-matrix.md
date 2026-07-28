# QA test matrix

Automated tests cover logic. This matrix covers the things that only break on a real Windows
machine with a real virtual machine, which is where this product's characteristic failures live.

## Automated, run by CI

| Area | Command |
| --- | --- |
| Lesson packages | `npm run lessons:validate` |
| Frontend logic | `npm run test --workspace apps/desktop` |
| Frontend types | `npm run check --workspace apps/desktop` |
| Rust, all crates | `cargo test --workspace` |
| Lint and format | `cargo clippy --workspace --all-targets`, `cargo fmt --check` |
| Guest boot | the `guest-image` workflow boots the image and waits for a login prompt |
| Lesson solutions | `npm run lessons:solutions` against a live guest |

## Manual: installation

| # | Case | Expected |
| --- | --- | --- |
| 1 | Install as a standard user with no administrator rights | Installs to `%LOCALAPPDATA%\Programs`, no UAC prompt at any point |
| 2 | Install over an existing older version | Progress, snapshots and settings survive; the runtime directory is new and versioned |
| 3 | Uninstall | The application and runtime are removed; the learner is asked before progress is deleted |
| 4 | Portable build launched from a USB stick | Runs; data goes beside the executable, not into `%LOCALAPPDATA%` |
| 5 | Install into a path containing spaces and non-ASCII characters | Starts; QEMU launches; the terminal works |
| 6 | Windows account name with non-ASCII characters | Paths resolve; SQLite opens; overlays are created |

## Manual: first run and startup

| # | Case | Expected |
| --- | --- | --- |
| 7 | First launch, cold | A terminal appears in under 10 s on baseline hardware |
| 8 | Second launch, warm | Under 4 s |
| 9 | Launch with Windows Hypervisor Platform enabled | Status bar reports hardware acceleration |
| 10 | Launch with it disabled | Reports software translation, still works, **never asks the learner to change a Windows setting** |
| 11 | Launch with under 2 GB free disk | Blocked with a finding naming the shortfall and a recheck action |
| 12 | Delete a runtime file, then launch | Blocked with a finding naming the file and offering Reinstall Runtime |
| 13 | Corrupt a runtime file without changing its size | Full verification catches it; the quick check does not, by design |
| 14 | Kill the application from Task Manager, then relaunch | The unclean shutdown is reported as a warning, not a block; no QEMU survives |

## Manual: the terminal

| # | Case | Expected |
| --- | --- | --- |
| 15 | Ctrl+C during `sleep 300` | Interrupts; the prompt returns |
| 16 | Ctrl+D at an empty prompt | The shell exits and a fresh one starts |
| 17 | Ctrl+R history search | Works; it is Bash doing it, not the application |
| 18 | Tab completion, single and ambiguous | Completes; a double Tab lists candidates |
| 19 | Resize the window, then run `htop` | The guest learns the new size; full-screen output is not corrupted |
| 20 | Drag the lesson split to both extremes | Neither panel disappears; the terminal reflows |
| 21 | Copy and paste, including multi-line | Works; a pasted newline runs the command as it would in any terminal |
| 22 | Output a large file with `cat` | Scrollback stays bounded; no runaway memory growth |
| 23 | Non-ASCII output: `echo "héllo → 世界"` | Renders correctly, including across a read-chunk boundary |
| 24 | `less` on a long file | Paging works; `q` quits |
| 25 | Download the session transcript | Saved; anything resembling a credential is masked |

## Manual: lessons

| # | Case | Expected |
| --- | --- | --- |
| 26 | Pass a task with the suggested solution | Accepted |
| 27 | Pass it a different valid way | Accepted |
| 28 | Type the suggested command in the wrong directory | Rejected, reported as *wrong working directory* |
| 29 | Complete half a multi-check task | Reported as partially complete with a percentage, not a flat failure |
| 30 | Open every hint, then the solution | Each hint arrives one at a time; mastery drops to *needs review* |
| 31 | Reveal a solution and pass | Recorded as *needs review*, and the learner still had to type it |
| 32 | Reset a task | The environment returns to its prepared state |
| 33 | Restart a lesson | The overlay is discarded and recreated; attempt state is cleared |
| 34 | Guided path with unmet prerequisites | The lesson is locked and the reason is named |
| 35 | Switch to Open Library | The same lesson opens with a warning rather than a lock |
| 36 | Assessment mode | No hints, no syntax, no worked examples |
| 37 | Revisit a mastered lesson and fail | Stored mastery does not drop |
| 38 | Open developer tools and inspect the lesson payload | No solutions, no alternate solutions, no review answers, no hint text |

## Manual: recovery and destruction

| # | Case | Expected |
| --- | --- | --- |
| 39 | `sudo rm -rf --no-preserve-root /` in Dangerous Mode | The guest dies; the UI reports it is no longer bootable and offers restore |
| 40 | Restore the last snapshot afterwards | The environment comes back |
| 41 | Create a fresh environment afterwards | A clean guest boots |
| 42 | Enable Dangerous Mode | A recovery snapshot is created **before** anything destructive is allowed |
| 43 | Factory reset Free Practice | A snapshot is kept first; the next start builds a clean environment |
| 44 | Fill the guest disk with `dd` | The guest reports no space; the host stays healthy; the panel shows byte and inode usage separately |
| 45 | Delete `/bin/bash` in Dangerous Mode | The shell dies; restore works |
| 46 | Corrupt the Free Practice overlay by hand | Detected before reuse; Repair is offered instead of a crash |

## Manual: host isolation — these must all fail from inside the guest

| # | Attempt | Expected |
| --- | --- | --- |
| 47 | `ls /mnt`, `ls /media`, `mount \| grep -i windows` | Nothing from the host |
| 48 | `cat /proc/mounts` | Only guest filesystems |
| 49 | `ip addr` in offline mode | Loopback only |
| 50 | `ping 8.8.8.8` in offline mode | No route; no packets leave the machine |
| 51 | `ss -tulpn` | Only guest services; the QMP and console ports are not visible in the guest |
| 52 | `curl http://127.0.0.1:<qmp port>` from the guest | Refused; the guest has no route to the host |
| 53 | `lsusb`, `lspci` | No physical devices |
| 54 | Look for the Windows user name anywhere in the guest environment | Absent |

## Manual: Windows integration

| # | Case | Expected |
| --- | --- | --- |
| 55 | Sleep and resume with the VM running | The guest recovers or is reported clearly; no silent hang |
| 56 | Shut Windows down with the VM running | No orphaned QEMU on the next boot |
| 57 | High DPI, 150% and 200% scaling | Layout and terminal are readable and correctly sized |
| 58 | Move the window between monitors of different DPI | The terminal re-fits |
| 59 | Light and dark theme | Both readable; contrast holds |
| 60 | Keyboard-only navigation of every screen | Every control reachable with a visible focus ring |
| 61 | Screen reader on the lesson player | Instructions, requirements and validation results are announced |
| 62 | Antivirus real-time scanning enabled | Installation completes; if a file is quarantined, the health check names it |
| 63 | 4 GB total system RAM | Runs acceptably |
| 64 | Two instances launched at once | The session lock is respected; overlays are not corrupted |

## Performance budgets

Measure on the baseline: 4 GB RAM, a spinning disk or slow SSD, software translation.

| Metric | Target |
| --- | --- |
| Installer size | 250–500 MB |
| Installed size | 500 MB – 1 GB |
| Guest RAM | 256 MB default, 384 MB for advanced labs |
| Total process RAM | 300–500 MB |
| Cold terminal | under 10 s |
| Warm terminal | under 4 s |
| Lesson reset | under 3 s |
| Administrator rights | never required |
| Internet | never required for the core curriculum |
