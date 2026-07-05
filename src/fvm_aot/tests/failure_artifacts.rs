use crate::fvm_aot::test_support::AotFixture;
use std::path::{Path, PathBuf};
use std::process::Command;

const CHILD_MODE_ENV: &str = "FVM_AOT_FAILURE_ARTIFACT_CHILD";
const KEEP_MODE: &str = "keep";
const CLEAN_MODE: &str = "clean";
const RETAINED_PREFIX: &str = "retained_dir=";
const ROOT_PREFIX: &str = "artifact_root=";

#[test]
fn preserves_failed_artifacts_when_env_var_is_set() {
    let output = run_child(KEEP_MODE, true);
    assert!(
        output.status.success(),
        "child retention test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    let retained = extract_path(&stdout, RETAINED_PREFIX);
    assert!(
        retained.exists(),
        "retained directory missing: {retained:?}"
    );
    assert_representative_artifacts_exist(&retained);

    std::fs::remove_dir_all(&retained).unwrap();
}

#[test]
fn cleans_failed_artifacts_when_env_var_is_unset() {
    let output = run_child(CLEAN_MODE, false);
    assert!(
        output.status.success(),
        "child cleanup test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("retained_dir=<none>"),
        "default cleanup reported retained artifacts:\n{stdout}"
    );
    let root = extract_path(&stdout, ROOT_PREFIX);
    assert!(
        !root.exists(),
        "default cleanup left artifact root: {root:?}"
    );
}

#[test]
fn child_failure_artifact_mode() {
    let Some(mode) = std::env::var(CHILD_MODE_ENV).ok() else {
        return;
    };

    let mut fixture = AotFixture::new().unwrap();
    let root = fixture.artifact_path("");
    write_representative_artifacts(&root);
    let retained = fixture.preserve_failed_artifacts("controlled failure artifact test");

    match mode.as_str() {
        KEEP_MODE => {
            let retained_dir = retained.retained_dir().unwrap();
            assert_eq!(retained_dir, root.as_path());
            assert_representative_artifacts_exist(retained_dir);
            println!("{RETAINED_PREFIX}{}", retained_dir.display());
        }
        CLEAN_MODE => {
            assert!(retained.retained_dir().is_none());
            println!("retained_dir=<none>");
        }
        other => panic!("unknown failure artifact child mode: {other}"),
    }

    drop(retained);
    drop(fixture);
    println!("{ROOT_PREFIX}{}", root.display());
}

fn run_child(mode: &str, keep_failed_aot: bool) -> std::process::Output {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .arg("--exact")
        .arg("fvm_aot::tests::failure_artifacts::child_failure_artifact_mode")
        .arg("--nocapture")
        .env(CHILD_MODE_ENV, mode);
    if keep_failed_aot {
        command.env("FVM_KEEP_FAILED_AOT", "1");
    } else {
        command.env_remove("FVM_KEEP_FAILED_AOT");
    }
    command.output().unwrap()
}

fn write_representative_artifacts(root: &Path) {
    for artifact in [
        (
            "src/AotFailure.java",
            b"final class AotFailure {}".as_slice(),
        ),
        ("classes/AotFailure.class", b"class-bytes".as_slice()),
        ("app.jar", b"jar-bytes".as_slice()),
        ("native/app", b"native-bytes".as_slice()),
        ("logs/compiler.log", b"compiler diagnostics".as_slice()),
    ] {
        write_artifact(root, artifact.0, artifact.1);
    }
}

fn write_artifact(root: &Path, relative_path: &str, contents: &[u8]) {
    let path = root.join(relative_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn assert_representative_artifacts_exist(root: &Path) {
    for relative_path in [
        "src/AotFailure.java",
        "classes/AotFailure.class",
        "app.jar",
        "native/app",
        "logs/compiler.log",
    ] {
        let path = root.join(relative_path);
        assert!(
            path.exists(),
            "expected artifact missing: {}",
            path.display()
        );
    }
}

fn extract_path(output: &str, prefix: &str) -> PathBuf {
    output
        .lines()
        .find_map(|line| line.strip_prefix(prefix).map(PathBuf::from))
        .unwrap_or_else(|| panic!("missing `{prefix}` line in output:\n{output}"))
}
