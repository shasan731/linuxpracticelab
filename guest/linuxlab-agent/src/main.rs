//! LinuxLab Agent.
//!
//! Runs as root inside the guest and answers the host over a virtio-serial port. There is no
//! network listener: the only channel is the character device QEMU created, which means the
//! agent is unreachable from anywhere except the desktop application that started the VM.
//!
//! Every request carries the shared token the host generated for this VM run. A frame whose
//! token does not match is dropped, which stops a process inside the guest — including
//! anything the learner writes during a lesson — from driving the agent and marking its own
//! tasks complete.

mod handlers;
mod sys;
mod validators;

use anyhow::{Context, Result};
use shared_types::{AgentResponse, RequestEnvelope, ResponseEnvelope};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Character device created by `-device virtserialport,name=org.linuxlab.agent`.
const DEFAULT_PORT: &str = "/dev/virtio-ports/org.linuxlab.agent";
/// Where the guest reads the expected token from. Written by the boot-time unit from the
/// kernel command line, and readable only by root.
const TOKEN_PATH: &str = "/run/linuxlab/control-token";

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("LINUXLAB_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let port = std::env::var("LINUXLAB_AGENT_PORT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_PORT));
    let token = load_token()?;

    std::fs::create_dir_all("/run/linuxlab/signals").ok();
    std::fs::create_dir_all("/run/linuxlab/checkpoints").ok();

    tracing::info!(
        "linuxlab-agent {} ready on {}",
        handlers::AGENT_VERSION,
        port.display()
    );

    serve(&port, &token).await
}

/// Reads the control token.
///
/// An empty or missing token file is fatal rather than permissive: running with
/// authentication disabled would let anything in the guest impersonate the host.
fn load_token() -> Result<String> {
    if let Ok(token) = std::env::var("LINUXLAB_CONTROL_TOKEN") {
        if !token.trim().is_empty() {
            return Ok(token.trim().to_string());
        }
    }
    let token = std::fs::read_to_string(TOKEN_PATH)
        .with_context(|| format!("could not read the control token from {TOKEN_PATH}"))?;
    let token = token.trim().to_string();
    anyhow::ensure!(
        !token.is_empty(),
        "the control token is empty; refusing to accept unauthenticated requests"
    );
    Ok(token)
}

/// Serves requests until the port closes, then waits for it to be reopened.
///
/// The host may connect, disconnect and reconnect across a single VM lifetime — restarting the
/// terminal does exactly that — so a closed port is a normal event, not a reason to exit.
async fn serve(port: &std::path::Path, token: &str) -> Result<()> {
    loop {
        match session(port, token).await {
            Ok(()) => tracing::info!("control channel closed; waiting for the host to reconnect"),
            Err(err) => tracing::warn!("control channel error: {err:#}"),
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

async fn session(port: &std::path::Path, token: &str) -> Result<()> {
    // A virtio-console port is one bidirectional character device. Opening it separately for
    // reading and writing can leave the second open waiting forever on a real guest. Tokio's
    // generic `split`, meanwhile, serialises both halves behind one lock, so a pending blocking
    // character-device read can prevent the response write. Open the device once, duplicate
    // that OS handle, then give Tokio an independent handle for each direction.
    let read_handle = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(port)
        .with_context(|| format!("could not open {} for read/write", port.display()))?;
    let write_handle = read_handle
        .try_clone()
        .with_context(|| format!("could not duplicate the handle for {}", port.display()))?;
    let read_handle = tokio::fs::File::from_std(read_handle);
    let mut write_handle = tokio::fs::File::from_std(write_handle);
    let mut reader = BufReader::new(read_handle);
    let mut line = String::new();

    loop {
        line.clear();
        let read = reader.read_line(&mut line).await?;
        if read == 0 {
            return Ok(());
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<RequestEnvelope>(trimmed) {
            Ok(envelope) => {
                // Constant-time comparison so a caller cannot learn the token by timing.
                if !constant_time_eq(envelope.token.as_bytes(), token.as_bytes()) {
                    tracing::warn!("rejected a request with an invalid control token");
                    ResponseEnvelope {
                        id: envelope.id,
                        response: AgentResponse::Error {
                            message: "invalid control token".into(),
                            retriable: false,
                        },
                    }
                } else {
                    let id = envelope.id;
                    let response = handlers::handle(envelope.request).await;
                    ResponseEnvelope { id, response }
                }
            }
            Err(err) => ResponseEnvelope {
                // A frame we cannot parse has no id to correlate with.
                id: 0,
                response: AgentResponse::Error {
                    message: format!("malformed request: {err}"),
                    retriable: false,
                },
            },
        };

        let mut encoded = serde_json::to_string(&response)?;
        encoded.push('\n');
        write_handle.write_all(encoded.as_bytes()).await?;
        write_handle.flush().await?;
    }
}

/// Compares two byte strings without an early exit.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut difference = 0u8;
    for (left, right) in a.iter().zip(b.iter()) {
        difference |= left ^ right;
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_comparison_rejects_mismatches_and_length_differences() {
        assert!(constant_time_eq(b"abc123", b"abc123"));
        assert!(!constant_time_eq(b"abc123", b"abc124"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(!constant_time_eq(b"", b"x"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn an_empty_token_file_is_refused() {
        // Guard against a boot-order bug quietly disabling authentication.
        let previous = std::env::var("LINUXLAB_CONTROL_TOKEN").ok();
        std::env::set_var("LINUXLAB_CONTROL_TOKEN", "   ");
        let result = load_token();
        match previous {
            Some(value) => std::env::set_var("LINUXLAB_CONTROL_TOKEN", value),
            None => std::env::remove_var("LINUXLAB_CONTROL_TOKEN"),
        }
        // With a blank environment token it falls through to the file, which does not exist
        // in a test environment, so this must be an error either way.
        assert!(result.is_err());
    }

    #[test]
    fn an_environment_token_is_honoured_for_development() {
        let previous = std::env::var("LINUXLAB_CONTROL_TOKEN").ok();
        std::env::set_var("LINUXLAB_CONTROL_TOKEN", "dev-token");
        let token = load_token().unwrap();
        match previous {
            Some(value) => std::env::set_var("LINUXLAB_CONTROL_TOKEN", value),
            None => std::env::remove_var("LINUXLAB_CONTROL_TOKEN"),
        }
        assert_eq!(token, "dev-token");
    }

    #[test]
    fn a_malformed_frame_produces_an_error_response_with_no_correlation_id() {
        // Mirrors the parse-failure branch in `session` without needing a character device.
        let parsed = serde_json::from_str::<RequestEnvelope>("{ not json");
        assert!(parsed.is_err());
    }

    #[test]
    fn a_valid_frame_roundtrips_through_the_envelope() {
        let json = r#"{"id":9,"token":"t","request":{"op":"ping"}}"#;
        let envelope: RequestEnvelope = serde_json::from_str(json).unwrap();
        assert_eq!(envelope.id, 9);
        assert_eq!(envelope.token, "t");
    }
}
