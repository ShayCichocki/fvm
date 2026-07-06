#![allow(dead_code)]

use super::runtime_stub::emit_runtime_stub_c;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) struct LinkSpec<'a> {
    pub(super) cc: &'a str,
    pub(super) object_bytes: &'a [u8],
    pub(super) entry_symbol: &'a str,
    pub(super) output_path: &'a Path,
}

#[derive(Debug)]
pub(super) struct LinkedExecutable {
    path: PathBuf,
}

impl LinkedExecutable {
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

pub(super) fn link_cranelift_object_with_runtime_stub(
    spec: &LinkSpec<'_>,
) -> Result<LinkedExecutable> {
    let temp = tempfile::tempdir().context("failed to create fvm-aot link directory")?;
    let object_path = temp.path().join("fvm_aot_app.o");
    let stub_source_path = temp.path().join("fvm_runtime_stub.c");
    let stub_object_path = temp.path().join("fvm_runtime_stub.o");

    std::fs::write(&object_path, spec.object_bytes)
        .with_context(|| format!("failed to write Cranelift object {}", object_path.display()))?;
    std::fs::write(&stub_source_path, runtime_stub_source(spec.entry_symbol)).with_context(
        || {
            format!(
                "failed to write runtime stub source {}",
                stub_source_path.display()
            )
        },
    )?;

    run_cc(
        spec.cc,
        &[
            "-c".as_ref(),
            stub_source_path.as_ref(),
            "-o".as_ref(),
            stub_object_path.as_ref(),
        ],
        "compile runtime stub",
    )?;
    run_cc(
        spec.cc,
        &[
            object_path.as_ref(),
            stub_object_path.as_ref(),
            "-o".as_ref(),
            spec.output_path,
        ],
        "link executable",
    )?;

    Ok(LinkedExecutable {
        path: spec.output_path.to_path_buf(),
    })
}

fn runtime_stub_source(entry_symbol: &str) -> String {
    let mut source = emit_runtime_stub_c();
    source.push_str("\nextern int ");
    source.push_str(entry_symbol);
    source.push_str("(void);\nint main(void) {\n  return ");
    source.push_str(entry_symbol);
    source.push_str("();\n}\n");
    source
}

fn run_cc(cc: &str, args: &[&Path], action: &str) -> Result<()> {
    let output = Command::new(cc)
        .args(args)
        .output()
        .with_context(|| format!("failed to execute fvm-aot linker `{cc}` for {action}"))?;
    if output.status.success() {
        return Ok(());
    }

    bail!(
        "fvm-aot linker `{cc}` failed to {action}: status={} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
