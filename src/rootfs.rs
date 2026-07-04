use crate::artifact::{BuildMode, GuestRssSource};
use crate::command_util::run_capture;
use crate::toolchain;
use anyhow::{Context, Result, bail};
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const DEFAULT_BOOT_ARGS_SUFFIX: &str =
    "root=/dev/vda ro console=ttyS0 quiet loglevel=0 reboot=k panic=1 pci=off";

#[derive(Clone, Debug)]
pub struct RootfsSpec {
    pub mode: BuildMode,
    pub rootfs_path: PathBuf,
    pub app_binary: Option<PathBuf>,
    pub input_jar: PathBuf,
    pub jre_dir: Option<PathBuf>,
    pub init_wrapper: Option<PathBuf>,
    pub init_mode: InitMode,
    pub rootfs_size: String,
    pub dry_run: bool,
    pub guest_uid: Option<u32>,
    pub guest_gid: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InitMode {
    Monitor,
    Exec,
}

impl InitMode {
    fn as_config_value(self) -> &'static str {
        match self {
            InitMode::Monitor => "monitor",
            InitMode::Exec => "exec",
        }
    }
}

#[derive(Clone, Debug)]
pub struct RootfsResult {
    pub init_path: String,
    pub guest_rss_source: GuestRssSource,
}

pub fn assemble_rootfs(spec: &RootfsSpec) -> Result<RootfsResult> {
    let staging = tempfile::tempdir().context("failed to create rootfs staging directory")?;
    create_base_dirs(staging.path())?;

    let result = match spec.mode {
        BuildMode::Native | BuildMode::SnapshotNative => stage_native_rootfs(spec, staging.path())?,
        BuildMode::LegacySnapshot => stage_legacy_rootfs(spec, staging.path())?,
    };

    create_ext4_image(
        staging.path(),
        &spec.rootfs_path,
        &spec.rootfs_size,
        spec.dry_run,
    )?;
    Ok(result)
}

pub fn boot_args(init_path: &str) -> String {
    format!("init={init_path} {DEFAULT_BOOT_ARGS_SUFFIX}")
}

fn create_base_dirs(staging: &Path) -> Result<()> {
    for dir in ["dev", "proc", "sys", "tmp", "etc"] {
        std::fs::create_dir_all(staging.join(dir))
            .with_context(|| format!("failed to create rootfs directory /{dir}"))?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(staging.join("tmp"), std::fs::Permissions::from_mode(0o1777))?;
    }

    if Path::new("/etc/resolv.conf").is_file() {
        let _ = std::fs::copy("/etc/resolv.conf", staging.join("etc/resolv.conf"));
    }

    Ok(())
}

fn stage_native_rootfs(spec: &RootfsSpec, staging: &Path) -> Result<RootfsResult> {
    let app_binary = spec
        .app_binary
        .as_ref()
        .context("native rootfs requires an app binary")?;
    let guest_app_path = staging.join("app");
    std::fs::copy(app_binary, &guest_app_path).with_context(|| {
        format!(
            "failed to copy native binary {} into rootfs",
            app_binary.display()
        )
    })?;
    make_executable(&guest_app_path)?;
    copy_dynamic_libraries(app_binary, staging, spec.dry_run)?;

    if let Some(init_wrapper) = &spec.init_wrapper {
        stage_init_wrapper(
            init_wrapper,
            staging,
            InitConfig {
                exec: "/app".to_string(),
                args: Vec::new(),
                mode: spec.init_mode,
                guest_uid: spec.guest_uid,
                guest_gid: spec.guest_gid,
            },
            spec.dry_run,
        )?;
        Ok(RootfsResult {
            init_path: "/init".to_string(),
            guest_rss_source: if spec.init_mode == InitMode::Monitor {
                GuestRssSource::InitWrapperSerial
            } else {
                GuestRssSource::Unavailable
            },
        })
    } else {
        Ok(RootfsResult {
            init_path: "/app".to_string(),
            guest_rss_source: GuestRssSource::Unavailable,
        })
    }
}

fn stage_legacy_rootfs(spec: &RootfsSpec, staging: &Path) -> Result<RootfsResult> {
    let init_wrapper = spec
        .init_wrapper
        .as_ref()
        .context("legacy-snapshot mode requires --init-wrapper or an auto-detected fvm-init")?;
    let jre_dir = spec
        .jre_dir
        .as_ref()
        .context("legacy-snapshot mode requires --jre pointing at a Java runtime directory")?;

    std::fs::copy(&spec.input_jar, staging.join("app.jar")).with_context(|| {
        format!(
            "failed to copy legacy JAR {} into rootfs",
            spec.input_jar.display()
        )
    })?;
    copy_dir_recursive(jre_dir, &staging.join("jre"))?;
    stage_init_wrapper(
        init_wrapper,
        staging,
        InitConfig {
            exec: "/jre/bin/java".to_string(),
            args: vec!["-jar".to_string(), "/app.jar".to_string()],
            mode: InitMode::Monitor,
            guest_uid: spec.guest_uid,
            guest_gid: spec.guest_gid,
        },
        spec.dry_run,
    )?;

    Ok(RootfsResult {
        init_path: "/init".to_string(),
        guest_rss_source: GuestRssSource::InitWrapperSerial,
    })
}

struct InitConfig {
    exec: String,
    args: Vec<String>,
    mode: InitMode,
    guest_uid: Option<u32>,
    guest_gid: Option<u32>,
}

fn stage_init_wrapper(
    init_wrapper: &Path,
    staging: &Path,
    config: InitConfig,
    dry_run: bool,
) -> Result<()> {
    let guest_init = staging.join("init");
    std::fs::copy(init_wrapper, &guest_init).with_context(|| {
        format!(
            "failed to copy init wrapper {} into rootfs",
            init_wrapper.display()
        )
    })?;
    make_executable(&guest_init)?;
    copy_dynamic_libraries(init_wrapper, staging, dry_run)?;

    let mut file = std::fs::File::create(staging.join("etc/fvm-init.conf"))?;
    writeln!(file, "exec={}", config.exec)?;
    writeln!(file, "mode={}", config.mode.as_config_value())?;
    for arg in config.args {
        writeln!(file, "arg={arg}")?;
    }
    if let Some(uid) = config.guest_uid {
        writeln!(file, "uid={uid}")?;
    }
    if let Some(gid) = config.guest_gid {
        writeln!(file, "gid={gid}")?;
    }
    writeln!(file, "rss_interval_ms=250")?;
    Ok(())
}

fn create_ext4_image(staging: &Path, output: &Path, size: &str, dry_run: bool) -> Result<()> {
    if dry_run {
        std::fs::write(
            output,
            format!(
                "dry-run ext4 image placeholder\nstaging={}\nsize={}\n",
                staging.display(),
                size
            ),
        )?;
        return Ok(());
    }

    let mkfs = toolchain::validate_rootfs_tooling(false)?;
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;

    run_capture(
        "truncate",
        [
            OsString::from("-s"),
            OsString::from(size),
            output.as_os_str().to_os_string(),
        ],
    )
    .context("failed to allocate rootfs image file; install `truncate` from coreutils")?;

    run_capture(
        &mkfs,
        [
            OsString::from("-q"),
            OsString::from("-F"),
            OsString::from("-t"),
            OsString::from("ext4"),
            OsString::from("-d"),
            staging.as_os_str().to_os_string(),
            output.as_os_str().to_os_string(),
        ],
    )?;

    Ok(())
}

fn copy_dynamic_libraries(binary: &Path, staging: &Path, dry_run: bool) -> Result<()> {
    if dry_run || std::env::consts::OS != "linux" {
        return Ok(());
    }

    let output = std::process::Command::new("ldd")
        .arg(binary)
        .output()
        .with_context(|| format!("failed to run ldd on {}", binary.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.contains("not a dynamic executable")
            || stderr.contains("not a dynamic executable")
        {
            return Ok(());
        }
        bail!("ldd failed for {}: {stderr}{stdout}", binary.display());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for path in parse_ldd_paths(&stdout) {
        copy_abs_path_into_rootfs(&path, staging)?;
    }
    Ok(())
}

fn parse_ldd_paths(output: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if let Some((_, rhs)) = line.split_once("=>") {
            let path = rhs.split_whitespace().next().unwrap_or_default();
            if path.starts_with('/') {
                paths.push(PathBuf::from(path));
            }
        } else if let Some(path) = line.split_whitespace().next()
            && path.starts_with('/')
        {
            paths.push(PathBuf::from(path));
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

fn copy_abs_path_into_rootfs(path: &Path, staging: &Path) -> Result<()> {
    let relative = path.strip_prefix("/")?;
    let target = staging.join(relative);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(path, &target)
        .with_context(|| format!("failed to copy dynamic dependency {}", path.display()))?;
    Ok(())
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<()> {
    if !source.is_dir() {
        bail!("{} is not a directory", source.display());
    }
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_dir_recursive(&source_path, &destination_path)?;
        } else if file_type.is_symlink() {
            #[cfg(unix)]
            {
                let target = std::fs::read_link(&source_path)?;
                std::os::unix::fs::symlink(target, destination_path)?;
            }
            #[cfg(not(unix))]
            {
                std::fs::copy(&source_path, &destination_path)?;
            }
        } else if file_type.is_file() {
            std::fs::copy(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

fn make_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path)?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_args_uses_requested_init() {
        assert_eq!(
            boot_args("/init"),
            "init=/init root=/dev/vda ro console=ttyS0 quiet loglevel=0 reboot=k panic=1 pci=off"
        );
    }

    #[test]
    fn parses_ldd_paths() {
        let paths = parse_ldd_paths(
            "linux-vdso.so.1 (0x00007ffd)\nlibc.so.6 => /lib/x86_64-linux-gnu/libc.so.6 (0x00007f)\n/lib64/ld-linux-x86-64.so.2 (0x00007f)\n",
        );
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&PathBuf::from("/lib/x86_64-linux-gnu/libc.so.6")));
        assert!(paths.contains(&PathBuf::from("/lib64/ld-linux-x86-64.so.2")));
    }
}
