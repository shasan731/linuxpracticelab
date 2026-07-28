//! Minimal QMP client.
//!
//! Only the handful of commands the product needs: capability negotiation, pause/resume,
//! clean powerdown, and a status query. Anything beyond that is deliberately absent, since
//! the QMP socket is the most powerful interface QEMU exposes.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

pub struct QmpClient {
    reader: BufReader<tokio::net::tcp::OwnedReadHalf>,
    writer: tokio::net::tcp::OwnedWriteHalf,
}

#[derive(Debug, Deserialize)]
struct StatusReturn {
    status: String,
}

impl QmpClient {
    /// Connects and completes the capability handshake. Loopback only, by construction.
    pub async fn connect(port: u16, timeout: Duration) -> Result<Self> {
        let stream = tokio::time::timeout(timeout, TcpStream::connect(("127.0.0.1", port)))
            .await
            .context("timed out connecting to the QEMU control socket")?
            .context("could not connect to the QEMU control socket")?;
        stream.set_nodelay(true).ok();

        let (read_half, writer) = stream.into_split();
        let mut client = Self {
            reader: BufReader::new(read_half),
            writer,
        };

        // QEMU greets with {"QMP": {...}} and rejects everything but qmp_capabilities until
        // we answer.
        let greeting = client.read_message().await?;
        if greeting.get("QMP").is_none() {
            bail!("unexpected QMP greeting: {greeting}");
        }
        client.execute("qmp_capabilities", None).await?;
        Ok(client)
    }

    async fn read_message(&mut self) -> Result<Value> {
        let mut line = String::new();
        let read = self
            .reader
            .read_line(&mut line)
            .await
            .context("failed reading from the QEMU control socket")?;
        if read == 0 {
            bail!("the QEMU control socket closed");
        }
        serde_json::from_str(&line).with_context(|| format!("malformed QMP message: {line}"))
    }

    /// Sends a command and returns its `return` payload, skipping asynchronous events.
    pub async fn execute(&mut self, command: &str, arguments: Option<Value>) -> Result<Value> {
        let mut request = json!({ "execute": command });
        if let Some(args) = arguments {
            request["arguments"] = args;
        }
        let mut line = serde_json::to_string(&request)?;
        line.push('\n');
        self.writer
            .write_all(line.as_bytes())
            .await
            .context("failed writing to the QEMU control socket")?;
        self.writer.flush().await.ok();

        // QEMU interleaves events with replies; keep reading until the reply arrives.
        for _ in 0..64 {
            let message = self.read_message().await?;
            if let Some(error) = message.get("error") {
                bail!("QMP command {command} failed: {error}");
            }
            if let Some(value) = message.get("return") {
                return Ok(value.clone());
            }
            if message.get("event").is_some() {
                tracing::debug!(?message, "QMP event");
                continue;
            }
        }
        bail!("no reply to QMP command {command} after 64 messages")
    }

    /// Current run state, e.g. `running`, `paused`, `shutdown`.
    pub async fn query_status(&mut self) -> Result<String> {
        let value = self.execute("query-status", None).await?;
        let status: StatusReturn =
            serde_json::from_value(value).context("unexpected query-status payload")?;
        Ok(status.status)
    }

    pub async fn pause(&mut self) -> Result<()> {
        self.execute("stop", None).await.map(|_| ())
    }

    pub async fn resume(&mut self) -> Result<()> {
        self.execute("cont", None).await.map(|_| ())
    }

    /// Asks the guest to shut down via ACPI. This is the polite path; the caller escalates
    /// to terminating the process group if the guest does not exit in time.
    pub async fn system_powerdown(&mut self) -> Result<()> {
        self.execute("system_powerdown", None).await.map(|_| ())
    }

    /// Immediate stop. Safe because every writable layer is a disposable overlay.
    pub async fn quit(&mut self) -> Result<()> {
        // `quit` frequently closes the socket before the reply is flushed, so a transport
        // error here is a success, not a failure.
        match self.execute("quit", None).await {
            Ok(_) => Ok(()),
            Err(err) => {
                tracing::debug!("quit returned {err:#}; treating as success");
                Ok(())
            }
        }
    }
}
