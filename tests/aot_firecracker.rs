use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fvm() -> &'static str {
    env!("CARGO_BIN_EXE_fvm")
}

#[test]
#[ignore]
fn aot_firecracker_smoke_requires_explicit_linux_kvm_gate() {
    let gate = match FirecrackerGate::check() {
        GateDecision::Skip(reason) => {
            println!("skipping fvm-aot Firecracker smoke: {reason}");
            return;
        }
        GateDecision::Run(gate) => gate,
    };

    let temp = tempfile::tempdir().unwrap();
    let classes_dir = temp.path().join("classes");
    let jar = temp.path().join("aot-http.jar");
    let artifact = temp.path().join("aot-http.fvm");

    compile_aot_http_classes(&classes_dir);
    package_aot_http_jar(&classes_dir, &jar);

    run_ok(
        Command::new(fvm())
            .arg("build")
            .arg(&jar)
            .arg("--backend")
            .arg("fvm-aot")
            .arg("--force")
            .arg("--output")
            .arg(&artifact)
            .arg("--kernel")
            .arg(&gate.kernel)
            .arg("--port")
            .arg("18080:9090")
            .arg("--readiness-http")
            .arg("/health"),
    );

    run_ok(
        Command::new(fvm())
            .arg("run")
            .arg(&artifact)
            .arg("--once")
            .arg("--wait-http")
            .arg("/health"),
    );

    let inspect = run_ok(
        Command::new(fvm())
            .arg("inspect")
            .arg(&artifact)
            .arg("--verify"),
    );
    assert_stdout_contains(&inspect, "verified=true");
}

struct FirecrackerGate {
    kernel: PathBuf,
}

enum GateDecision {
    Skip(String),
    Run(FirecrackerGate),
}

impl FirecrackerGate {
    fn check() -> GateDecision {
        if !cfg!(target_os = "linux") {
            return GateDecision::Skip(
                "Firecracker requires Linux/KVM; macOS is build/test/dry-run only".to_string(),
            );
        }

        let kvm = Path::new("/dev/kvm");
        if !kvm.exists() {
            return GateDecision::Skip(
                "Firecracker requires Linux/KVM; missing /dev/kvm".to_string(),
            );
        }

        let tun = Path::new("/dev/net/tun");
        if !tun.exists() {
            return GateDecision::Skip(
                "Firecracker requires Linux/KVM networking; missing /dev/net/tun".to_string(),
            );
        }

        let Some(kernel) = std::env::var_os("FVM_KERNEL").map(PathBuf::from) else {
            return GateDecision::Skip(
                "Firecracker requires Linux/KVM and FVM_KERNEL=/path/to/vmlinux".to_string(),
            );
        };
        if !kernel.is_file() {
            return GateDecision::Skip(format!(
                "Firecracker requires Linux/KVM and a readable FVM_KERNEL; {} is not a file",
                kernel.display()
            ));
        }

        for executable in ["firecracker", "javac", "jar", "cc", "ip"] {
            if !command_available(executable) {
                return GateDecision::Skip(format!(
                    "Firecracker requires Linux/KVM prerequisite `{executable}` on PATH"
                ));
            }
        }

        if !command_available("mkfs.ext4") && !command_available("mke2fs") {
            return GateDecision::Skip(
                "Firecracker requires Linux/KVM prerequisite `mkfs.ext4` or `mke2fs` on PATH"
                    .to_string(),
            );
        }

        GateDecision::Run(FirecrackerGate { kernel })
    }
}

fn compile_aot_http_classes(classes_dir: &Path) {
    std::fs::create_dir_all(classes_dir).unwrap();
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/aot-http/src");
    run_ok(
        Command::new("javac")
            .arg("--release")
            .arg("17")
            .arg("-d")
            .arg(classes_dir)
            .args([
                source_root.join("AotHttp.java"),
                source_root.join("AotConfig.java"),
                source_root.join("AotHandler.java"),
                source_root.join("AotResponder.java"),
                source_root.join("fvm/runtime/Http.java"),
            ]),
    );
}

fn package_aot_http_jar(classes_dir: &Path, jar: &Path) {
    run_ok(
        Command::new("jar")
            .arg("--create")
            .arg("--file")
            .arg(jar)
            .arg("--main-class")
            .arg("AotHttp")
            .arg("-C")
            .arg(classes_dir)
            .arg("."),
    );
}

fn run_ok(command: &mut Command) -> Output {
    let output = command.output().unwrap();
    if !output.status.success() {
        panic!(
            "command failed: {:?}\nstdout:\n{}\nstderr:\n{}",
            command,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    output
}

fn assert_stdout_contains(output: &Output, expected: &str) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(expected),
        "stdout did not contain `{expected}`:\n{stdout}"
    );
}

fn command_available(name: &str) -> bool {
    Command::new(name)
        .arg(OsStr::new("--version"))
        .output()
        .is_ok()
}
