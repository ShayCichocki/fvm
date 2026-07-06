use crate::fvm_aot::runtime_stub::{RuntimeHelperDeclaration, emit_runtime_stub_c};
use crate::fvm_aot::test_support::command_available;
use std::process::Command;

#[test]
fn runtime_stub_compiles_when_cc_available() -> anyhow::Result<()> {
    if !command_available("cc") {
        println!("skipping runtime stub C compilation test because required tool is missing: cc");
        return Ok(());
    }

    let temp = tempfile::tempdir()?;
    let source_path = temp.path().join("fvm_runtime_stub.c");
    let object_path = temp.path().join("fvm_runtime_stub.o");

    std::fs::write(&source_path, emit_runtime_stub_c())?;
    let output = Command::new("cc")
        .arg("-c")
        .arg(&source_path)
        .arg("-o")
        .arg(&object_path)
        .output()?;

    assert!(
        output.status.success(),
        "cc failed compiling runtime stub: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(object_path.exists());
    Ok(())
}

#[test]
fn malformed_runtime_helper_declaration_returns_actionable_error() {
    let message = RuntimeHelperDeclaration::new("fvm-rt-bad-name", "void", &[])
        .unwrap_err()
        .to_string();

    assert!(message.contains("runtime helper declaration"));
    assert!(message.contains("fvm-rt-bad-name"));
    assert!(message.contains("C identifier"));
}
