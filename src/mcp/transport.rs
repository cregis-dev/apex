use crate::mcp::protocol::{ErrorResponse, JsonRpcMessage, Notification, Request};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Stdin, Stdout};
use tokio::sync::mpsc;
use tracing::{debug, error};

pub struct StdinTransport {
    stdin: BufReader<Stdin>,
    stdout: Stdout,
}

impl StdinTransport {
    pub fn new() -> Self {
        Self {
            stdin: BufReader::new(tokio::io::stdin()),
            stdout: tokio::io::stdout(),
        }
    }

    pub async fn run(
        self,
        tx: mpsc::Sender<JsonRpcMessage>,
        mut rx: mpsc::Receiver<JsonRpcMessage>,
    ) -> anyhow::Result<()> {
        let StdinTransport { stdin, mut stdout } = self;
        let mut lines = stdin.lines();

        loop {
            tokio::select! {
                // Read from stdin
                result = lines.next_line() => {
                    match result {
                        Ok(Some(line)) => {
                            debug!("Received line: {}", line);
                            match serde_json::from_str::<Value>(&line) {
                                Ok(val) => {
                                    // Basic JSON-RPC validation logic
                                    // TODO: Improve deserialization to handle batch or specific types
                                    // For now, try to parse as Request or Notification
                                    if let Ok(req) = serde_json::from_value::<Request>(val.clone()) {
                                        tx.send(JsonRpcMessage::Request(req)).await?;
                                    } else if let Ok(notif) = serde_json::from_value::<Notification>(val.clone()) {
                                        tx.send(JsonRpcMessage::Notification(notif)).await?;
                                    } else {
                                        // Invalid JSON-RPC
                                        let err = ErrorResponse::invalid_request(Some(serde_json::json!({"original": line})));
                                        let resp = serde_json::to_string(&err)?;
                                        stdout.write_all(resp.as_bytes()).await?;
                                        stdout.write_all(b"\n").await?;
                                        stdout.flush().await?;
                                    }
                                }
                                Err(e) => {
                                    error!("Parse error: {}", e);
                                    let err = ErrorResponse::parse_error(Some(serde_json::json!(e.to_string())));
                                    let resp = serde_json::to_string(&err)?;
                                    stdout.write_all(resp.as_bytes()).await?;
                                    stdout.write_all(b"\n").await?;
                                    stdout.flush().await?;
                                }
                            }
                        }
                        Ok(None) => break, // EOF
                        Err(e) => return Err(e.into()),
                    }
                }
                // Write to stdout
                msg = rx.recv() => {
                    match msg {
                        Some(msg) => {
                            let json = match msg {
                                JsonRpcMessage::Response(r) => serde_json::to_string(&r)?,
                                JsonRpcMessage::Notification(n) => serde_json::to_string(&n)?,
                                JsonRpcMessage::Error(e) => serde_json::to_string(&e)?,
                                _ => continue, // Should not send requests back to client in this loop? Or maybe server notifications
                            };
                            debug!("Sending line: {}", json);
                            stdout.write_all(json.as_bytes()).await?;
                            stdout.write_all(b"\n").await?;
                            stdout.flush().await?;
                        }
                        None => break, // Channel closed
                    }
                }
            }
        }
        Ok(())
    }
}

impl Default for StdinTransport {
    fn default() -> Self {
        Self::new()
    }
}
