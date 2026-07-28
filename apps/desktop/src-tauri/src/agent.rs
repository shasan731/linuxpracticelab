//! Host side of the guest control channel.
//!
//! QEMU exposes the guest's virtio-serial port as a loopback TCP socket, so this is a plain
//! newline-delimited JSON client. Requests are serialised through a mutex because the guest
//! agent handles one frame at a time and correlating interleaved replies would buy nothing.

use anyhow::{bail, Context, Result};
use shared_types::{AgentRequest, AgentResponse, RequestEnvelope, ResponseEnvelope};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

pub struct AgentClient {
    port: u16,
    token: String,
    next_id: AtomicU64,
    connection: Mutex<Option<Connection>>,
}

struct Connection {
    reader: BufReader<tokio::net::tcp::OwnedReadHalf>,
    writer: tokio::net::tcp::OwnedWriteHalf,
}

impl AgentClient {
    pub fn new(port: u16, token: impl Into<String>) -> Self {
        Self {
            port,
            token: token.into(),
            next_id: AtomicU64::new(1),
            connection: Mutex::new(None),
        }
    }

    /// Polls until the guest agent answers, or the deadline passes.
    ///
    /// A guest takes several seconds to boot, longer under software translation, and the
    /// character device refuses connections until QEMU has created it. So "not ready yet" is
    /// the normal case here and must not be reported as a failure.
    pub async fn wait_until_ready(&self, timeout: Duration) -> Result<AgentResponse> {
        let deadline = tokio::time::Instant::now() + timeout;
        let mut last_error = None;

        while tokio::time::Instant::now() < deadline {
            // QEMU accepts the host socket before Linux opens the virtio port. Keep that first
            // connection and ping alive: QEMU buffers the frame and the agent answers as soon as
            // it starts. Reconnecting every couple of seconds leaves stale pings queued behind
            // QEMU's character device and can add tens of seconds after Linux is already usable.
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let attempt =
                tokio::time::timeout(remaining, self.try_request(&AgentRequest::Ping, remaining))
                    .await;
            match attempt {
                Ok(Ok(response @ AgentResponse::Pong { .. })) => return Ok(response),
                Ok(Ok(AgentResponse::Error { message, .. })) => last_error = Some(message),
                Ok(Ok(other)) => last_error = Some(format!("unexpected reply to ping: {other:?}")),
                Ok(Err(err)) => last_error = Some(err.to_string()),
                Err(_) => last_error = Some("the guest did not reply in time".into()),
            }

            // A refused or dropped socket can be retried, but never reuse a connection that may
            // contain a timed-out ping and its eventual stale reply.
            *self.connection.lock().await = None;
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            tokio::time::sleep(remaining.min(Duration::from_millis(150))).await;
        }

        bail!(
            "Linux did not finish starting in time{}",
            last_error
                .map(|detail| format!(" (last error: {detail})"))
                .unwrap_or_default()
        )
    }

    /// Sends a request and waits for its reply, reconnecting once if the channel has dropped.
    pub async fn request(&self, request: AgentRequest) -> Result<AgentResponse> {
        self.request_with_timeout(request, Duration::from_secs(180))
            .await
    }

    async fn request_with_timeout(
        &self,
        request: AgentRequest,
        response_timeout: Duration,
    ) -> Result<AgentResponse> {
        match self.try_request(&request, response_timeout).await {
            Ok(response) => Ok(response),
            Err(first) => {
                // A restarted terminal or a guest reboot closes the socket. One transparent
                // retry turns that into a hiccup instead of a visible error.
                tracing::debug!("agent request failed ({first:#}); reconnecting once");
                *self.connection.lock().await = None;
                self.try_request(&request, response_timeout).await
            }
        }
    }

    async fn try_request(
        &self,
        request: &AgentRequest,
        response_timeout: Duration,
    ) -> Result<AgentResponse> {
        let mut guard = self.connection.lock().await;
        if guard.is_none() {
            *guard = Some(self.connect().await?);
        }
        let connection = guard.as_mut().expect("connection was just established");

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let envelope = RequestEnvelope {
            id,
            token: self.token.clone(),
            request: request.clone(),
        };
        let mut line = serde_json::to_string(&envelope)?;
        line.push('\n');
        connection.writer.write_all(line.as_bytes()).await?;
        connection.writer.flush().await?;

        // Skip stale replies from a previous request that timed out, rather than mistaking one
        // for the answer to this request.
        for _ in 0..8 {
            let mut response_line = String::new();
            let read = tokio::time::timeout(
                response_timeout,
                connection.reader.read_line(&mut response_line),
            )
            .await
            .context("the guest did not reply in time")??;
            if read == 0 {
                bail!("the guest control channel closed");
            }
            let response: ResponseEnvelope = serde_json::from_str(response_line.trim())
                .with_context(|| format!("malformed reply from the guest: {response_line}"))?;
            if response.id == id || response.id == 0 {
                return Ok(response.response);
            }
            tracing::warn!("discarding a stale guest reply for request {}", response.id);
        }
        bail!("could not correlate a reply from the guest")
    }

    async fn connect(&self) -> Result<Connection> {
        let stream = tokio::time::timeout(
            Duration::from_secs(3),
            TcpStream::connect(("127.0.0.1", self.port)),
        )
        .await
        .context("timed out connecting to the guest control channel")?
        .context("could not connect to the guest control channel")?;
        stream.set_nodelay(true).ok();
        let (read_half, writer) = stream.into_split();
        Ok(Connection {
            reader: BufReader::new(read_half),
            writer,
        })
    }
}

/// Unwraps a response that must be a specific variant, turning an agent error into an ordinary
/// `Result` so callers do not have to match twice.
pub fn expect_ok(response: AgentResponse) -> Result<AgentResponse> {
    match response {
        AgentResponse::Error { message, .. } => bail!(message),
        other => Ok(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn a_request_to_a_closed_port_fails_rather_than_hanging() {
        // Port 1 is not something we ever bind, so connect fails fast on every platform.
        let client = AgentClient::new(1, "token");
        let result = client.request(AgentRequest::Ping).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn waiting_for_readiness_gives_up_with_a_learner_readable_message() {
        let client = AgentClient::new(1, "token");
        let err = client
            .wait_until_ready(Duration::from_millis(600))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("did not finish starting"), "{err}");
    }

    #[tokio::test]
    async fn readiness_keeps_the_early_qemu_connection_until_the_guest_opens_the_port() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let guest = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (reader, mut writer) = stream.into_split();
            let mut reader = BufReader::new(reader);
            let mut request_line = String::new();
            reader.read_line(&mut request_line).await.unwrap();
            let request: RequestEnvelope = serde_json::from_str(request_line.trim()).unwrap();

            // This is longer than the former two-second readiness probe. A QEMU character
            // socket behaves the same way while Linux is booting: it accepts early, then the
            // guest agent reads and answers the buffered frame once its virtio port is ready.
            tokio::time::sleep(Duration::from_millis(2_200)).await;
            let response = ResponseEnvelope {
                id: request.id,
                response: AgentResponse::Pong {
                    agent_version: "test".into(),
                    image_version: "test-image".into(),
                    kernel: "test-kernel".into(),
                    uptime_seconds: 2,
                },
            };
            let mut line = serde_json::to_string(&response).unwrap();
            line.push('\n');
            writer.write_all(line.as_bytes()).await.unwrap();
            writer.flush().await.unwrap();
        });

        let client = AgentClient::new(port, "token");
        let response = client
            .wait_until_ready(Duration::from_secs(4))
            .await
            .unwrap();
        assert!(matches!(response, AgentResponse::Pong { .. }));
        guest.await.unwrap();
    }

    #[test]
    fn agent_errors_become_result_errors() {
        let response = AgentResponse::Error {
            message: "the lesson setup script failed".into(),
            retriable: true,
        };
        let err = expect_ok(response).unwrap_err().to_string();
        assert_eq!(err, "the lesson setup script failed");
    }

    #[test]
    fn successful_responses_pass_through() {
        let response = AgentResponse::LessonReset {
            lesson_id: "m.01".into(),
        };
        assert!(expect_ok(response).is_ok());
    }

    /// Opt-in Windows release smoke test using the same client and timeout loop as the app.
    #[cfg(windows)]
    #[tokio::test]
    #[ignore = "requires LPL_BOOT_RUNTIME, LPL_BOOT_BASE and LPL_BOOT_DATA"]
    async fn packaged_guest_answers_the_desktop_agent_client() {
        use shared_types::NetworkMode;
        use std::path::PathBuf;
        use vm_manager::{RuntimePaths, SessionKind, VmManager};

        let canonical = |name: &str| -> PathBuf {
            let value = std::env::var_os(name).unwrap_or_else(|| panic!("{name} is not set"));
            std::fs::canonicalize(value).unwrap()
        };
        let runtime = canonical("LPL_BOOT_RUNTIME");
        let base_image = canonical("LPL_BOOT_BASE");
        let data_root = canonical("LPL_BOOT_DATA");
        let data_dir = data_root.join(format!("desktop-run-{}", std::process::id()));
        std::fs::create_dir_all(&data_dir).unwrap();
        let data_dir = std::fs::canonicalize(data_dir).unwrap();
        if let Some(source) = std::env::var_os("LPL_BOOT_OVERLAY") {
            std::fs::copy(source, data_dir.join("free-practice.qcow2")).unwrap();
        }
        let log_dir = data_dir.join("logs");
        std::fs::create_dir_all(&log_dir).unwrap();

        let mut manager = VmManager::new(RuntimePaths {
            qemu_system: runtime.join("qemu-system-x86_64.exe"),
            qemu_img: runtime.join("qemu-img.exe"),
            kernel: runtime.join("vmlinuz"),
            initrd: Some(runtime.join("initrd.img")),
            base_image,
            data_dir: data_dir.clone(),
            log_dir,
        });
        let config = manager
            .prepare(&SessionKind::FreePractice, NetworkMode::Disabled, None)
            .await
            .unwrap();
        let started = std::time::Instant::now();
        manager.start(&config).await.unwrap();
        let mut diagnostics_console = if std::env::var("LPL_BOOT_DIAGNOSTICS").as_deref() == Ok("1")
        {
            Some(connect_to_console(config.console_port).await.unwrap())
        } else {
            None
        };

        let client = AgentClient::new(config.agent_port, config.control_token);
        let ready = client.wait_until_ready(Duration::from_secs(150)).await;
        let elapsed = started.elapsed();
        let diagnostics = match diagnostics_console.as_mut() {
            Some(console) => Some(collect_boot_diagnostics(console).await.unwrap()),
            None => None,
        };
        manager.stop().await.unwrap();
        let response = ready.unwrap();
        if let AgentResponse::Pong {
            uptime_seconds,
            image_version,
            ..
        } = &response
        {
            eprintln!(
                "packaged guest ready in {:.2}s (guest uptime {}s, image {})",
                elapsed.as_secs_f64(),
                uptime_seconds,
                image_version
            );
        }
        if let Some(diagnostics) = diagnostics {
            eprintln!("guest boot diagnostics:\n{diagnostics}");
        }
        assert!(
            matches!(response, AgentResponse::Pong { .. }),
            "{response:?}"
        );
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[cfg(windows)]
    async fn connect_to_console(port: u16) -> Result<TcpStream> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            match TcpStream::connect(("127.0.0.1", port)).await {
                Ok(stream) => return Ok(stream),
                Err(error) if tokio::time::Instant::now() < deadline => {
                    let _ = error;
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    #[cfg(windows)]
    async fn collect_boot_diagnostics(stream: &mut TcpStream) -> Result<String> {
        let mut output = Vec::new();
        read_console_until(
            stream,
            &mut output,
            b"student@linuxlab:",
            Duration::from_secs(15),
        )
        .await?;
        stream
            .write_all(
                b"\rsystemd-analyze time; systemd-analyze critical-chain linuxlab-agent.service; \
systemd-analyze blame | head -30; printf '__LINUXLAB_%s__\\n' DIAGNOSTICS_DONE\r",
            )
            .await?;
        stream.flush().await?;

        read_console_until(
            stream,
            &mut output,
            b"__LINUXLAB_DIAGNOSTICS_DONE__",
            Duration::from_secs(15),
        )
        .await?;
        Ok(String::from_utf8_lossy(&output).to_string())
    }

    #[cfg(windows)]
    async fn read_console_until(
        stream: &mut TcpStream,
        output: &mut Vec<u8>,
        marker: &[u8],
        timeout: Duration,
    ) -> Result<()> {
        let deadline = tokio::time::Instant::now() + timeout;
        while tokio::time::Instant::now() < deadline {
            let mut chunk = [0_u8; 4096];
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let read = match tokio::time::timeout(remaining, stream.read(&mut chunk)).await {
                Ok(result) => result?,
                Err(_) => break,
            };
            if read == 0 {
                break;
            }
            output.extend_from_slice(&chunk[..read]);
            if output.windows(marker.len()).any(|window| window == marker) {
                break;
            }
        }
        Ok(())
    }
}
