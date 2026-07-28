//! Terminal bridge between the guest serial console and xterm.js.
//!
//! Bytes flow guest -> QEMU chardev socket -> here -> Tauri event -> xterm.js, and keystrokes
//! flow back the other way. Nothing in this path interprets the stream: the guest's Bash owns
//! line editing, history and control characters, which is exactly why Ctrl+C, Ctrl+R and
//! tab completion behave like a real terminal rather than an approximation.

use anyhow::{Context, Result};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::{Duration, Instant};

/// Event name the frontend subscribes to for terminal output.
pub const OUTPUT_EVENT: &str = "terminal://output";
/// Event name signalling the console has detached.
pub const CLOSED_EVENT: &str = "terminal://closed";

pub struct ConsoleBridge {
    writer: Arc<Mutex<Option<tokio::net::tcp::OwnedWriteHalf>>>,
    diagnostic_tail: Arc<Mutex<Vec<u8>>>,
}

impl ConsoleBridge {
    pub fn new() -> Self {
        Self {
            writer: Arc::new(Mutex::new(None)),
            diagnostic_tail: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Attaches to the console socket and starts pumping output to the frontend.
    pub async fn attach(&self, app: AppHandle, port: u16) -> Result<()> {
        // Antivirus and cold filesystem caches can delay a newly spawned QEMU process for
        // several seconds on Windows. Keep retrying while Linux boots so a healthy VM never
        // reaches Ready with a blank, permanently detached terminal.
        let stream = connect_when_ready(port, Duration::from_secs(10))
            .await
            .context("could not attach to the Linux console")?;
        stream.set_nodelay(true).ok();
        let (mut read_half, write_half) = stream.into_split();
        *self.writer.lock().await = Some(write_half);
        self.diagnostic_tail.lock().await.clear();
        let diagnostic_tail = self.diagnostic_tail.clone();

        tokio::spawn(async move {
            // 8 KiB matches a typical terminal burst; larger buffers add latency for
            // interactive typing without improving throughput that matters here.
            let mut buffer = vec![0u8; 8192];
            loop {
                match read_half.read(&mut buffer).await {
                    Ok(0) => break,
                    Ok(read) => {
                        let mut tail = diagnostic_tail.lock().await;
                        tail.extend_from_slice(&buffer[..read]);
                        if tail.len() > 32 * 1024 {
                            let excess = tail.len() - 32 * 1024;
                            tail.drain(..excess);
                        }
                        drop(tail);
                        // The guest emits arbitrary bytes, including partial UTF-8 sequences at
                        // a chunk boundary. Sending the raw bytes and letting xterm.js reassemble
                        // them avoids mangling multi-byte characters mid-sequence.
                        if app.emit(OUTPUT_EVENT, buffer[..read].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(err) => {
                        tracing::warn!("console read failed: {err:#}");
                        break;
                    }
                }
            }
            let _ = app.emit(CLOSED_EVENT, ());
        });

        Ok(())
    }

    /// Sends keystrokes to the guest.
    pub async fn write(&self, bytes: &[u8]) -> Result<()> {
        let mut guard = self.writer.lock().await;
        let writer = guard
            .as_mut()
            .context("the terminal is not connected yet")?;
        writer.write_all(bytes).await?;
        writer.flush().await?;
        Ok(())
    }

    pub async fn detach(&self) {
        if let Some(mut writer) = self.writer.lock().await.take() {
            writer.shutdown().await.ok();
        }
    }

    /// Most useful console line for a startup error. Retained after detaching so the backend
    /// can explain a kernel panic or missing root device instead of only saying "timed out".
    pub async fn diagnostic_summary(&self) -> Option<String> {
        summarise_console(&self.diagnostic_tail.lock().await)
    }

    #[cfg(test)]
    pub async fn is_attached(&self) -> bool {
        self.writer.lock().await.is_some()
    }
}

fn summarise_console(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = text
        .lines()
        .map(|line| line.trim_matches(|character: char| character.is_whitespace()))
        .filter(|line| !line.is_empty())
        .collect();
    for marker in [
        "Kernel panic",
        "VFS:",
        "Waiting for root device",
        "[FAILED]",
        " error:",
    ] {
        if let Some(line) = lines.iter().rev().find(|line| line.contains(marker)) {
            return Some(line.chars().take(500).collect());
        }
    }
    lines.last().map(|line| line.chars().take(500).collect())
}

async fn connect_when_ready(port: u16, timeout: Duration) -> std::io::Result<TcpStream> {
    let deadline = Instant::now() + timeout;
    loop {
        match TcpStream::connect(("127.0.0.1", port)).await {
            Ok(stream) => return Ok(stream),
            Err(error) if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(50)).await;
                tracing::trace!("Linux console is not listening yet: {error}");
            }
            Err(error) => return Err(error),
        }
    }
}

impl Default for ConsoleBridge {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn writing_before_attaching_explains_itself() {
        let bridge = ConsoleBridge::new();
        assert!(!bridge.is_attached().await);
        let err = bridge.write(b"ls\n").await.unwrap_err().to_string();
        assert!(err.contains("not connected"), "{err}");
    }

    #[tokio::test]
    async fn detaching_an_unattached_bridge_is_harmless() {
        let bridge = ConsoleBridge::new();
        bridge.detach().await;
        assert!(!bridge.is_attached().await);
    }

    #[test]
    fn diagnostic_summary_prefers_the_kernel_cause_over_a_later_stack_trace() {
        let output = b"VFS: Cannot open root device\nKernel panic - not syncing: VFS: Unable to mount root fs\ndump_stack\n";
        assert_eq!(
            summarise_console(output).as_deref(),
            Some("Kernel panic - not syncing: VFS: Unable to mount root fs")
        );
    }
}
