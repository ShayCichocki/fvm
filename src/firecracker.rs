use crate::artifact::{CgroupSetting, PortMapping, ReadinessConfig};
use crate::command_util::{run_capture, spawn_with_log_streaming};
use anyhow::{Context, Result, bail};
use serde_json::json;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DEFAULT_GUEST_MAC: &str = "AA:FC:00:00:00:01";

#[derive(Clone, Debug)]
pub struct FirecrackerConfigSpec {
    pub kernel_path: PathBuf,
    pub rootfs_path: PathBuf,
    pub boot_args: String,
    pub memory_mib: u32,
    pub vcpus: u8,
    pub rootfs_read_only: bool,
    pub tap_name: Option<String>,
    pub guest_mac: Option<String>,
    pub log_path: Option<PathBuf>,
    pub metrics_path: Option<PathBuf>,
    pub track_dirty_pages: bool,
}

#[derive(Clone, Debug)]
pub struct LaunchSpec {
    pub firecracker: String,
    pub kernel_path: PathBuf,
    pub rootfs_path: PathBuf,
    pub boot_args: String,
    pub memory_mib: u32,
    pub vcpus: u8,
    pub rootfs_read_only: bool,
    pub ports: Vec<PortMapping>,
    pub dry_run: bool,
    pub track_dirty_pages: bool,
    pub snapshot_load: Option<SnapshotLoadSpec>,
    pub cgroups: Vec<CgroupSetting>,
}

#[derive(Clone, Debug)]
pub struct SnapshotLoadSpec {
    pub mem_path: PathBuf,
    pub vmstate_path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct RunMetrics {
    pub boot_to_listen_ms: Option<u128>,
    pub host_rss_kib: Option<u64>,
    pub guest_rss_kib: Option<u64>,
}

pub struct RunningVm {
    child: Option<Child>,
    _run_dir: tempfile::TempDir,
    network: Option<NetworkHandle>,
    forwarders: Vec<PortForwarder>,
    pub api_socket: PathBuf,
    pub log_path: PathBuf,
    pub started_at: Instant,
    pub dry_run: bool,
}

pub struct NetworkHandle {
    pub tap_name: String,
    pub guest_ip: String,
}

struct NetworkCleanup {
    tap_name: Option<String>,
}

impl Drop for NetworkCleanup {
    fn drop(&mut self) {
        if let Some(tap_name) = &self.tap_name {
            cleanup_tap(tap_name);
        }
    }
}

pub struct PortForwarder {
    host_port: u16,
    guest_ip: String,
    guest_port: u16,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Drop for RunningVm {
    fn drop(&mut self) {
        if self.dry_run {
            return;
        }
        if let Some(child) = &mut self.child {
            match child.try_wait() {
                Ok(Some(_)) => {}
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                Err(_) => {}
            }
        }
        if let Some(network) = &self.network {
            cleanup_tap(&network.tap_name);
        }
    }
}

impl Drop for PortForwarder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl RunningVm {
    pub fn wait_for_readiness(&self, readiness: &Option<ReadinessConfig>) -> Result<Option<u128>> {
        let Some(readiness) = readiness else {
            return Ok(None);
        };
        if self.dry_run {
            return Ok(Some(0));
        }
        let (host, port) = self
            .readiness_endpoint()
            .context("readiness requires at least one forwarded port")?;
        wait_for_http_readiness(&host, port, readiness, self.started_at)
    }

    pub fn metrics_after_readiness(&self, boot_to_listen_ms: Option<u128>) -> RunMetrics {
        let host_rss_kib = self
            .child
            .as_ref()
            .and_then(|child| host_rss_kib(child.id()));
        let guest_rss_kib = latest_guest_rss_from_log(&self.log_path);
        RunMetrics {
            boot_to_listen_ms,
            host_rss_kib,
            guest_rss_kib,
        }
    }

    pub fn wait(mut self) -> Result<()> {
        if let Some(child) = &mut self.child {
            let status = child.wait().context("failed to wait for Firecracker")?;
            if !status.success() {
                bail!("Firecracker exited with status {status}");
            }
        }
        Ok(())
    }

    pub fn shutdown(&mut self) -> Result<()> {
        if self.dry_run {
            return Ok(());
        }

        let _ = firecracker_api(&self.api_socket, "PATCH", "/vm", r#"{"state":"Paused"}"#);
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(network) = &self.network {
            cleanup_tap(&network.tap_name);
        }
        Ok(())
    }

    pub fn create_snapshot(&self, mem_path: &Path, vmstate_path: &Path) -> Result<()> {
        if self.dry_run {
            std::fs::write(mem_path, b"dry-run snapshot memory\n")?;
            std::fs::write(vmstate_path, b"dry-run snapshot vmstate\n")?;
            return Ok(());
        }

        firecracker_api(&self.api_socket, "PATCH", "/vm", r#"{"state":"Paused"}"#)?;
        let body = json!({
            "snapshot_type": "Full",
            "snapshot_path": vmstate_path,
            "mem_file_path": mem_path
        })
        .to_string();
        firecracker_api(&self.api_socket, "PUT", "/snapshot/create", &body)?;
        Ok(())
    }

    fn readiness_endpoint(&self) -> Option<(String, u16)> {
        let forwarder = self.forwarders.first()?;
        if self.network.is_some() {
            Some((forwarder.guest_ip.clone(), forwarder.guest_port))
        } else {
            forwarder
                .bound_port()
                .map(|port| ("127.0.0.1".to_string(), port))
        }
    }
}

impl PortForwarder {
    fn bound_port(&self) -> Option<u16> {
        Some(self.host_port)
    }
}

pub fn write_firecracker_config(path: &Path, spec: &FirecrackerConfigSpec) -> Result<()> {
    let mut config = json!({
        "boot-source": {
            "kernel_image_path": spec.kernel_path,
            "boot_args": spec.boot_args,
        },
        "drives": [{
            "drive_id": "rootfs",
            "path_on_host": spec.rootfs_path,
            "is_root_device": true,
            "is_read_only": spec.rootfs_read_only,
        }],
        "machine-config": {
            "vcpu_count": spec.vcpus,
            "mem_size_mib": spec.memory_mib,
            "smt": false,
            "track_dirty_pages": spec.track_dirty_pages,
        }
    });

    if let Some(tap_name) = &spec.tap_name {
        config["network-interfaces"] = json!([{
            "iface_id": "eth0",
            "guest_mac": spec.guest_mac.as_deref().unwrap_or(DEFAULT_GUEST_MAC),
            "host_dev_name": tap_name,
        }]);
    }

    if let Some(log_path) = &spec.log_path {
        config["logger"] = json!({
            "log_path": log_path,
            "level": "Info",
            "show_level": true,
            "show_log_origin": false,
        });
    }

    if let Some(metrics_path) = &spec.metrics_path {
        config["metrics"] = json!({
            "metrics_path": metrics_path,
        });
    }

    let json = serde_json::to_string_pretty(&config)?;
    std::fs::write(path, format!("{json}\n"))
        .with_context(|| format!("failed to write Firecracker config {}", path.display()))?;
    Ok(())
}

pub fn launch_vm(spec: &LaunchSpec) -> Result<RunningVm> {
    let run_dir = tempfile::Builder::new()
        .prefix("fvm-run-")
        .tempdir()
        .context("failed to create Firecracker run directory")?;
    let api_socket = run_dir.path().join("firecracker.sock");
    let log_path = run_dir.path().join("serial.log");
    let metrics_path = run_dir.path().join("metrics.json");
    let fc_config_path = run_dir.path().join("firecracker.json");

    let network = if spec.ports.is_empty() {
        None
    } else {
        Some(setup_network(spec.dry_run, spec.snapshot_load.is_none())?)
    };
    let mut network_cleanup = NetworkCleanup {
        tap_name: network.as_ref().map(|network| network.tap_name.clone()),
    };

    let boot_args = if let Some(network) = &network {
        format!(
            "{} ip={}::172.16.0.1:255.255.255.252::eth0:off",
            spec.boot_args, network.guest_ip
        )
    } else {
        spec.boot_args.clone()
    };

    let mut forwarders = Vec::new();
    if let Some(network) = &network {
        for mapping in &spec.ports {
            forwarders.push(start_port_forwarder(
                mapping.host,
                network.guest_ip.clone(),
                mapping.guest,
                spec.dry_run,
            )?);
        }
    }

    write_firecracker_config(
        &fc_config_path,
        &FirecrackerConfigSpec {
            kernel_path: spec.kernel_path.clone(),
            rootfs_path: spec.rootfs_path.clone(),
            boot_args,
            memory_mib: spec.memory_mib,
            vcpus: spec.vcpus,
            rootfs_read_only: spec.rootfs_read_only,
            tap_name: network.as_ref().map(|network| network.tap_name.clone()),
            guest_mac: Some(DEFAULT_GUEST_MAC.to_string()),
            log_path: Some(log_path.clone()),
            metrics_path: Some(metrics_path.clone()),
            track_dirty_pages: spec.track_dirty_pages,
        },
    )?;

    if spec.dry_run {
        if let Some(snapshot) = &spec.snapshot_load {
            let _ = (&snapshot.mem_path, &snapshot.vmstate_path);
        }
        network_cleanup.tap_name = None;
        return Ok(RunningVm {
            child: None,
            _run_dir: run_dir,
            network,
            forwarders,
            api_socket,
            log_path,
            started_at: Instant::now(),
            dry_run: true,
        });
    }

    let child = if let Some(snapshot) = &spec.snapshot_load {
        launch_restored_vm(
            &spec.firecracker,
            &api_socket,
            snapshot,
            network.as_ref().map(|network| network.tap_name.as_str()),
            &log_path,
        )?
    } else {
        let mut command = Command::new(&spec.firecracker);
        command
            .arg("--api-sock")
            .arg(&api_socket)
            .arg("--config-file")
            .arg(&fc_config_path);
        spawn_with_log_streaming(command, &log_path)?
    };

    apply_cgroups(child.id(), &spec.cgroups)?;

    network_cleanup.tap_name = None;
    Ok(RunningVm {
        child: Some(child),
        _run_dir: run_dir,
        network,
        forwarders,
        api_socket,
        log_path,
        started_at: Instant::now(),
        dry_run: false,
    })
}

fn launch_restored_vm(
    firecracker: &str,
    api_socket: &Path,
    snapshot: &SnapshotLoadSpec,
    tap_name: Option<&str>,
    log_path: &Path,
) -> Result<Child> {
    let mut command = Command::new(firecracker);
    command.arg("--api-sock").arg(api_socket);
    let child = spawn_with_log_streaming(command, log_path)
        .context("failed to launch Firecracker for snapshot restore")?;

    wait_for_api_socket(api_socket, Duration::from_secs(5))?;
    let mut body = json!({
        "snapshot_path": snapshot.vmstate_path,
        "mem_backend": {
            "backend_type": "File",
            "backend_path": snapshot.mem_path,
        },
        "enable_diff_snapshots": false,
        "resume_vm": true,
    });
    if let Some(tap_name) = tap_name {
        body["network_overrides"] = json!([{
            "iface_id": "eth0",
            "host_dev_name": tap_name,
        }]);
    }
    let body = body.to_string();
    firecracker_api(api_socket, "PUT", "/snapshot/load", &body)?;
    Ok(child)
}

fn setup_network(dry_run: bool, static_guest_neighbor: bool) -> Result<NetworkHandle> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        % 100_000;
    let suffix = format!("{}{}", std::process::id() % 1000, millis);
    let tap_name = format!("fvm{suffix}");
    let guest_ip = "172.16.0.2".to_string();

    if dry_run {
        return Ok(NetworkHandle { tap_name, guest_ip });
    }

    run_capture("ip", ["tuntap", "add", "dev", &tap_name, "mode", "tap"])?;
    run_capture("ip", ["addr", "add", "172.16.0.1/30", "dev", &tap_name])?;
    run_capture("ip", ["link", "set", "dev", &tap_name, "up"])?;
    if static_guest_neighbor {
        run_capture(
            "ip",
            [
                "neigh",
                "replace",
                &guest_ip,
                "lladdr",
                DEFAULT_GUEST_MAC,
                "dev",
                &tap_name,
                "nud",
                "permanent",
            ],
        )?;
    }
    Ok(NetworkHandle { tap_name, guest_ip })
}

fn cleanup_tap(tap_name: &str) {
    let _ = run_capture("ip", ["link", "set", "dev", tap_name, "down"]);
    let _ = run_capture("ip", ["tuntap", "del", "dev", tap_name, "mode", "tap"]);
}

fn apply_cgroups(pid: u32, settings: &[CgroupSetting]) -> Result<()> {
    if settings.is_empty() {
        return Ok(());
    }
    if std::env::consts::OS != "linux" {
        bail!("cgroup settings require Linux cgroup v2");
    }

    let base = std::env::var_os("FVM_CGROUP_BASE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/sys/fs/cgroup/fvm"));
    let group = base.join(format!("vm-{pid}"));
    std::fs::create_dir_all(&group)
        .with_context(|| format!("failed to create cgroup {}", group.display()))?;

    for setting in settings {
        if setting.key.contains('/') || setting.key.contains("..") {
            bail!("invalid cgroup key `{}`", setting.key);
        }
        std::fs::write(group.join(&setting.key), &setting.value).with_context(|| {
            format!(
                "failed to write cgroup setting {}={} in {}",
                setting.key,
                setting.value,
                group.display()
            )
        })?;
    }
    std::fs::write(group.join("cgroup.procs"), pid.to_string()).with_context(|| {
        format!(
            "failed to attach Firecracker pid {pid} to cgroup {}",
            group.display()
        )
    })?;
    Ok(())
}

fn start_port_forwarder(
    host_port: u16,
    guest_ip: String,
    guest_port: u16,
    dry_run: bool,
) -> Result<PortForwarder> {
    let stop = Arc::new(AtomicBool::new(false));
    if dry_run {
        return Ok(PortForwarder {
            host_port,
            guest_ip,
            guest_port,
            stop,
            handle: None,
        });
    }

    let listener = TcpListener::bind(("127.0.0.1", host_port))
        .with_context(|| format!("failed to bind host port {host_port}"))?;
    listener.set_nonblocking(true)?;
    let guest_addr = resolve_socket_addr(&guest_ip, guest_port)?;
    let stop_for_thread = Arc::clone(&stop);
    let handle = thread::spawn(move || {
        while !stop_for_thread.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((client, _)) => {
                    thread::spawn(move || {
                        if let Ok(upstream) =
                            TcpStream::connect_timeout(&guest_addr, Duration::from_millis(50))
                        {
                            let _ = proxy_tcp(client, upstream);
                        }
                    });
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(_) => break,
            }
        }
    });

    Ok(PortForwarder {
        host_port,
        guest_ip,
        guest_port,
        stop,
        handle: Some(handle),
    })
}

fn proxy_tcp(mut left: TcpStream, mut right: TcpStream) -> Result<()> {
    let mut left_clone = left.try_clone()?;
    let mut right_clone = right.try_clone()?;
    let a = thread::spawn(move || std::io::copy(&mut left_clone, &mut right));
    let b = thread::spawn(move || std::io::copy(&mut right_clone, &mut left));
    let _ = a.join();
    let _ = b.join();
    Ok(())
}

fn wait_for_http_readiness(
    host: &str,
    port: u16,
    readiness: &ReadinessConfig,
    started_at: Instant,
) -> Result<Option<u128>> {
    let deadline = Instant::now() + Duration::from_secs(readiness.timeout_seconds);
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n",
        readiness.http_path
    );
    let trace = std::env::var_os("FVM_TRACE_READINESS").is_some();
    let mut attempts = 0_u32;

    let mut last_error = None;
    while Instant::now() < deadline {
        attempts += 1;
        let address = resolve_socket_addr(host, port)?;
        match TcpStream::connect_timeout(&address, Duration::from_millis(50)) {
            Ok(mut stream) => {
                let _ = stream.set_read_timeout(Some(Duration::from_millis(250)));
                stream.write_all(request.as_bytes())?;
                let response = read_http_status_response(&mut stream)?;
                if trace {
                    let status = response.lines().next().unwrap_or("empty");
                    eprintln!(
                        "FVM_READINESS attempt={attempts} elapsed_ms={} endpoint={host}:{port} status={status:?}",
                        started_at.elapsed().as_millis()
                    );
                }
                if response.starts_with("HTTP/1.1 2") || response.starts_with("HTTP/1.0 2") {
                    return Ok(Some(started_at.elapsed().as_millis()));
                }
                last_error = Some(format!("readiness returned non-2xx response: {response}"));
            }
            Err(err) => {
                if trace {
                    eprintln!(
                        "FVM_READINESS attempt={attempts} elapsed_ms={} endpoint={host}:{port} error={err}",
                        started_at.elapsed().as_millis()
                    );
                }
                last_error = Some(err.to_string());
            }
        }
        thread::sleep(Duration::from_millis(10));
    }

    bail!(
        "readiness timed out after {}s for http://{host}:{port}{}; last error: {}",
        readiness.timeout_seconds,
        readiness.http_path,
        last_error.unwrap_or_else(|| "none".to_string())
    );
}

fn read_http_status_response<R: Read>(stream: &mut R) -> Result<String> {
    let mut response = Vec::new();
    let mut buf = [0_u8; 256];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                response.extend_from_slice(&buf[..n]);
                if response.windows(2).any(|window| window == b"\r\n") {
                    break;
                }
            }
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.kind() == std::io::ErrorKind::TimedOut =>
            {
                break;
            }
            Err(err) => return Err(err.into()),
        }
    }
    Ok(String::from_utf8_lossy(&response).to_string())
}

fn read_http_response<R: Read>(stream: &mut R) -> Result<String> {
    let mut response = Vec::new();
    let mut buf = [0_u8; 1024];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => response.extend_from_slice(&buf[..n]),
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.kind() == std::io::ErrorKind::TimedOut =>
            {
                break;
            }
            Err(err) => return Err(err.into()),
        }
    }
    Ok(String::from_utf8_lossy(&response).to_string())
}

fn resolve_socket_addr(host: &str, port: u16) -> Result<SocketAddr> {
    (host, port)
        .to_socket_addrs()?
        .next()
        .with_context(|| format!("failed to resolve {host}:{port}"))
}

fn host_rss_kib(_pid: u32) -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let pid = _pid;
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
        for line in status.lines() {
            if let Some(value) = line.strip_prefix("VmRSS:") {
                return value.split_whitespace().next()?.parse().ok();
            }
        }
    }
    None
}

pub fn latest_guest_rss_from_log(path: &Path) -> Option<u64> {
    let content = std::fs::read_to_string(path).ok()?;
    content.lines().rev().find_map(|line| {
        line.strip_prefix("FVM_GUEST_RSS_KIB=")
            .and_then(|value| value.trim().parse().ok())
    })
}

fn wait_for_api_socket(path: &Path, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(20));
    }
    bail!(
        "timed out waiting for Firecracker API socket {}",
        path.display()
    );
}

pub fn firecracker_api(socket: &Path, method: &str, path: &str, body: &str) -> Result<String> {
    #[cfg(unix)]
    {
        use std::os::unix::net::UnixStream;

        let mut stream = UnixStream::connect(socket).with_context(|| {
            format!(
                "failed to connect to Firecracker API socket {}",
                socket.display()
            )
        })?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes())?;
        let response = read_http_response(&mut stream)?;
        if !(response.starts_with("HTTP/1.1 2") || response.starts_with("HTTP/1.0 2")) {
            bail!("Firecracker API {method} {path} failed: {response}");
        }
        Ok(response)
    }

    #[cfg(not(unix))]
    {
        let _ = (socket, method, path, body);
        bail!("Firecracker API requires Unix sockets")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_latest_guest_rss() {
        let temp = tempfile::tempdir().unwrap();
        let log = temp.path().join("serial.log");
        std::fs::write(
            &log,
            "hello\nFVM_GUEST_RSS_KIB=12\nother\nFVM_GUEST_RSS_KIB=42\n",
        )
        .unwrap();
        assert_eq!(latest_guest_rss_from_log(&log), Some(42));
    }

    #[test]
    fn writes_firecracker_config() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("firecracker.json");
        write_firecracker_config(
            &config,
            &FirecrackerConfigSpec {
                kernel_path: PathBuf::from("kernel"),
                rootfs_path: PathBuf::from("rootfs.ext4"),
                boot_args: "init=/app".to_string(),
                memory_mib: 64,
                vcpus: 1,
                rootfs_read_only: true,
                tap_name: Some("fvm0".to_string()),
                guest_mac: Some("AA:FC:00:00:00:01".to_string()),
                log_path: None,
                metrics_path: None,
                track_dirty_pages: true,
            },
        )
        .unwrap();
        let content = std::fs::read_to_string(config).unwrap();
        assert!(content.contains("boot-source"));
        assert!(content.contains("network-interfaces"));
        assert!(content.contains("track_dirty_pages"));
    }
}
