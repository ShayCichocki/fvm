#![allow(dead_code)]

use super::runtime_stub::{PRINT_INT_SYMBOL, emit_runtime_stub_c, is_c_identifier};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

/// How the compiled entry's result is delivered to the process. Until the real
/// `main(String[])` path (P4.3) lands, the int-method harness treats the entry's
/// return value as the program's output and *prints* it — matching what
/// `System.out.println(entry())` would emit — instead of squeezing it through
/// the 8-bit process exit code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum EntryReturn {
    Int,
    Void,
}

pub(super) struct LinkSpec<'a> {
    pub(super) cc: &'a str,
    pub(super) object_bytes: &'a [u8],
    pub(super) entry_symbol: &'a str,
    pub(super) entry_return: EntryReturn,
    /// Class-initializer symbols (`<clinit>`), in the order they must run before
    /// the entry. Each is a `void(void)` function the generated `main` calls up
    /// front so static fields hold their initialized values (P2.4).
    pub(super) clinit_symbols: &'a [String],
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
    if !is_c_identifier(spec.entry_symbol) {
        bail!(
            "fvm-aot refuses to splice entry symbol `{}` into generated C: not a valid C identifier",
            spec.entry_symbol
        );
    }
    for clinit in spec.clinit_symbols {
        if !is_c_identifier(clinit) {
            bail!(
                "fvm-aot refuses to splice class-initializer symbol `{clinit}` into generated C: not a valid C identifier"
            );
        }
    }

    let temp = tempfile::tempdir().context("failed to create fvm-aot link directory")?;
    let object_path = temp.path().join("fvm_aot_app.o");
    let stub_source_path = temp.path().join("fvm_runtime_stub.c");
    let stub_object_path = temp.path().join("fvm_runtime_stub.o");

    std::fs::write(&object_path, spec.object_bytes)
        .with_context(|| format!("failed to write Cranelift object {}", object_path.display()))?;
    std::fs::write(
        &stub_source_path,
        runtime_stub_source(spec.entry_symbol, spec.entry_return, spec.clinit_symbols),
    )
    .with_context(|| {
        format!(
            "failed to write runtime stub source {}",
            stub_source_path.display()
        )
    })?;

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

fn runtime_stub_source(
    entry_symbol: &str,
    entry_return: EntryReturn,
    clinit_symbols: &[String],
) -> String {
    let mut source = emit_runtime_stub_c();
    source.push('\n');
    for clinit in clinit_symbols {
        source.push_str("extern void ");
        source.push_str(clinit);
        source.push_str("(void);\n");
    }
    // The class initializers run first (in dependency order) so static fields
    // hold their `<clinit>`-installed values before the entry executes.
    let mut run_clinits = String::new();
    for clinit in clinit_symbols {
        run_clinits.push_str("  ");
        run_clinits.push_str(clinit);
        run_clinits.push_str("();\n");
    }
    match entry_return {
        EntryReturn::Int => {
            source.push_str("extern int ");
            source.push_str(entry_symbol);
            source.push_str("(void);\nint main(void) {\n");
            source.push_str(&run_clinits);
            source.push_str("  ");
            source.push_str(PRINT_INT_SYMBOL);
            source.push('(');
            source.push_str(entry_symbol);
            source.push_str("());\n  return 0;\n}\n");
        }
        EntryReturn::Void => {
            source.push_str("extern void ");
            source.push_str(entry_symbol);
            source.push_str("(void);\nint main(void) {\n");
            source.push_str(&run_clinits);
            source.push_str("  ");
            source.push_str(entry_symbol);
            source.push_str("();\n  return 0;\n}\n");
        }
    }
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
