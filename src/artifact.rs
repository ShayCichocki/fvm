use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const SCHEMA_VERSION: u32 = 1;
pub const MANIFEST_FILE: &str = "metadata.json";
pub const FIRECRACKER_CONFIG_FILE: &str = "firecracker.json";
pub const KERNEL_FILE: &str = "kernel";
pub const ROOTFS_FILE: &str = "rootfs.ext4";
pub const APP_BINARY_FILE: &str = "app.bin";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum BuildMode {
    Native,
    SnapshotNative,
    LegacySnapshot,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum BuildBackend {
    #[default]
    Graal,
    FvmAot,
}

impl fmt::Display for BuildBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuildBackend::Graal => write!(f, "graal"),
            BuildBackend::FvmAot => write!(f, "fvm-aot"),
        }
    }
}

impl fmt::Display for BuildMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuildMode::Native => write!(f, "native"),
            BuildMode::SnapshotNative => write!(f, "snapshot-native"),
            BuildMode::LegacySnapshot => write!(f, "legacy-snapshot"),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FrameworkKind {
    PlainJava,
    Micronaut,
    Quarkus,
    SpringBoot,
    ServletWar,
    Unknown,
}

impl fmt::Display for FrameworkKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrameworkKind::PlainJava => write!(f, "plain-java"),
            FrameworkKind::Micronaut => write!(f, "micronaut"),
            FrameworkKind::Quarkus => write!(f, "quarkus"),
            FrameworkKind::SpringBoot => write!(f, "spring-boot"),
            FrameworkKind::ServletWar => write!(f, "servlet-war"),
            FrameworkKind::Unknown => write!(f, "unknown"),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ArtifactManifest {
    pub schema_version: u32,
    pub fvm_version: String,
    pub app: AppInfo,
    pub java: JavaInfo,
    pub target: TargetInfo,
    pub build: BuildInfo,
    pub files: ArtifactFiles,
    pub runtime: RuntimeConfig,
    pub security: SecurityConfig,
    pub snapshots: Vec<SnapshotRecord>,
    pub benchmarks: Vec<BenchmarkReport>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AppInfo {
    pub name: String,
    pub version: Option<String>,
    pub main_class: Option<String>,
    pub frameworks: Vec<FrameworkDetection>,
    pub native_image_metadata_present: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FrameworkDetection {
    pub kind: FrameworkKind,
    pub confidence: String,
    pub evidence: Vec<String>,
    pub supported_in_native: bool,
    pub recommendation: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct JavaInfo {
    pub target_version: u16,
    pub native_image: Option<ToolVersion>,
    pub jdk: Option<ToolVersion>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ToolVersion {
    pub executable: String,
    pub version_output: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TargetInfo {
    pub os: String,
    pub arch: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BuildInfo {
    pub mode: BuildMode,
    #[serde(default)]
    pub backend: BuildBackend,
    pub timestamp_unix_seconds: u64,
    pub input_jar: FileRecord,
    pub dry_run: bool,
    pub native_image_args: Vec<String>,
    pub rootfs_size: String,
    pub init_wrapper: Option<FileRecord>,
    pub jre_source_path: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ArtifactFiles {
    pub kernel: FileRecord,
    pub rootfs: FileRecord,
    pub app_binary: Option<FileRecord>,
    pub firecracker_config: FileRecord,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FileRecord {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RuntimeConfig {
    pub memory_mib: u32,
    pub vcpus: u8,
    pub ports: Vec<PortMapping>,
    pub readiness: Option<ReadinessConfig>,
    pub rootfs_read_only: bool,
    pub boot_args: String,
    pub guest_rss_source: GuestRssSource,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GuestRssSource {
    InitWrapperSerial,
    Unavailable,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PortMapping {
    pub host: u16,
    pub guest: u16,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReadinessConfig {
    pub http_path: String,
    pub timeout_seconds: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SecurityConfig {
    pub guest_uid: Option<u32>,
    pub guest_gid: Option<u32>,
    pub jailer: Option<JailerConfig>,
    pub cgroups: Vec<CgroupSetting>,
    pub secrets: Vec<SecretMount>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct JailerConfig {
    pub executable: String,
    pub uid: u32,
    pub gid: u32,
    pub chroot_base: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CgroupSetting {
    pub key: String,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SecretMount {
    pub name: String,
    pub source_path: String,
    pub guest_path: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SnapshotRecord {
    pub name: String,
    pub created_unix_seconds: u64,
    pub mem_file: FileRecord,
    pub vmstate_file: FileRecord,
    pub readiness: Option<ReadinessConfig>,
    pub restored_verified: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BenchmarkReport {
    pub name: String,
    pub created_unix_seconds: u64,
    pub iterations: Vec<BenchmarkIteration>,
    pub summary: BenchmarkSummary,
    pub host: HostBenchmarkInfo,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BenchmarkIteration {
    pub boot_to_listen_ms: Option<u128>,
    pub host_rss_kib: Option<u64>,
    pub guest_rss_kib: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct BenchmarkSummary {
    pub boot_to_listen_ms_median: Option<u128>,
    pub boot_to_listen_ms_p90: Option<u128>,
    pub boot_to_listen_ms_p99: Option<u128>,
    pub host_rss_kib_max: Option<u64>,
    pub guest_rss_kib_max: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HostBenchmarkInfo {
    pub os: String,
    pub arch: String,
    pub kernel: Option<String>,
    pub cpu: Option<String>,
    pub firecracker_version: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticLevel {
    Info,
    Warning,
    Error,
}

impl ArtifactManifest {
    pub fn save(&self, artifact_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(artifact_dir).with_context(|| {
            format!(
                "failed to create artifact directory {}",
                artifact_dir.display()
            )
        })?;
        let path = artifact_dir.join(MANIFEST_FILE);
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, format!("{json}\n"))
            .with_context(|| format!("failed to write manifest {}", path.display()))?;
        Ok(())
    }

    pub fn load(artifact_dir: &Path) -> Result<Self> {
        let path = artifact_dir.join(MANIFEST_FILE);
        let bytes = std::fs::read(&path)
            .with_context(|| format!("failed to read manifest {}", path.display()))?;
        let manifest: ArtifactManifest = serde_json::from_slice(&bytes)
            .with_context(|| format!("failed to parse manifest {}", path.display()))?;
        if manifest.schema_version != SCHEMA_VERSION {
            bail!(
                "unsupported artifact schema {} in {}; expected {}",
                manifest.schema_version,
                path.display(),
                SCHEMA_VERSION
            );
        }
        Ok(manifest)
    }

    pub fn verify_files(&self, artifact_dir: &Path) -> Result<Vec<FileVerification>> {
        let mut records = vec![
            ("kernel", &self.files.kernel),
            ("rootfs", &self.files.rootfs),
            ("firecracker_config", &self.files.firecracker_config),
        ];

        if let Some(app) = &self.files.app_binary {
            records.push(("app_binary", app));
        }

        for snapshot in &self.snapshots {
            records.push(("snapshot_mem", &snapshot.mem_file));
            records.push(("snapshot_vmstate", &snapshot.vmstate_file));
        }

        let mut results = Vec::with_capacity(records.len());
        for (name, record) in records {
            let path = artifact_dir.join(&record.path);
            let actual = record_file(artifact_dir, &path)
                .with_context(|| format!("failed to verify {name} at {}", path.display()))?;
            let ok = record.sha256 == actual.sha256 && record.size == actual.size;
            results.push(FileVerification {
                name: name.to_string(),
                path: record.path.clone(),
                expected_sha256: record.sha256.clone(),
                actual_sha256: actual.sha256,
                expected_size: record.size,
                actual_size: actual.size,
                ok,
            });
        }

        Ok(results)
    }
}

#[derive(Clone, Debug)]
pub struct FileVerification {
    pub name: String,
    pub path: String,
    pub expected_sha256: String,
    pub actual_sha256: String,
    pub expected_size: u64,
    pub actual_size: u64,
    pub ok: bool,
}

pub fn record_file(base_dir: &Path, path: &Path) -> Result<FileRecord> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("failed to read file metadata for {}", path.display()))?;
    let relative = path
        .strip_prefix(base_dir)
        .with_context(|| {
            format!(
                "file {} is not inside artifact directory {}",
                path.display(),
                base_dir.display()
            )
        })?
        .to_string_lossy()
        .replace('\\', "/");

    Ok(FileRecord {
        path: relative,
        size: metadata.len(),
        sha256: sha256_file(path)?,
    })
}

pub fn record_external_file(path: &Path) -> Result<FileRecord> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("failed to read file metadata for {}", path.display()))?;
    Ok(FileRecord {
        path: path.to_string_lossy().to_string(),
        size: metadata.len(),
        sha256: sha256_file(path)?,
    })
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file =
        File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buf)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn artifact_dir_from_output(output: Option<PathBuf>, jar: &Path) -> PathBuf {
    output.unwrap_or_else(|| {
        let stem = jar
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("app")
            .to_string();
        PathBuf::from(format!("{stem}.fvm"))
    })
}

pub fn parse_port_mapping(raw: &str) -> Result<PortMapping> {
    let Some((host, guest)) = raw.split_once(':') else {
        bail!("port mapping must be HOST:GUEST, got `{raw}`");
    };
    Ok(PortMapping {
        host: host
            .parse()
            .with_context(|| format!("invalid host port `{host}`"))?,
        guest: guest
            .parse()
            .with_context(|| format!("invalid guest port `{guest}`"))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_port_mapping() {
        let mapping = parse_port_mapping("8080:9090").unwrap();
        assert_eq!(mapping.host, 8080);
        assert_eq!(mapping.guest, 9090);
    }

    #[test]
    fn rejects_invalid_port_mapping() {
        assert!(parse_port_mapping("8080").is_err());
        assert!(parse_port_mapping("x:8080").is_err());
    }

    #[test]
    fn records_file_relative_to_artifact_dir() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("a.txt");
        std::fs::write(&file, "hello").unwrap();
        let record = record_file(temp.path(), &file).unwrap();
        assert_eq!(record.path, "a.txt");
        assert_eq!(record.size, 5);
        assert_eq!(
            record.sha256,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }
}
