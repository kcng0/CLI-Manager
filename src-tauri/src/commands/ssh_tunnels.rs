use crate::shell_resolver::silent_command;
use crate::ssh_transport::{SshForwardSpec, SshTransportSpec};
use serde::Serialize;
use std::collections::HashMap;
use std::io::Read;
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use crate::process_job::ChildJob;

const STARTUP_PROBE: Duration = Duration::from_millis(400);
const STDERR_LIMIT: usize = 4 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SshTunnelStatus {
    pub forward_id: String,
    pub state: String,
    pub error: Option<String>,
    pub started_at_ms: Option<u64>,
}

struct TunnelProcess {
    child: Child,
    host_id: String,
    started_at_ms: u64,
    #[cfg(windows)]
    _job: ChildJob,
}

#[derive(Clone)]
pub struct SshTunnelManager {
    inner: Arc<Mutex<HashMap<String, TunnelProcess>>>,
}

impl Default for SshTunnelManager {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl Drop for SshTunnelManager {
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) == 1 {
            self.stop_all();
        }
    }
}

impl SshTunnelManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start(
        &self,
        host_id: String,
        forward_id: String,
        spec: SshTransportSpec,
        forward: SshForwardSpec,
    ) -> Result<SshTunnelStatus, String> {
        validate_forward_id(&host_id)?;
        validate_forward_id(&forward_id)?;
        let launch = spec.build_tunnel_launch(&forward)?;
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "ssh_tunnel_lock_failed".to_string())?;
        if let Some(existing) = inner.get_mut(&forward_id) {
            if existing.child.try_wait().ok().flatten().is_none() {
                return Ok(SshTunnelStatus {
                    forward_id,
                    state: "running".to_string(),
                    error: None,
                    started_at_ms: Some(existing.started_at_ms),
                });
            }
            inner.remove(&forward_id);
        }

        let mut command = silent_command(&launch.executable);
        command
            .args(launch.args)
            .envs(launch.env)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|error| format!("ssh_tunnel_start_failed: {error}"))?;
        #[cfg(windows)]
        let job = match ChildJob::assign(&child, "ssh tunnel") {
            Ok(job) => job,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        std::thread::sleep(STARTUP_PROBE);
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("ssh_tunnel_start_failed: {error}"))?
        {
            let stderr = read_child_stderr(&mut child);
            return Err(if stderr.is_empty() {
                format!("ssh_tunnel_exited: {status}")
            } else {
                stderr
            });
        }
        let started_at_ms = now_ms();
        inner.insert(
            forward_id.clone(),
            TunnelProcess {
                child,
                host_id,
                started_at_ms,
                #[cfg(windows)]
                _job: job,
            },
        );
        Ok(SshTunnelStatus {
            forward_id,
            state: "running".to_string(),
            error: None,
            started_at_ms: Some(started_at_ms),
        })
    }

    pub fn stop(&self, forward_id: &str) -> Result<SshTunnelStatus, String> {
        validate_forward_id(forward_id)?;
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "ssh_tunnel_lock_failed".to_string())?;
        if let Some(mut process) = inner.remove(forward_id) {
            let _ = process.child.kill();
            let _ = process.child.wait();
        }
        Ok(SshTunnelStatus {
            forward_id: forward_id.to_string(),
            state: "stopped".to_string(),
            error: None,
            started_at_ms: None,
        })
    }

    pub fn stop_all(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            for (_, mut process) in inner.drain() {
                let _ = process.child.kill();
                let _ = process.child.wait();
            }
        }
    }

    pub fn stop_for_host(&self, host_id: &str) {
        if host_id.is_empty() {
            return;
        }
        if let Ok(mut inner) = self.inner.lock() {
            let ids: Vec<String> = inner
                .iter()
                .filter(|(_, process)| process.host_id == host_id)
                .map(|(id, _)| id.clone())
                .collect();
            for id in ids {
                if let Some(mut process) = inner.remove(&id) {
                    let _ = process.child.kill();
                    let _ = process.child.wait();
                }
            }
        }
    }

    pub fn status(&self, forward_id: &str) -> Result<SshTunnelStatus, String> {
        validate_forward_id(forward_id)?;
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "ssh_tunnel_lock_failed".to_string())?;
        let Some(process) = inner.get_mut(forward_id) else {
            return Ok(SshTunnelStatus {
                forward_id: forward_id.to_string(),
                state: "stopped".to_string(),
                error: None,
                started_at_ms: None,
            });
        };
        match process.child.try_wait() {
            Ok(None) => Ok(SshTunnelStatus {
                forward_id: forward_id.to_string(),
                state: "running".to_string(),
                error: None,
                started_at_ms: Some(process.started_at_ms),
            }),
            Ok(Some(status)) => {
                let stderr = read_child_stderr(&mut process.child);
                inner.remove(forward_id);
                Ok(SshTunnelStatus {
                    forward_id: forward_id.to_string(),
                    state: "error".to_string(),
                    error: Some(if stderr.is_empty() {
                        format!("ssh_tunnel_exited: {status}")
                    } else {
                        stderr
                    }),
                    started_at_ms: None,
                })
            }
            Err(error) => {
                inner.remove(forward_id);
                Ok(SshTunnelStatus {
                    forward_id: forward_id.to_string(),
                    state: "error".to_string(),
                    error: Some(error.to_string()),
                    started_at_ms: None,
                })
            }
        }
    }

    pub fn list(&self, forward_ids: &[String]) -> Result<Vec<SshTunnelStatus>, String> {
        forward_ids
            .iter()
            .map(|id| self.status(id))
            .collect()
    }
}

fn validate_forward_id(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 128 || value.contains(['\0', '\r', '\n', '/', '\\']) {
        return Err("ssh_forward_id_invalid".to_string());
    }
    Ok(())
}

fn read_child_stderr(child: &mut Child) -> String {
    let Some(mut stderr) = child.stderr.take() else {
        return String::new();
    };
    let mut buffer = Vec::new();
    let _ = stderr.take(STDERR_LIMIT as u64 + 1).read_to_end(&mut buffer);
    String::from_utf8_lossy(&buffer)
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(400)
        .collect()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_millis() as u64
}

#[tauri::command]
pub async fn ssh_tunnel_start(
    manager: tauri::State<'_, SshTunnelManager>,
    host_id: String,
    forward_id: String,
    spec: SshTransportSpec,
    forward: SshForwardSpec,
) -> Result<SshTunnelStatus, String> {
    let manager = manager.inner().clone();
    tokio::task::spawn_blocking(move || manager.start(host_id, forward_id, spec, forward))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn ssh_tunnel_stop(
    manager: tauri::State<'_, SshTunnelManager>,
    forward_id: String,
) -> Result<SshTunnelStatus, String> {
    manager.stop(&forward_id)
}

#[tauri::command]
pub async fn ssh_tunnel_status(
    manager: tauri::State<'_, SshTunnelManager>,
    forward_id: String,
) -> Result<SshTunnelStatus, String> {
    manager.status(&forward_id)
}

#[tauri::command]
pub async fn ssh_tunnel_list(
    manager: tauri::State<'_, SshTunnelManager>,
    forward_ids: Vec<String>,
) -> Result<Vec<SshTunnelStatus>, String> {
    manager.list(&forward_ids)
}

#[cfg(test)]
mod tests {
    use super::validate_forward_id;

    #[test]
    fn forward_ids_reject_path_and_control_characters() {
        assert!(validate_forward_id("fwd-1").is_ok());
        assert!(validate_forward_id("../fwd").is_err());
        assert!(validate_forward_id("fwd\nid").is_err());
        assert!(validate_forward_id("").is_err());
    }
}
