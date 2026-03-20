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
        let log_path = config_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("apex-e2e.log");
        let log_file = File::create(&log_path)
            .with_context(|| format!("failed to create log file: {}", log_path.display()))?;
        let log_file_err = log_file
            .try_clone()
            .with_context(|| format!("failed to clone log file: {}", log_path.display()))?;

        let child = Command::new(env!("CARGO_BIN_EXE_apex"))
            .arg("--config")
            .arg(config_path)
            .arg("gateway")
            .arg("start")
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

            if TcpStream::connect_timeout(
                &addr
                    .parse()
                    .context("failed to parse gateway listen addr")?,
                Duration::from_millis(200),
            )
            .is_ok()
            {
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
