use anyhow::{Context, bail};
use std::fs::File;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub struct GatewayProcess {
    child: Child,
    base_url: String,
    log_path: PathBuf,
}

impl GatewayProcess {
    pub fn spawn(config_path: &Path, listen: &str) -> anyhow::Result<Self> {
        Self::spawn_with_command(config_path, listen, false)
    }

    pub fn spawn_run_with_env_config(config_path: &Path, listen: &str) -> anyhow::Result<Self> {
        Self::spawn_with_command(config_path, listen, true)
    }

    fn spawn_with_command(
        config_path: &Path,
        listen: &str,
        use_env_config_and_run: bool,
    ) -> anyhow::Result<Self> {
        let log_path = config_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("apex-e2e.log");
        let log_file = File::create(&log_path)
            .with_context(|| format!("failed to create log file: {}", log_path.display()))?;
        let log_file_err = log_file
            .try_clone()
            .with_context(|| format!("failed to clone log file: {}", log_path.display()))?;

        let mut command = Command::new(env!("CARGO_BIN_EXE_apex"));
        command.arg("gateway");
        if use_env_config_and_run {
            command.arg("run").env("APEX_CONFIG", config_path);
        } else {
            command.arg("start").arg("--config").arg(config_path);
        }

        let child = command
            .stdout(Stdio::from(log_file))
            .stderr(Stdio::from(log_file_err))
            .spawn()
            .context("failed to spawn apex gateway process")?;

        Ok(Self {
            child,
            base_url: format!("http://{listen}"),
            log_path,
        })
    }

    pub fn wait_until_ready(&mut self, timeout: Duration) -> anyhow::Result<()> {
        let endpoint = self.base_url.clone();
        let addr = endpoint
            .strip_prefix("http://")
            .unwrap_or(&endpoint)
            .to_string();
        let deadline = Instant::now() + timeout;

        while Instant::now() < deadline {
            if let Some(status) = self.child.try_wait()? {
                let logs = self.read_logs();
                bail!("apex exited before becoming ready (status: {status}). logs:\n{logs}");
            }

            if serves_http(
                &addr
                    .parse()
                    .context("failed to parse gateway listen addr")?,
            ) {
                return Ok(());
            }

            std::thread::sleep(Duration::from_millis(100));
        }

        let logs = self.read_logs();
        bail!("timed out waiting for apex at {endpoint}. logs:\n{logs}");
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn read_logs(&self) -> String {
        std::fs::read_to_string(&self.log_path).unwrap_or_else(|_| "<no logs>".to_string())
    }
}

impl Drop for GatewayProcess {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

pub fn pick_listen_addr() -> anyhow::Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0").context("failed to bind ephemeral port")?;
    let addr: SocketAddr = listener.local_addr().context("failed to read local addr")?;
    Ok(addr.to_string())
}

/// Readiness = the server answers HTTP, not merely that the port is bound.
///
/// A bare `TcpStream::connect` can succeed against the kernel's listen backlog
/// before axum is accepting, and the probe then drops that connection — so the
/// first real request could land on a socket the server had not finished
/// wiring up and come back as `ConnectionReset`. Sending an actual request and
/// waiting for a status line closes that window.
///
/// Any HTTP response counts, including 401: `/api/cp/info` enforces the global
/// auth key itself, and a rejection still proves the router is live.
pub fn serves_http(addr: &SocketAddr) -> bool {
    use std::io::{Read, Write};

    let Ok(mut stream) = TcpStream::connect_timeout(addr, Duration::from_millis(200)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));

    let request = format!("GET /api/cp/info HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }

    let mut buf = [0u8; 16];
    let mut seen = 0;
    while seen < buf.len() {
        match stream.read(&mut buf[seen..]) {
            Ok(0) => break,
            Ok(n) => seen += n,
            Err(_) => return false,
        }
    }
    buf[..seen].starts_with(b"HTTP/")
}
