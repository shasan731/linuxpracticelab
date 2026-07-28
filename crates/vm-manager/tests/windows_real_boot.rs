#![cfg(windows)]

use anyhow::{bail, Context, Result};
use shared_types::{AgentRequest, AgentResponse, NetworkMode, RequestEnvelope, ResponseEnvelope};
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::process::Command;
use vm_manager::{RuntimePaths, SessionKind, VmManager};

/// Opt-in smoke test for the packaged Windows QEMU and real guest image.
///
/// Canonicalising the paths deliberately produces Windows `\\?\` prefixes. This is the
/// exact shape used by a portable install and guards against passing `//?/C:/...` to QEMU.
#[tokio::test]
#[ignore = "requires LPL_BOOT_RUNTIME, LPL_BOOT_BASE and LPL_BOOT_DATA plus the packaged guest"]
async fn packaged_guest_boots_from_canonical_windows_paths() -> Result<()> {
    let runtime = canonical_env("LPL_BOOT_RUNTIME")?;
    let base_image = canonical_env("LPL_BOOT_BASE")?;
    let data_root = canonical_env("LPL_BOOT_DATA")?;
    let data_dir = data_root.join(format!("run-{}", std::process::id()));
    std::fs::create_dir_all(&data_dir)?;
    let data_dir = std::fs::canonicalize(data_dir)?;
    let log_dir = data_dir.join("logs");
    std::fs::create_dir_all(&log_dir)?;

    assert!(
        runtime.to_string_lossy().starts_with(r"\\?\"),
        "test paths must exercise the extended-length Windows form"
    );

    // Reproduce overlays written by builds that converted `\\?\F:\...` to `//?/F:/...`.
    // `prepare` must rebase this metadata without erasing learner sectors.
    let qemu_img = runtime.join("qemu-img.exe");
    let overlay = data_dir.join("free-practice.qcow2");
    create_legacy_overlay(&qemu_img, &base_image, &overlay).await?;

    let mut manager = VmManager::new(RuntimePaths {
        qemu_system: runtime.join("qemu-system-x86_64.exe"),
        qemu_img: qemu_img.clone(),
        kernel: runtime.join("vmlinuz"),
        initrd: Some(runtime.join("initrd.img")),
        base_image,
        data_dir: data_dir.clone(),
        log_dir,
    });
    if std::env::var("LPL_BOOT_MACHINE").as_deref() == Ok("q35") {
        manager.use_fallback_machine();
    }
    let config = manager
        .prepare(&SessionKind::FreePractice, NetworkMode::Disabled, None)
        .await?;
    assert_rebased(&qemu_img, &overlay).await?;
    manager.start(&config).await?;
    println!(
        "booting with {:?} on {:?} from {}",
        config.accel,
        config.machine,
        config.overlay.display()
    );
    let console = capture_console(config.console_port).await?;

    let ready = wait_for_pong(&mut manager, config.agent_port, &config.control_token).await;
    let stop_result = manager.stop().await;
    let console_output = tokio::time::timeout(Duration::from_secs(3), console)
        .await
        .ok()
        .and_then(|result| result.ok())
        .and_then(|result| result.ok())
        .unwrap_or_default();
    let response = ready.with_context(|| {
        format!(
            "console tail:\n{}",
            console_output
                .chars()
                .rev()
                .take(4000)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>()
        )
    })?;
    stop_result?;
    assert!(
        matches!(response, AgentResponse::Pong { .. }),
        "unexpected guest response: {response:?}"
    );
    std::fs::remove_dir_all(&data_dir).context("could not remove the completed smoke-test run")?;
    Ok(())
}

async fn capture_console(port: u16) -> Result<tokio::task::JoinHandle<std::io::Result<String>>> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    let stream = loop {
        match TcpStream::connect(("127.0.0.1", port)).await {
            Ok(stream) => break stream,
            Err(error) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(50)).await;
                let _ = error;
            }
            Err(error) => return Err(error.into()),
        }
    };
    Ok(tokio::spawn(async move {
        let mut stream = stream;
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).await?;
        Ok(String::from_utf8_lossy(&bytes).to_string())
    }))
}

async fn create_legacy_overlay(
    qemu_img: &PathBuf,
    base: &PathBuf,
    overlay: &PathBuf,
) -> Result<()> {
    let legacy_backing = base.to_string_lossy().replace('\\', "/");
    assert!(legacy_backing.starts_with("//?/"), "{legacy_backing}");
    let output = Command::new(qemu_img)
        .args(["create", "-q", "-f", "qcow2", "-F", "raw", "-b"])
        .arg(legacy_backing)
        .arg(qemu_path(overlay))
        .output()
        .await?;
    if !output.status.success() {
        bail!(
            "could not create the legacy overlay: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

async fn assert_rebased(qemu_img: &PathBuf, overlay: &PathBuf) -> Result<()> {
    let output = Command::new(qemu_img)
        .args(["info", "--output=json"])
        .arg(qemu_path(overlay))
        .output()
        .await?;
    let info: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let backing = info["backing-filename"].as_str().unwrap_or_default();
    assert!(!backing.starts_with("//?/"), "{backing}");
    Ok(())
}

fn qemu_path(path: &PathBuf) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .trim_start_matches("//?/")
        .to_string()
}

fn canonical_env(name: &str) -> Result<PathBuf> {
    let value = std::env::var_os(name).with_context(|| format!("{name} is not set"))?;
    std::fs::canonicalize(value).with_context(|| format!("could not canonicalise {name}"))
}

async fn wait_for_pong(manager: &mut VmManager, port: u16, token: &str) -> Result<AgentResponse> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(150);
    let mut last_error = None;
    while tokio::time::Instant::now() < deadline {
        match ping(port, token).await {
            Ok(response @ AgentResponse::Pong { .. }) => return Ok(response),
            Ok(response) => last_error = Some(format!("unexpected response: {response:?}")),
            Err(error) => last_error = Some(error.to_string()),
        }
        if let Some(status) = manager.poll_exited()? {
            bail!("QEMU exited during guest boot ({status})");
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
    bail!(
        "guest did not answer before the boot deadline{}",
        last_error
            .map(|error| format!("; last error: {error}"))
            .unwrap_or_default()
    )
}

async fn ping(port: u16, token: &str) -> Result<AgentResponse> {
    let stream = tokio::time::timeout(
        Duration::from_secs(2),
        TcpStream::connect(("127.0.0.1", port)),
    )
    .await
    .context("agent connection timed out")??;
    let (read_half, mut write_half) = stream.into_split();
    let request = RequestEnvelope {
        id: 1,
        token: token.to_string(),
        request: AgentRequest::Ping,
    };
    let mut frame = serde_json::to_vec(&request)?;
    frame.push(b'\n');
    write_half.write_all(&frame).await?;

    let mut line = String::new();
    tokio::time::timeout(
        Duration::from_secs(2),
        BufReader::new(read_half).read_line(&mut line),
    )
    .await
    .context("agent response timed out")??;
    let response: ResponseEnvelope = serde_json::from_str(&line)?;
    Ok(response.response)
}
