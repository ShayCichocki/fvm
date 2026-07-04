use crate::artifact::{HostBenchmarkInfo, ToolVersion};
use crate::command_util::{command_exists, platform_is_linux};
use anyhow::{Result, bail};
use std::path::Path;
use std::process::Command;

#[derive(Clone, Debug)]
pub struct ToolchainConfig {
    pub java: String,
    pub native_image: String,
    pub firecracker: String,
}

impl Default for ToolchainConfig {
    fn default() -> Self {
        Self {
            java: "java".to_string(),
            native_image: "native-image".to_string(),
            firecracker: "firecracker".to_string(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ToolchainVersions {
    pub java: Option<ToolVersion>,
    pub native_image: Option<ToolVersion>,
    pub firecracker: Option<ToolVersion>,
}

pub fn probe_versions(config: &ToolchainConfig) -> ToolchainVersions {
    ToolchainVersions {
        java: capture_version(&config.java, &["-version"]),
        native_image: capture_version(&config.native_image, &["--version"]),
        firecracker: capture_version(&config.firecracker, &["--version"]),
    }
}

pub fn validate_build_toolchain(
    config: &ToolchainConfig,
    target_java: u16,
    dry_run: bool,
    allow_java_mismatch: bool,
    java_required: bool,
    native_image_required: bool,
) -> Result<ToolchainVersions> {
    let versions = probe_versions(config);

    if dry_run {
        return Ok(versions);
    }

    if java_required {
        require_executable(&config.java)?;
    }
    if native_image_required {
        require_executable(&config.native_image)?;
    }

    if !allow_java_mismatch {
        if java_required && let Some(java) = &versions.java {
            ensure_version_mentions_target("java", java, target_java)?;
        }
        if native_image_required && let Some(native_image) = &versions.native_image {
            ensure_version_mentions_target("native-image", native_image, target_java)?;
        }
    }

    Ok(versions)
}

pub fn validate_run_toolchain(
    config: &ToolchainConfig,
    dry_run: bool,
) -> Result<ToolchainVersions> {
    let versions = probe_versions(config);
    if dry_run {
        return Ok(versions);
    }
    ensure_linux_kvm()?;
    require_executable(&config.firecracker)?;
    require_executable("ip")?;
    Ok(versions)
}

pub fn validate_rootfs_tooling(dry_run: bool) -> Result<String> {
    if dry_run {
        return Ok("dry-run".to_string());
    }
    if command_exists("mkfs.ext4") {
        return Ok("mkfs.ext4".to_string());
    }
    if command_exists("mke2fs") {
        return Ok("mke2fs".to_string());
    }
    bail!("missing ext4 tooling: install `mkfs.ext4` or `mke2fs`");
}

pub fn require_executable(name: &str) -> Result<()> {
    if Path::new(name).is_absolute() && Path::new(name).is_file() {
        return Ok(());
    }
    if command_exists(name) {
        return Ok(());
    }
    bail!("missing required executable `{name}` in PATH");
}

pub fn ensure_linux_kvm() -> Result<()> {
    if !platform_is_linux() {
        bail!(
            "Firecracker requires a Linux host with KVM; this host is `{}`",
            std::env::consts::OS
        );
    }
    if !Path::new("/dev/kvm").exists() {
        bail!("missing /dev/kvm; enable KVM or run on a host with hardware virtualization exposed");
    }
    Ok(())
}

pub fn host_benchmark_info(firecracker: Option<ToolVersion>) -> HostBenchmarkInfo {
    HostBenchmarkInfo {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        kernel: capture_text("uname", &["-r"]),
        cpu: read_cpu_model(),
        firecracker_version: firecracker.map(|version| version.version_output),
    }
}

fn capture_version(executable: &str, args: &[&str]) -> Option<ToolVersion> {
    let output = Command::new(executable).args(args).output().ok()?;
    let mut version_output = String::new();
    version_output.push_str(&String::from_utf8_lossy(&output.stdout));
    version_output.push_str(&String::from_utf8_lossy(&output.stderr));
    let version_output = version_output.trim().to_string();
    if version_output.is_empty() {
        return None;
    }
    Some(ToolVersion {
        executable: executable.to_string(),
        version_output,
    })
}

fn ensure_version_mentions_target(
    name: &str,
    version: &ToolVersion,
    target_java: u16,
) -> Result<()> {
    let raw = &version.version_output;
    let target = target_java.to_string();
    let acceptable = [
        format!(" {target}"),
        format!("\"{target}"),
        format!("{target}."),
        format!("-{target}."),
    ];
    if acceptable.iter().any(|needle| raw.contains(needle)) {
        return Ok(());
    }
    bail!(
        "{name} does not appear to target Java {target_java}. Version output was: {}. Pass --allow-java-mismatch only if you know this compiler supports the target.",
        raw
    );
}

fn capture_text(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn read_cpu_model() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").ok()?;
        for line in cpuinfo.lines() {
            if let Some(model) = line.strip_prefix("model name") {
                return model
                    .split_once(':')
                    .map(|(_, value)| value.trim().to_string());
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        return capture_text("sysctl", &["-n", "machdep.cpu.brand_string"]);
    }

    #[allow(unreachable_code)]
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_toolchain_names_are_expected() {
        let config = ToolchainConfig::default();
        assert_eq!(config.java, "java");
        assert_eq!(config.native_image, "native-image");
        assert_eq!(config.firecracker, "firecracker");
    }
}
