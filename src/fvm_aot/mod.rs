use anyhow::{Context, Result, bail};
mod classfile;
mod codegen;
#[cfg(test)]
mod compiler;
mod diagnostics;
mod emitter;
mod evaluator;
mod ir;
mod ir_verify;
mod link;
#[cfg(test)]
mod lower;
mod object_model;
mod reachability;
mod runtime_stub;
#[cfg(test)]
mod test_support;
mod types;
use classfile::ClassFile;
use emitter::emit_c;
use evaluator::compile_main;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use zip::ZipArchive;

pub struct CompileSpec {
    pub jar_path: PathBuf,
    pub main_class: Option<String>,
    pub output_path: PathBuf,
    pub cc: String,
    pub dry_run: bool,
}

pub fn compile_jar(spec: &CompileSpec) -> Result<()> {
    let main_class = spec
        .main_class
        .as_deref()
        .context("fvm-aot requires a Main-Class manifest entry or --main-class")?;
    let world = read_class_world(&spec.jar_path)?;
    let program = compile_main(&world, &main_class.replace('.', "/"))?;

    if spec.dry_run {
        std::fs::write(
            &spec.output_path,
            format!(
                "dry-run fvm-aot native binary placeholder\nmain_class={}\nprintln_count={}\nhttp_server={}\n",
                main_class,
                program.printlns.len(),
                program.http_server.is_some()
            ),
        )?;
        make_executable(&spec.output_path)?;
        return Ok(());
    }

    let temp = tempfile::tempdir().context("failed to create fvm-aot build directory")?;
    let c_path = temp.path().join("app.c");
    std::fs::write(&c_path, emit_c(&program))
        .with_context(|| format!("failed to write generated C source {}", c_path.display()))?;

    let status = Command::new(&spec.cc)
        .arg("-Os")
        .arg(&c_path)
        .arg("-o")
        .arg(&spec.output_path)
        .status()
        .with_context(|| format!("failed to execute fvm-aot C compiler `{}`", spec.cc))?;
    if !status.success() {
        bail!(
            "fvm-aot C compiler `{}` exited with status {status}",
            spec.cc
        );
    }
    make_executable(&spec.output_path)?;
    Ok(())
}

/// Compile a JAR strictly through the IR → Cranelift compiler path, with the
/// build-time evaluator fallback disabled. Unsupported constructs fail loudly
/// with milestone-tagged diagnostics instead of being constant-folded. Used by
/// the M1 "compiler-required" tests to prove real compilation vs. build-time
/// evaluation.
#[cfg(test)]
fn compile_jar_compiler_required(spec: &CompileSpec) -> Result<()> {
    let main_class = spec
        .main_class
        .as_deref()
        .context("fvm-aot requires a Main-Class manifest entry or --main-class")?;
    let world = read_class_world(&spec.jar_path)?;
    compiler::CompilerPipeline::from_world(world, main_class)
        .compile_entry(&spec.cc, &spec.output_path, spec.dry_run)
        .context(
            "fvm-aot compiler-required path rejected the program; \
             no build-time evaluator fallback is available on this path",
        )?;
    make_executable(&spec.output_path)?;
    Ok(())
}

fn read_class_world(jar_path: &Path) -> Result<ClassWorld> {
    let file = std::fs::File::open(jar_path)
        .with_context(|| format!("failed to open JAR {}", jar_path.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("failed to read JAR/ZIP archive {}", jar_path.display()))?;

    let mut classes = HashMap::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index)?;
        let name = file.name().to_string();
        if !name.ends_with(".class") || name.ends_with("module-info.class") {
            continue;
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let class_file = ClassFile::parse(&bytes)
            .with_context(|| format!("failed to parse classfile entry {name}"))?;
        classes.insert(class_file.this_name.clone(), class_file);
    }
    if classes.is_empty() {
        bail!("fvm-aot found no class files in JAR {}", jar_path.display());
    }
    Ok(ClassWorld { classes })
}

#[derive(Debug)]
struct AotProgram {
    printlns: Vec<Vec<u8>>,
    http_server: Option<HttpServer>,
}

#[derive(Debug)]
struct HttpServer {
    port: u16,
    body: Vec<u8>,
}

#[derive(Debug)]
struct ClassWorld {
    classes: HashMap<String, ClassFile>,
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

    mod codegen_cranelift;
    mod compiler_pipeline;
    mod current_slice;
    mod differential;
    mod failure_artifacts;
    mod ir;
    mod ir_verify;
    mod link;
    mod lower;
    mod m1_compiler_path;
    mod reachability;
    mod runtime_stub;
    mod unsupported;

    #[test]
    fn rejects_invalid_classfile() {
        let err = ClassFile::parse(b"nope").unwrap_err();
        assert!(err.to_string().contains("truncated") || err.to_string().contains("magic"));
    }
}
