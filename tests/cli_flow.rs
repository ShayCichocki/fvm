use serde_json::Value;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fvm() -> &'static str {
    env!("CARGO_BIN_EXE_fvm")
}

#[test]
fn dry_run_artifact_lifecycle_works() {
    let temp = tempfile::tempdir().unwrap();
    let jar = temp.path().join("app.jar");
    let artifact = temp.path().join("app.fvm");
    write_test_jar(&jar, &["App.class"], Some("App"));

    let analyze = run_ok(Command::new(fvm()).arg("analyze").arg(&jar));
    assert_stdout_contains(&analyze, "plain-java");

    run_ok(
        Command::new(fvm())
            .arg("build")
            .arg(&jar)
            .arg("--dry-run")
            .arg("--force")
            .arg("--output")
            .arg(&artifact)
            .arg("--port")
            .arg("18080:8080"),
    );

    run_ok(
        Command::new(fvm())
            .arg("run")
            .arg(&artifact)
            .arg("--dry-run")
            .arg("--once"),
    );

    run_ok(
        Command::new(fvm())
            .arg("snapshot")
            .arg(&artifact)
            .arg("--dry-run")
            .arg("--verify-restore"),
    );

    let inspect = run_ok(
        Command::new(fvm())
            .arg("inspect")
            .arg(&artifact)
            .arg("--verify"),
    );
    assert_stdout_contains(&inspect, "initialized verified=true");

    let inspect_json = run_ok(
        Command::new(fvm())
            .arg("inspect")
            .arg(&artifact)
            .arg("--json"),
    );
    let manifest: Value = serde_json::from_slice(&inspect_json.stdout).unwrap();
    assert_eq!(manifest["java"]["target_version"], 25);
    assert_eq!(manifest["build"]["mode"], "native");
    assert_eq!(manifest["build"]["backend"], "graal");
    assert_eq!(manifest["runtime"]["ports"][0]["host"], 18080);

    let math = run_ok(
        Command::new(fvm())
            .arg("math")
            .arg(&artifact)
            .arg("--host-memory")
            .arg("32G")
            .arg("--reserve")
            .arg("2G")
            .arg("--baseline-host-rss")
            .arg("256M")
            .arg("--baseline-boot-ms")
            .arg("5000"),
    );
    assert_stdout_contains(&math, "usable memory: 30720 MiB");
}

#[test]
fn unsupported_spring_shape_fails_native_build() {
    let temp = tempfile::tempdir().unwrap();
    let jar = temp.path().join("spring.jar");
    let artifact = temp.path().join("spring.fvm");
    write_test_jar(&jar, &["BOOT-INF/classes/com/example/App.class"], None);

    let output = Command::new(fvm())
        .arg("build")
        .arg(&jar)
        .arg("--dry-run")
        .arg("--output")
        .arg(&artifact)
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert_stderr_contains(&output, "not safely supported in native mode yet");
}

#[test]
fn doctor_non_strict_is_laptop_safe() {
    let output = run_ok(Command::new(fvm()).arg("doctor"));
    assert_stdout_contains(&output, "host:");
}

#[test]
fn fvm_aot_dry_run_builds_supported_println_subset() {
    if skip_missing_javac() {
        return;
    }

    let temp = tempfile::tempdir().unwrap();
    let jar = temp.path().join("aot.jar");
    let artifact = temp.path().join("aot.fvm");
    let class_file = compile_java_class(
        temp.path(),
        "AotApp",
        r#"public final class AotApp {
    public static void main(String[] args) {
        System.out.println("hello from fvm-aot");
    }
}
"#,
    );
    write_test_jar_from_class(&jar, "AotApp", &class_file);

    run_ok(
        Command::new(fvm())
            .arg("build")
            .arg(&jar)
            .arg("--backend")
            .arg("fvm-aot")
            .arg("--dry-run")
            .arg("--force")
            .arg("--output")
            .arg(&artifact),
    );

    let inspect_json = run_ok(
        Command::new(fvm())
            .arg("inspect")
            .arg(&artifact)
            .arg("--json"),
    );
    let manifest: Value = serde_json::from_slice(&inspect_json.stdout).unwrap();
    assert_eq!(manifest["build"]["backend"], "fvm-aot");
    assert_eq!(manifest["java"]["native_image"], Value::Null);
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

fn assert_stderr_contains(output: &Output, expected: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(expected),
        "stderr did not contain `{expected}`:\n{stderr}"
    );
}

fn command_available(name: &str) -> bool {
    Command::new(name).arg("--version").output().is_ok()
}

/// Returns `true` if the test should skip because `javac` is missing. When
/// `FVM_AOT_REQUIRE_TOOLCHAIN=1` (CI), a missing toolchain is a hard failure
/// instead of a silent skip (PUNCHLIST P0.4).
fn skip_missing_javac() -> bool {
    if command_available("javac") {
        return false;
    }
    if std::env::var_os("FVM_AOT_REQUIRE_TOOLCHAIN").is_some_and(|value| value == "1") {
        panic!("FVM_AOT_REQUIRE_TOOLCHAIN=1 but javac is missing");
    }
    println!("skipping fvm-aot CLI flow test because javac is missing");
    true
}

/// Compiles a single Java class, panicking on failure. Callers must gate on
/// [`skip_missing_javac`] first; once javac is known present, a compile failure
/// is a real bug in the fixture, not a reason to silently pass the test
/// (PUNCHLIST P0.4).
fn compile_java_class(temp: &Path, class_name: &str, source: &str) -> PathBuf {
    let src_dir = temp.join("src");
    let classes_dir = temp.join("classes");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::create_dir_all(&classes_dir).unwrap();
    let source_path = src_dir.join(format!("{class_name}.java"));
    std::fs::write(&source_path, source).unwrap();
    let status = Command::new("javac")
        .arg("--release")
        .arg("17")
        .arg("-d")
        .arg(&classes_dir)
        .arg(&source_path)
        .status()
        .expect("failed to execute javac");
    assert!(
        status.success(),
        "javac failed to compile fixture {class_name}"
    );
    classes_dir.join(format!("{class_name}.class"))
}

fn write_test_jar(path: &Path, entries: &[&str], main_class: Option<&str>) {
    let file = File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::FileOptions::<()>::default();
    let manifest = match main_class {
        Some(main_class) => format!("Manifest-Version: 1.0\nMain-Class: {main_class}\n"),
        None => "Manifest-Version: 1.0\n".to_string(),
    };

    zip.start_file("META-INF/MANIFEST.MF", options).unwrap();
    zip.write_all(manifest.as_bytes()).unwrap();
    for entry in entries {
        zip.start_file(entry, options).unwrap();
        zip.write_all(b"test").unwrap();
    }
    zip.finish().unwrap();
}

fn write_test_jar_from_class(path: &Path, main_class: &str, class_file: &Path) {
    let file = File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::FileOptions::<()>::default();
    let manifest = format!("Manifest-Version: 1.0\nMain-Class: {main_class}\n");

    zip.start_file("META-INF/MANIFEST.MF", options).unwrap();
    zip.write_all(manifest.as_bytes()).unwrap();
    zip.start_file(format!("{main_class}.class"), options)
        .unwrap();
    zip.write_all(&std::fs::read(class_file).unwrap()).unwrap();
    zip.finish().unwrap();
}
