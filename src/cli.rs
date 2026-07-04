use crate::artifact::{
    APP_BINARY_FILE, AppInfo, ArtifactFiles, ArtifactManifest, BuildBackend, BuildInfo, BuildMode,
    CgroupSetting, Diagnostic, DiagnosticLevel, FIRECRACKER_CONFIG_FILE, GuestRssSource, JavaInfo,
    KERNEL_FILE, PortMapping, ROOTFS_FILE, ReadinessConfig, RuntimeConfig, SCHEMA_VERSION,
    SecretMount, SecurityConfig, SnapshotRecord, TargetInfo, artifact_dir_from_output,
    now_unix_seconds, parse_port_mapping, record_external_file, record_file,
};
use crate::benchmark;
use crate::command_util::run_streaming;
use crate::firecracker::{self, LaunchSpec, SnapshotLoadSpec};
use crate::framework::{analyze_jar, ensure_supported_for_mode};
use crate::guest_init::resolve_init_wrapper;
use crate::rootfs::{self, RootfsSpec};
use crate::toolchain::{self, ToolchainConfig};
use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[derive(Debug, Parser)]
#[command(name = "fvm")]
#[command(about = "Firecracker-native deployment toolchain for Java applications")]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)]
enum Commands {
    Build(BuildArgs),
    Run(RunArgs),
    Snapshot(SnapshotArgs),
    Inspect(InspectArgs),
    Math(MathArgs),
    Analyze(AnalyzeArgs),
    Doctor(DoctorArgs),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum InitModeArg {
    Monitor,
    Exec,
}

#[derive(Debug, Args)]
struct BuildArgs {
    jar: PathBuf,

    #[arg(long, value_enum, default_value_t = BuildMode::Native)]
    mode: BuildMode,

    #[arg(long, value_enum, default_value_t = BuildBackend::Graal)]
    backend: BuildBackend,

    #[arg(long, default_value_t = 25)]
    java: u16,

    #[arg(long)]
    output: Option<PathBuf>,

    #[arg(long)]
    name: Option<String>,

    #[arg(long)]
    version: Option<String>,

    #[arg(long)]
    main_class: Option<String>,

    #[arg(long)]
    kernel: Option<PathBuf>,

    #[arg(long)]
    jre: Option<PathBuf>,

    #[arg(long, default_value = "auto")]
    init_wrapper: String,

    #[arg(long, value_enum, default_value_t = InitModeArg::Monitor)]
    init_mode: InitModeArg,

    #[arg(long)]
    allow_missing_guest_rss: bool,

    #[arg(long)]
    allow_java_mismatch: bool,

    #[arg(long)]
    allow_unsupported_framework: bool,

    #[arg(long, default_value = "64M", value_parser = parse_memory_mib)]
    memory: u32,

    #[arg(long, default_value_t = 1)]
    vcpus: u8,

    #[arg(long, default_value = "8080:8080")]
    port: Vec<String>,

    #[arg(long, default_value = "/health")]
    readiness_http: String,

    #[arg(long, default_value_t = 10)]
    readiness_timeout: u64,

    #[arg(long, default_value = "64M")]
    rootfs_size: String,

    #[arg(long, default_value = "java")]
    java_exec: String,

    #[arg(long, default_value = "native-image")]
    native_image: String,

    #[arg(long, default_value = "cc")]
    cc: String,

    #[arg(long, default_value = "firecracker")]
    firecracker: String,

    #[arg(long)]
    native_image_arg: Vec<String>,

    #[arg(long)]
    native_config_dir: Vec<PathBuf>,

    #[arg(long)]
    guest_uid: Option<u32>,

    #[arg(long)]
    guest_gid: Option<u32>,

    #[arg(long)]
    cgroup: Vec<String>,

    #[arg(long)]
    secret: Vec<String>,

    #[arg(long)]
    dry_run: bool,

    #[arg(long)]
    force: bool,
}

#[derive(Debug, Args)]
struct RunArgs {
    artifact: PathBuf,

    #[arg(long, value_parser = parse_memory_mib)]
    memory: Option<u32>,

    #[arg(long)]
    vcpus: Option<u8>,

    #[arg(long)]
    port: Vec<String>,

    #[arg(long)]
    wait_http: Option<String>,

    #[arg(long, default_value_t = 10)]
    readiness_timeout: u64,

    #[arg(long)]
    snapshot: Option<String>,

    #[arg(long, default_value_t = 1)]
    benchmark_iterations: u16,

    #[arg(long)]
    once: bool,

    #[arg(long)]
    no_record_benchmark: bool,

    #[arg(long, default_value = "firecracker")]
    firecracker: String,

    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Args)]
struct SnapshotArgs {
    artifact: PathBuf,

    #[arg(long, default_value = "initialized")]
    name: String,

    #[arg(long)]
    wait_http: Option<String>,

    #[arg(long, default_value_t = 10)]
    readiness_timeout: u64,

    #[arg(long)]
    port: Vec<String>,

    #[arg(long)]
    verify_restore: bool,

    #[arg(long, default_value_t = 1000)]
    stabilize_ms: u64,

    #[arg(long, default_value = "firecracker")]
    firecracker: String,

    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Args)]
struct InspectArgs {
    artifact: PathBuf,

    #[arg(long)]
    json: bool,

    #[arg(long)]
    verify: bool,
}

#[derive(Debug, Args)]
struct MathArgs {
    artifact: PathBuf,

    #[arg(long, default_value = "32G", value_parser = parse_memory_mib)]
    host_memory: u32,

    #[arg(long, default_value = "2G", value_parser = parse_memory_mib)]
    reserve: u32,

    #[arg(long, value_parser = parse_memory_mib)]
    baseline_host_rss: Option<u32>,

    #[arg(long)]
    baseline_boot_ms: Option<u128>,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct AnalyzeArgs {
    jar: PathBuf,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct DoctorArgs {
    #[arg(long, default_value = "java")]
    java_exec: String,

    #[arg(long, default_value = "native-image")]
    native_image: String,

    #[arg(long, default_value = "firecracker")]
    firecracker: String,

    #[arg(long)]
    strict: bool,
}

pub fn execute(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Build(args) => build(args),
        Commands::Run(args) => run(args),
        Commands::Snapshot(args) => snapshot(args),
        Commands::Inspect(args) => inspect(args),
        Commands::Math(args) => math(args),
        Commands::Analyze(args) => analyze(args),
        Commands::Doctor(args) => doctor(args),
    }
}

fn build(args: BuildArgs) -> Result<()> {
    if !args.jar.is_file() {
        bail!("input JAR {} does not exist", args.jar.display());
    }
    if args.vcpus == 0 {
        bail!("--vcpus must be greater than zero");
    }
    if args.backend == BuildBackend::FvmAot && args.mode == BuildMode::LegacySnapshot {
        bail!("--backend fvm-aot cannot be used with --mode legacy-snapshot");
    }
    if args.backend == BuildBackend::FvmAot
        && (!args.native_image_arg.is_empty() || !args.native_config_dir.is_empty())
    {
        bail!("--backend fvm-aot does not accept native-image args or config directories");
    }

    let artifact_dir = artifact_dir_from_output(args.output.clone(), &args.jar);
    prepare_artifact_dir(&artifact_dir, args.force)?;

    let analysis = analyze_jar(&args.jar)?;
    ensure_supported_for_mode(args.mode, &analysis, args.allow_unsupported_framework)?;

    let toolchain_config = ToolchainConfig {
        java: args.java_exec.clone(),
        native_image: args.native_image.clone(),
        firecracker: args.firecracker.clone(),
    };
    let java_required =
        args.backend == BuildBackend::Graal || args.mode == BuildMode::LegacySnapshot;
    let native_image_required =
        args.backend == BuildBackend::Graal && args.mode != BuildMode::LegacySnapshot;
    let versions = toolchain::validate_build_toolchain(
        &toolchain_config,
        args.java,
        args.dry_run,
        args.allow_java_mismatch,
        java_required,
        native_image_required,
    )?;

    let require_guest_rss = !args.allow_missing_guest_rss;
    let init_wrapper = resolve_init_wrapper(
        &args.init_wrapper,
        args.dry_run,
        require_guest_rss,
        args.mode == BuildMode::LegacySnapshot,
    )?;
    if args.init_mode == InitModeArg::Exec && init_wrapper.is_none() && !args.dry_run {
        bail!(
            "--init-mode exec requires fvm-init; pass --init-wrapper /path/to/fvm-init or build fvm-init next to fvm"
        );
    }

    let app_binary_path = artifact_dir.join(APP_BINARY_FILE);
    let app_binary = match args.mode {
        BuildMode::Native | BuildMode::SnapshotNative => {
            build_native_binary(&args, &analysis.main_class, &app_binary_path)?;
            Some(app_binary_path.clone())
        }
        BuildMode::LegacySnapshot => None,
    };

    let kernel_path = artifact_dir.join(KERNEL_FILE);
    copy_kernel(args.kernel.as_deref(), &kernel_path, args.dry_run)?;

    let rootfs_path = artifact_dir.join(ROOTFS_FILE);
    let rootfs_result = rootfs::assemble_rootfs(&RootfsSpec {
        mode: args.mode,
        rootfs_path: rootfs_path.clone(),
        app_binary: app_binary.clone(),
        input_jar: args.jar.clone(),
        jre_dir: args.jre.clone(),
        init_wrapper: init_wrapper.clone(),
        init_mode: match args.init_mode {
            InitModeArg::Monitor => rootfs::InitMode::Monitor,
            InitModeArg::Exec => rootfs::InitMode::Exec,
        },
        rootfs_size: args.rootfs_size.clone(),
        dry_run: args.dry_run,
        guest_uid: args.guest_uid,
        guest_gid: args.guest_gid,
    })?;

    let boot_args = rootfs::boot_args(&rootfs_result.init_path);
    let firecracker_config_path = artifact_dir.join(FIRECRACKER_CONFIG_FILE);
    firecracker::write_firecracker_config(
        &firecracker_config_path,
        &firecracker::FirecrackerConfigSpec {
            kernel_path: PathBuf::from(KERNEL_FILE),
            rootfs_path: PathBuf::from(ROOTFS_FILE),
            boot_args: boot_args.clone(),
            memory_mib: args.memory,
            vcpus: args.vcpus,
            rootfs_read_only: true,
            tap_name: None,
            guest_mac: None,
            log_path: None,
            metrics_path: None,
            track_dirty_pages: args.mode != BuildMode::Native,
        },
    )?;

    let ports = parse_ports(&args.port)?;
    let app_name = args
        .name
        .clone()
        .unwrap_or_else(|| default_app_name(&args.jar));
    let main_class = args.main_class.clone().or(analysis.main_class.clone());
    let diagnostics = build_diagnostics(
        &analysis.frameworks,
        rootfs_result.guest_rss_source == GuestRssSource::Unavailable,
    );

    let manifest = ArtifactManifest {
        schema_version: SCHEMA_VERSION,
        fvm_version: env!("CARGO_PKG_VERSION").to_string(),
        app: AppInfo {
            name: app_name,
            version: args.version.clone(),
            main_class,
            frameworks: analysis.frameworks.clone(),
            native_image_metadata_present: analysis.native_image_metadata_present,
        },
        java: JavaInfo {
            target_version: args.java,
            native_image: if native_image_required {
                versions.native_image
            } else {
                None
            },
            jdk: if java_required { versions.java } else { None },
        },
        target: TargetInfo {
            os: "linux".to_string(),
            arch: "x86_64".to_string(),
        },
        build: BuildInfo {
            mode: args.mode,
            backend: args.backend,
            timestamp_unix_seconds: now_unix_seconds(),
            input_jar: record_external_file(&args.jar)?,
            dry_run: args.dry_run,
            native_image_args: if args.backend == BuildBackend::Graal {
                resolved_native_image_args(&args, &analysis.main_class)
            } else {
                Vec::new()
            },
            rootfs_size: args.rootfs_size.clone(),
            init_wrapper: init_wrapper
                .as_ref()
                .map(|path| record_external_file(path))
                .transpose()?,
            jre_source_path: args
                .jre
                .as_ref()
                .map(|path| path.to_string_lossy().to_string()),
        },
        files: ArtifactFiles {
            kernel: record_file(&artifact_dir, &kernel_path)?,
            rootfs: record_file(&artifact_dir, &rootfs_path)?,
            app_binary: app_binary
                .as_ref()
                .map(|path| record_file(&artifact_dir, path))
                .transpose()?,
            firecracker_config: record_file(&artifact_dir, &firecracker_config_path)?,
        },
        runtime: RuntimeConfig {
            memory_mib: args.memory,
            vcpus: args.vcpus,
            ports,
            readiness: Some(ReadinessConfig {
                http_path: args.readiness_http.clone(),
                timeout_seconds: args.readiness_timeout,
            }),
            rootfs_read_only: true,
            boot_args,
            guest_rss_source: rootfs_result.guest_rss_source,
        },
        security: SecurityConfig {
            guest_uid: args.guest_uid,
            guest_gid: args.guest_gid,
            jailer: None,
            cgroups: parse_cgroups(&args.cgroup)?,
            secrets: parse_secrets(&args.secret)?,
        },
        snapshots: Vec::new(),
        benchmarks: Vec::new(),
        diagnostics,
    };

    manifest.save(&artifact_dir)?;
    println!("built artifact: {}", artifact_dir.display());
    println!("mode: {}", args.mode);
    println!("java target: {}", args.java);
    println!("guest rss source: {:?}", manifest.runtime.guest_rss_source);
    Ok(())
}

fn run(args: RunArgs) -> Result<()> {
    let mut manifest = ArtifactManifest::load(&args.artifact)?;
    let toolchain_config = ToolchainConfig {
        firecracker: args.firecracker.clone(),
        ..ToolchainConfig::default()
    };
    let versions = toolchain::validate_run_toolchain(&toolchain_config, args.dry_run)?;

    let ports = if args.port.is_empty() {
        manifest.runtime.ports.clone()
    } else {
        parse_ports(&args.port)?
    };
    let readiness = readiness_override(
        manifest.runtime.readiness.clone(),
        args.wait_http.as_deref(),
        args.readiness_timeout,
    );
    let snapshot_load = resolve_snapshot_load(&args.artifact, &manifest, args.snapshot.as_deref())?;
    let iterations = args.benchmark_iterations.max(1);
    let stop_after_readiness = args.once || iterations > 1 || args.dry_run;

    let mut benchmark_iterations = Vec::new();
    for iteration in 0..iterations {
        if iterations > 1 {
            println!("benchmark iteration {}/{}", iteration + 1, iterations);
        }
        let mut vm = firecracker::launch_vm(&LaunchSpec {
            firecracker: args.firecracker.clone(),
            kernel_path: args.artifact.join(&manifest.files.kernel.path),
            rootfs_path: args.artifact.join(&manifest.files.rootfs.path),
            boot_args: manifest.runtime.boot_args.clone(),
            memory_mib: args.memory.unwrap_or(manifest.runtime.memory_mib),
            vcpus: args.vcpus.unwrap_or(manifest.runtime.vcpus),
            rootfs_read_only: manifest.runtime.rootfs_read_only,
            ports: ports.clone(),
            dry_run: args.dry_run,
            track_dirty_pages: false,
            snapshot_load: snapshot_load.clone(),
            cgroups: manifest.security.cgroups.clone(),
        })?;
        let boot_to_listen_ms = vm.wait_for_readiness(&readiness)?;
        let metrics = vm.metrics_after_readiness(boot_to_listen_ms);
        println_metrics(&metrics);
        benchmark_iterations.push(crate::artifact::BenchmarkIteration {
            boot_to_listen_ms: metrics.boot_to_listen_ms,
            host_rss_kib: metrics.host_rss_kib,
            guest_rss_kib: metrics.guest_rss_kib,
        });

        if stop_after_readiness {
            vm.shutdown()?;
        } else {
            if !args.no_record_benchmark {
                record_benchmark(
                    &args.artifact,
                    &mut manifest,
                    "run",
                    benchmark_iterations.clone(),
                    versions.firecracker.clone(),
                )?;
            }
            println!("VM is running; stop Firecracker or interrupt this command to exit");
            return vm.wait();
        }
    }

    if !args.no_record_benchmark {
        record_benchmark(
            &args.artifact,
            &mut manifest,
            "run",
            benchmark_iterations,
            versions.firecracker,
        )?;
    }

    Ok(())
}

fn snapshot(args: SnapshotArgs) -> Result<()> {
    let mut manifest = ArtifactManifest::load(&args.artifact)?;
    let toolchain_config = ToolchainConfig {
        firecracker: args.firecracker.clone(),
        ..ToolchainConfig::default()
    };
    toolchain::validate_run_toolchain(&toolchain_config, args.dry_run)?;

    let ports = if args.port.is_empty() {
        manifest.runtime.ports.clone()
    } else {
        parse_ports(&args.port)?
    };
    let readiness = readiness_override(
        manifest.runtime.readiness.clone(),
        args.wait_http.as_deref(),
        args.readiness_timeout,
    );
    let snapshots_dir = args.artifact.join("snapshots");
    std::fs::create_dir_all(&snapshots_dir)?;
    let mem_path = snapshots_dir.join(format!("{}.mem", args.name));
    let vmstate_path = snapshots_dir.join(format!("{}.vmstate", args.name));

    let mut vm = firecracker::launch_vm(&LaunchSpec {
        firecracker: args.firecracker.clone(),
        kernel_path: args.artifact.join(&manifest.files.kernel.path),
        rootfs_path: args.artifact.join(&manifest.files.rootfs.path),
        boot_args: manifest.runtime.boot_args.clone(),
        memory_mib: manifest.runtime.memory_mib,
        vcpus: manifest.runtime.vcpus,
        rootfs_read_only: manifest.runtime.rootfs_read_only,
        ports: ports.clone(),
        dry_run: args.dry_run,
        track_dirty_pages: true,
        snapshot_load: None,
        cgroups: manifest.security.cgroups.clone(),
    })?;
    vm.wait_for_readiness(&readiness)?;
    if !args.dry_run && args.stabilize_ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(args.stabilize_ms));
    }
    vm.create_snapshot(&mem_path, &vmstate_path)?;
    vm.shutdown()?;
    drop(vm);

    let mut restored_verified = false;
    if args.verify_restore {
        let mut restored = firecracker::launch_vm(&LaunchSpec {
            firecracker: args.firecracker.clone(),
            kernel_path: args.artifact.join(&manifest.files.kernel.path),
            rootfs_path: args.artifact.join(&manifest.files.rootfs.path),
            boot_args: manifest.runtime.boot_args.clone(),
            memory_mib: manifest.runtime.memory_mib,
            vcpus: manifest.runtime.vcpus,
            rootfs_read_only: manifest.runtime.rootfs_read_only,
            ports,
            dry_run: args.dry_run,
            track_dirty_pages: false,
            snapshot_load: Some(SnapshotLoadSpec {
                mem_path: mem_path.clone(),
                vmstate_path: vmstate_path.clone(),
            }),
            cgroups: manifest.security.cgroups.clone(),
        })?;
        restored.wait_for_readiness(&readiness)?;
        restored.shutdown()?;
        restored_verified = true;
    }

    manifest
        .snapshots
        .retain(|snapshot| snapshot.name != args.name);
    manifest.snapshots.push(SnapshotRecord {
        name: args.name.clone(),
        created_unix_seconds: now_unix_seconds(),
        mem_file: record_file(&args.artifact, &mem_path)?,
        vmstate_file: record_file(&args.artifact, &vmstate_path)?,
        readiness,
        restored_verified,
    });
    manifest.save(&args.artifact)?;
    println!("snapshot created: {}", args.name);
    Ok(())
}

fn inspect(args: InspectArgs) -> Result<()> {
    let manifest = ArtifactManifest::load(&args.artifact)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&manifest)?);
        return Ok(());
    }

    println!("artifact: {}", args.artifact.display());
    println!("app: {}", manifest.app.name);
    println!("mode: {}", manifest.build.mode);
    println!("backend: {}", manifest.build.backend);
    println!("java: {}", manifest.java.target_version);
    println!("target: {}/{}", manifest.target.os, manifest.target.arch);
    println!("memory: {} MiB", manifest.runtime.memory_mib);
    println!("vcpus: {}", manifest.runtime.vcpus);
    println!("rootfs: {}", manifest.files.rootfs.path);
    println!("kernel: {}", manifest.files.kernel.path);
    println!("guest rss source: {:?}", manifest.runtime.guest_rss_source);
    if !manifest.runtime.ports.is_empty() {
        let ports = manifest
            .runtime
            .ports
            .iter()
            .map(|port| format!("{}:{}", port.host, port.guest))
            .collect::<Vec<_>>()
            .join(", ");
        println!("ports: {ports}");
    }
    if !manifest.snapshots.is_empty() {
        println!("snapshots:");
        for snapshot in &manifest.snapshots {
            println!(
                "  {} verified={}",
                snapshot.name, snapshot.restored_verified
            );
        }
    }
    if let Some(benchmark) = manifest.benchmarks.last() {
        println!("latest benchmark: {}", benchmark.name);
        println!(
            "  boot median: {:?} ms",
            benchmark.summary.boot_to_listen_ms_median
        );
        println!(
            "  host rss max: {:?} KiB",
            benchmark.summary.host_rss_kib_max
        );
        println!(
            "  guest rss max: {:?} KiB",
            benchmark.summary.guest_rss_kib_max
        );
    }
    if !manifest.diagnostics.is_empty() {
        println!("diagnostics:");
        for diagnostic in &manifest.diagnostics {
            println!("  {:?}: {}", diagnostic.level, diagnostic.message);
        }
    }

    if args.verify {
        println!("verification:");
        for result in manifest.verify_files(&args.artifact)? {
            println!(
                "  {} {} size={}/{} sha={}/{}",
                if result.ok { "ok" } else { "bad" },
                result.path,
                result.actual_size,
                result.expected_size,
                &result.actual_sha256[..12.min(result.actual_sha256.len())],
                &result.expected_sha256[..12.min(result.expected_sha256.len())],
            );
            if !result.ok {
                bail!("artifact verification failed for {}", result.name);
            }
        }
    }

    Ok(())
}

fn math(args: MathArgs) -> Result<()> {
    let manifest = ArtifactManifest::load(&args.artifact)?;
    let benchmark = manifest
        .benchmarks
        .last()
        .with_context(|| format!("artifact {} has no benchmark data", args.artifact.display()))?;
    let report = MathReport::from_args(&manifest, benchmark, &args)?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("artifact: {}", args.artifact.display());
    println!("benchmark: {}", benchmark.name);
    println!("iterations: {}", benchmark.iterations.len());
    println!("boot median: {:?} ms", report.boot_median_ms);
    println!("boot p90: {:?} ms", report.boot_p90_ms);
    println!("boot p99: {:?} ms", report.boot_p99_ms);
    println!("host rss max: {:?} MiB", report.host_rss_max_mib);
    println!("guest rss max: {:?} MiB", report.guest_rss_max_mib);
    println!("host memory: {} MiB", report.host_memory_mib);
    println!("reserved memory: {} MiB", report.reserve_mib);
    println!("usable memory: {} MiB", report.usable_memory_mib);
    println!(
        "projected density by host RSS: {:?} microVMs",
        report.projected_density_by_host_rss
    );
    if let Some(speedup) = report.boot_speedup_vs_baseline {
        println!("boot speedup vs baseline: {:.2}x", speedup);
    }
    if let Some(reduction) = report.host_rss_reduction_vs_baseline {
        println!("host RSS reduction vs baseline: {:.2}x", reduction);
    }
    Ok(())
}

#[derive(Debug, Serialize)]
struct MathReport {
    app: String,
    benchmark: String,
    iterations: usize,
    boot_median_ms: Option<u128>,
    boot_p90_ms: Option<u128>,
    boot_p99_ms: Option<u128>,
    host_rss_max_kib: Option<u64>,
    host_rss_max_mib: Option<f64>,
    guest_rss_max_kib: Option<u64>,
    guest_rss_max_mib: Option<f64>,
    host_memory_mib: u32,
    reserve_mib: u32,
    usable_memory_mib: u32,
    projected_density_by_host_rss: Option<u64>,
    baseline_host_rss_mib: Option<u32>,
    baseline_boot_ms: Option<u128>,
    host_rss_reduction_vs_baseline: Option<f64>,
    boot_speedup_vs_baseline: Option<f64>,
}

impl MathReport {
    fn from_args(
        manifest: &ArtifactManifest,
        benchmark: &crate::artifact::BenchmarkReport,
        args: &MathArgs,
    ) -> Result<Self> {
        if args.reserve >= args.host_memory {
            bail!("--reserve must be smaller than --host-memory");
        }
        let usable_memory_mib = args.host_memory - args.reserve;
        let host_rss_max_kib = benchmark.summary.host_rss_kib_max;
        let host_rss_max_mib = host_rss_max_kib.map(kib_to_mib);
        let guest_rss_max_kib = benchmark.summary.guest_rss_kib_max;
        let guest_rss_max_mib = guest_rss_max_kib.map(kib_to_mib);
        let projected_density_by_host_rss = host_rss_max_mib
            .filter(|rss| *rss > 0.0)
            .map(|rss| (usable_memory_mib as f64 / rss).floor() as u64);
        let host_rss_reduction_vs_baseline = args.baseline_host_rss.and_then(|baseline| {
            host_rss_max_mib
                .filter(|rss| *rss > 0.0)
                .map(|rss| baseline as f64 / rss)
        });
        let boot_speedup_vs_baseline = args.baseline_boot_ms.and_then(|baseline| {
            benchmark
                .summary
                .boot_to_listen_ms_median
                .filter(|median| *median > 0)
                .map(|median| baseline as f64 / median as f64)
        });

        Ok(Self {
            app: manifest.app.name.clone(),
            benchmark: benchmark.name.clone(),
            iterations: benchmark.iterations.len(),
            boot_median_ms: benchmark.summary.boot_to_listen_ms_median,
            boot_p90_ms: benchmark.summary.boot_to_listen_ms_p90,
            boot_p99_ms: benchmark.summary.boot_to_listen_ms_p99,
            host_rss_max_kib,
            host_rss_max_mib,
            guest_rss_max_kib,
            guest_rss_max_mib,
            host_memory_mib: args.host_memory,
            reserve_mib: args.reserve,
            usable_memory_mib,
            projected_density_by_host_rss,
            baseline_host_rss_mib: args.baseline_host_rss,
            baseline_boot_ms: args.baseline_boot_ms,
            host_rss_reduction_vs_baseline,
            boot_speedup_vs_baseline,
        })
    }
}

fn kib_to_mib(kib: u64) -> f64 {
    (kib as f64 * 100.0 / 1024.0).round() / 100.0
}

fn analyze(args: AnalyzeArgs) -> Result<()> {
    let analysis = analyze_jar(&args.jar)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&analysis)?);
    } else {
        println!("jar: {}", args.jar.display());
        println!("entries: {}", analysis.entry_count);
        println!(
            "main class: {}",
            analysis.main_class.as_deref().unwrap_or("<none>")
        );
        println!(
            "native-image metadata: {}",
            analysis.native_image_metadata_present
        );
        println!("frameworks:");
        for framework in analysis.frameworks {
            println!(
                "  {} supported_in_native={} confidence={}",
                framework.kind, framework.supported_in_native, framework.confidence
            );
            if let Some(recommendation) = framework.recommendation {
                println!("  recommendation: {recommendation}");
            }
        }
    }
    Ok(())
}

fn doctor(args: DoctorArgs) -> Result<()> {
    let config = ToolchainConfig {
        java: args.java_exec,
        native_image: args.native_image,
        firecracker: args.firecracker,
    };
    let versions = toolchain::probe_versions(&config);
    println!("host: {}/{}", std::env::consts::OS, std::env::consts::ARCH);
    println!("java: {}", format_tool(versions.java.as_ref()));
    println!(
        "native-image: {}",
        format_tool(versions.native_image.as_ref())
    );
    println!(
        "firecracker: {}",
        format_tool(versions.firecracker.as_ref())
    );
    println!(
        "ext4 tooling: {}",
        toolchain::validate_rootfs_tooling(false).unwrap_or_else(|err| err.to_string())
    );

    match toolchain::ensure_linux_kvm() {
        Ok(()) => println!("kvm: available"),
        Err(err) => {
            println!("kvm: unavailable ({err})");
            if args.strict {
                return Err(err);
            }
        }
    }
    Ok(())
}

fn build_native_binary(
    args: &BuildArgs,
    detected_main: &Option<String>,
    output: &Path,
) -> Result<()> {
    match args.backend {
        BuildBackend::Graal => build_graal_native_binary(args, detected_main, output)?,
        BuildBackend::FvmAot => crate::fvm_aot::compile_jar(&crate::fvm_aot::CompileSpec {
            jar_path: args.jar.clone(),
            main_class: args.main_class.clone().or_else(|| detected_main.clone()),
            output_path: output.to_path_buf(),
            cc: args.cc.clone(),
            dry_run: args.dry_run,
        })?,
    }
    Ok(())
}

fn build_graal_native_binary(
    args: &BuildArgs,
    detected_main: &Option<String>,
    output: &Path,
) -> Result<()> {
    if args.dry_run {
        std::fs::write(output, b"dry-run native binary placeholder\n")?;
        make_executable(output)?;
        return Ok(());
    }

    let native_args = resolved_native_image_args(args, detected_main);
    let os_args = native_args
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
    run_streaming(&args.native_image, os_args, None)?;
    make_executable(output)?;
    Ok(())
}

fn resolved_native_image_args(args: &BuildArgs, detected_main: &Option<String>) -> Vec<String> {
    let _ = detected_main;
    let mut native_args = vec![
        "--no-fallback".to_string(),
        "-H:+ReportExceptionStackTraces".to_string(),
    ];
    for dir in &args.native_config_dir {
        native_args.push(format!("-H:ConfigurationFileDirectories={}", dir.display()));
    }
    native_args.extend(args.native_image_arg.clone());

    let main_class = args.main_class.as_ref();
    if let Some(main_class) = main_class {
        native_args.push("-cp".to_string());
        native_args.push(args.jar.to_string_lossy().to_string());
        native_args.push(main_class.clone());
        native_args.push(args_output_binary_path(args).to_string_lossy().to_string());
    } else {
        native_args.push("-jar".to_string());
        native_args.push(args.jar.to_string_lossy().to_string());
        native_args.push(args_output_binary_path(args).to_string_lossy().to_string());
    }
    native_args
}

fn args_output_binary_path(args: &BuildArgs) -> PathBuf {
    artifact_dir_from_output(args.output.clone(), &args.jar).join(APP_BINARY_FILE)
}

fn prepare_artifact_dir(path: &Path, force: bool) -> Result<()> {
    if path.exists() {
        if !force {
            bail!(
                "artifact {} already exists; pass --force to replace it",
                path.display()
            );
        }
        if path.parent().is_none() || path == Path::new("/") {
            bail!("refusing to remove unsafe artifact path {}", path.display());
        }
        std::fs::remove_dir_all(path)
            .with_context(|| format!("failed to remove existing artifact {}", path.display()))?;
    }
    std::fs::create_dir_all(path)
        .with_context(|| format!("failed to create artifact directory {}", path.display()))?;
    Ok(())
}

fn copy_kernel(kernel: Option<&Path>, output: &Path, dry_run: bool) -> Result<()> {
    let kernel = kernel
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("FVM_KERNEL").map(PathBuf::from));
    let Some(kernel) = kernel else {
        if dry_run {
            std::fs::write(output, b"dry-run kernel placeholder\n")?;
            return Ok(());
        }
        bail!("missing kernel image; pass --kernel /path/to/vmlinux or set FVM_KERNEL");
    };
    if !kernel.is_file() {
        bail!("kernel image {} does not exist", kernel.display());
    }
    std::fs::copy(&kernel, output).with_context(|| {
        format!(
            "failed to copy kernel {} to {}",
            kernel.display(),
            output.display()
        )
    })?;
    Ok(())
}

fn parse_ports(raw_ports: &[String]) -> Result<Vec<PortMapping>> {
    raw_ports
        .iter()
        .map(|raw| parse_port_mapping(raw))
        .collect()
}

fn parse_memory_mib(raw: &str) -> std::result::Result<u32, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("memory value cannot be empty".to_string());
    }
    let lower = trimmed.to_ascii_lowercase();
    let (number, multiplier) = if let Some(number) = lower.strip_suffix("mib") {
        (number, 1_u32)
    } else if let Some(number) = lower.strip_suffix('m') {
        (number, 1_u32)
    } else if let Some(number) = lower.strip_suffix("gib") {
        (number, 1024_u32)
    } else if let Some(number) = lower.strip_suffix('g') {
        (number, 1024_u32)
    } else {
        (lower.as_str(), 1_u32)
    };
    let value: u32 = number
        .trim()
        .parse()
        .map_err(|_| format!("invalid memory value `{raw}`"))?;
    value
        .checked_mul(multiplier)
        .ok_or_else(|| format!("memory value `{raw}` is too large"))
}

fn readiness_override(
    existing: Option<ReadinessConfig>,
    override_path: Option<&str>,
    timeout_seconds: u64,
) -> Option<ReadinessConfig> {
    override_path
        .map(|http_path| ReadinessConfig {
            http_path: http_path.to_string(),
            timeout_seconds,
        })
        .or(existing)
}

fn resolve_snapshot_load(
    artifact_dir: &Path,
    manifest: &ArtifactManifest,
    snapshot_name: Option<&str>,
) -> Result<Option<SnapshotLoadSpec>> {
    let Some(snapshot_name) = snapshot_name else {
        return Ok(None);
    };
    let snapshot = manifest
        .snapshots
        .iter()
        .find(|snapshot| snapshot.name == snapshot_name)
        .with_context(|| format!("snapshot `{snapshot_name}` not found in artifact"))?;
    Ok(Some(SnapshotLoadSpec {
        mem_path: artifact_dir.join(&snapshot.mem_file.path),
        vmstate_path: artifact_dir.join(&snapshot.vmstate_file.path),
    }))
}

fn record_benchmark(
    artifact_dir: &Path,
    manifest: &mut ArtifactManifest,
    name: &str,
    iterations: Vec<crate::artifact::BenchmarkIteration>,
    firecracker_version: Option<crate::artifact::ToolVersion>,
) -> Result<()> {
    let host = toolchain::host_benchmark_info(firecracker_version);
    manifest
        .benchmarks
        .push(benchmark::build_report(name, iterations, host));
    manifest.save(artifact_dir)
}

fn println_metrics(metrics: &firecracker::RunMetrics) {
    println!("boot_to_listen_ms: {:?}", metrics.boot_to_listen_ms);
    println!("host_rss_kib: {:?}", metrics.host_rss_kib);
    println!("guest_rss_kib: {:?}", metrics.guest_rss_kib);
}

fn default_app_name(jar: &Path) -> String {
    jar.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("app")
        .to_string()
}

fn build_diagnostics(
    frameworks: &[crate::artifact::FrameworkDetection],
    guest_rss_unavailable: bool,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for framework in frameworks {
        if let Some(recommendation) = &framework.recommendation {
            diagnostics.push(Diagnostic {
                level: DiagnosticLevel::Warning,
                message: recommendation.clone(),
            });
        }
    }
    if guest_rss_unavailable {
        diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Warning,
            message: "guest RSS will not be available for this artifact".to_string(),
        });
    }
    diagnostics
}

fn parse_cgroups(raw: &[String]) -> Result<Vec<CgroupSetting>> {
    raw.iter()
        .map(|entry| {
            let Some((key, value)) = entry.split_once('=') else {
                bail!("cgroup setting must be KEY=VALUE, got `{entry}`");
            };
            Ok(CgroupSetting {
                key: key.to_string(),
                value: value.to_string(),
            })
        })
        .collect()
}

fn parse_secrets(raw: &[String]) -> Result<Vec<SecretMount>> {
    raw.iter()
        .map(|entry| {
            let Some((name, rest)) = entry.split_once('=') else {
                bail!("secret must be NAME=SOURCE[:GUEST_PATH], got `{entry}`");
            };
            let (source, guest_path) = rest.split_once(':').unwrap_or((rest, "/run/secrets"));
            Ok(SecretMount {
                name: name.to_string(),
                source_path: source.to_string(),
                guest_path: if guest_path == "/run/secrets" {
                    format!("/run/secrets/{name}")
                } else {
                    guest_path.to_string()
                },
            })
        })
        .collect()
}

fn format_tool(tool: Option<&crate::artifact::ToolVersion>) -> String {
    tool.map(|tool| tool.version_output.replace('\n', " "))
        .unwrap_or_else(|| "missing".to_string())
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
    fn parses_secret_with_default_guest_path() {
        let secrets = parse_secrets(&["TOKEN=/tmp/token".to_string()]).unwrap();
        assert_eq!(secrets[0].name, "TOKEN");
        assert_eq!(secrets[0].guest_path, "/run/secrets/TOKEN");
    }

    #[test]
    fn parses_cgroup_setting() {
        let cgroups = parse_cgroups(&["memory.max=64M".to_string()]).unwrap();
        assert_eq!(cgroups[0].key, "memory.max");
        assert_eq!(cgroups[0].value, "64M");
    }

    #[test]
    fn parses_memory_units() {
        assert_eq!(parse_memory_mib("64").unwrap(), 64);
        assert_eq!(parse_memory_mib("64M").unwrap(), 64);
        assert_eq!(parse_memory_mib("64MiB").unwrap(), 64);
        assert_eq!(parse_memory_mib("1G").unwrap(), 1024);
    }

    #[test]
    fn converts_kib_to_mib() {
        assert_eq!(kib_to_mib(1024), 1.0);
        assert_eq!(kib_to_mib(1536), 1.5);
    }
}
