use super::{CompileSpec, compile_jar};
use anyhow::{Context, Result, bail};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

const KEEP_FAILED_AOT_ENV: &str = "FVM_KEEP_FAILED_AOT";

pub(super) const HTTP_RUNTIME_SOURCE: &str = r#"package fvm.runtime;

public final class Http {
    private Http() {}
    public static void respond(int port, String body) {}
}
"#;

pub(super) struct JavaSource<'a> {
    pub(super) relative_path: &'a str,
    pub(super) contents: &'a str,
}

pub(super) struct ClassEntry<'a> {
    pub(super) jar_entry: &'a str,
    pub(super) class_relative_path: &'a str,
}

pub(super) struct JarSpec<'a> {
    pub(super) jar_name: &'a str,
    pub(super) main_class: &'a str,
    pub(super) entries: &'a [ClassEntry<'a>],
}

pub(super) struct NativeSpec<'a> {
    pub(super) jar_path: PathBuf,
    pub(super) main_class: &'a str,
    pub(super) output_name: &'a str,
    pub(super) dry_run: bool,
}

#[derive(Debug)]
pub(super) struct CompiledSources {
    classes_dir: PathBuf,
}

impl CompiledSources {
    pub(super) fn class_path(&self, relative_path: &str) -> PathBuf {
        self.classes_dir.join(relative_path)
    }
}

pub(super) struct AotFixture {
    temp: Option<tempfile::TempDir>,
}

#[derive(Debug)]
pub(super) struct FailedAotArtifacts {
    retained_dir: Option<PathBuf>,
}

impl FailedAotArtifacts {
    pub(super) fn retained_dir(&self) -> Option<&Path> {
        self.retained_dir.as_deref()
    }
}

impl AotFixture {
    pub(super) fn new() -> Result<Self> {
        Ok(Self {
            temp: Some(tempfile::tempdir().context("failed to create AOT test tempdir")?),
        })
    }

    pub(super) fn compile_sources(&self, sources: &[JavaSource<'_>]) -> Result<CompiledSources> {
        let src_dir = self.path().join("src");
        let mut paths = Vec::with_capacity(sources.len());
        for source in sources {
            let path = src_dir.join(source.relative_path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create source dir {}", parent.display()))?;
            }
            std::fs::write(&path, source.contents)
                .with_context(|| format!("failed to write Java source {}", path.display()))?;
            paths.push(path);
        }
        self.compile_source_paths(&paths)
    }

    pub(super) fn compile_source_paths(&self, sources: &[PathBuf]) -> Result<CompiledSources> {
        let classes_dir = self.path().join("classes");
        std::fs::create_dir_all(&classes_dir)
            .with_context(|| format!("failed to create classes dir {}", classes_dir.display()))?;

        for source in sources {
            if !source.exists() {
                bail!("Java source path does not exist: {}", source.display());
            }
        }

        let status = Command::new("javac")
            .arg("--release")
            .arg("17")
            .arg("-d")
            .arg(&classes_dir)
            .args(sources)
            .status()
            .with_context(|| format!("failed to execute javac for {}", display_paths(sources)))?;
        if !status.success() {
            bail!(
                "javac exited with status {status} for {}",
                display_paths(sources)
            );
        }
        Ok(CompiledSources { classes_dir })
    }

    pub(super) fn package_jar(
        &self,
        classes: &CompiledSources,
        spec: JarSpec<'_>,
    ) -> Result<PathBuf> {
        let jar = self.path().join(spec.jar_name);
        let file = std::fs::File::create(&jar)
            .with_context(|| format!("failed to create test JAR {}", jar.display()))?;
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::FileOptions::<()>::default();
        zip.start_file("META-INF/MANIFEST.MF", options)?;
        zip.write_all(
            format!("Manifest-Version: 1.0\nMain-Class: {}\n", spec.main_class).as_bytes(),
        )?;
        for entry in spec.entries {
            let class_path = classes.class_path(entry.class_relative_path);
            zip.start_file(entry.jar_entry, options)?;
            zip.write_all(
                &std::fs::read(&class_path).with_context(|| {
                    format!("failed to read class file {}", class_path.display())
                })?,
            )?;
        }
        zip.finish()?;
        Ok(jar)
    }

    pub(super) fn compile_native(&self, spec: NativeSpec<'_>) -> Result<PathBuf> {
        let output_path = self.path().join(spec.output_name);
        compile_jar(&CompileSpec {
            jar_path: spec.jar_path,
            main_class: Some(spec.main_class.to_string()),
            output_path: output_path.clone(),
            cc: "cc".to_string(),
            dry_run: spec.dry_run,
        })?;
        Ok(output_path)
    }

    pub(super) fn compile_native_compiler_required(
        &self,
        spec: NativeSpec<'_>,
    ) -> Result<PathBuf> {
        let output_path = self.path().join(spec.output_name);
        super::compile_jar_compiler_required(&CompileSpec {
            jar_path: spec.jar_path,
            main_class: Some(spec.main_class.to_string()),
            output_path: output_path.clone(),
            cc: "cc".to_string(),
            dry_run: spec.dry_run,
        })?;
        Ok(output_path)
    }

    pub(super) fn preserve_failed_artifacts(&mut self, reason: &str) -> FailedAotArtifacts {
        if keep_failed_aot_artifacts() {
            let retained_dir = self.keep_artifacts();
            eprintln!(
                "preserved AOT test artifacts at {} ({reason})",
                retained_dir.display()
            );
            return FailedAotArtifacts {
                retained_dir: Some(retained_dir),
            };
        }

        FailedAotArtifacts { retained_dir: None }
    }

    pub(super) fn artifact_path(&self, relative_path: &str) -> PathBuf {
        self.path().join(relative_path)
    }

    pub(super) fn keep_artifacts(&mut self) -> PathBuf {
        let temp = self.temp.take().expect("AOT test tempdir already consumed");
        temp.keep()
    }

    fn path(&self) -> &Path {
        self.temp
            .as_ref()
            .expect("AOT test tempdir already consumed")
            .path()
    }
}

impl Drop for AotFixture {
    fn drop(&mut self) {
        if std::thread::panicking()
            && keep_failed_aot_artifacts()
            && let Some(temp) = self.temp.take()
        {
            let path = temp.keep();
            eprintln!("preserved AOT test artifacts at {}", path.display());
        }
    }
}

fn keep_failed_aot_artifacts() -> bool {
    std::env::var_os(KEEP_FAILED_AOT_ENV).is_some_and(|value| value == "1")
}

pub(super) fn command_available(name: &str) -> bool {
    Command::new(name).arg("--version").output().is_ok()
}

pub(super) fn run_hotspot(classes: &CompiledSources, main_class: &str) -> Result<Output> {
    Command::new("java")
        .arg("-cp")
        .arg(&classes.classes_dir)
        .arg(main_class)
        .output()
        .with_context(|| format!("failed to run HotSpot baseline for {main_class}"))
}

pub(super) fn run_native(binary: &Path) -> Result<Output> {
    Command::new(binary)
        .output()
        .with_context(|| format!("failed to run native binary {}", binary.display()))
}

pub(super) fn run_native_http(binary: &Path, port: u16) -> Result<String> {
    let mut child = Command::new(binary)
        .spawn()
        .with_context(|| format!("failed to spawn native binary {}", binary.display()))?;
    let response = wait_http_response(port);
    let kill_result = child.kill();
    let wait_result = child.wait();

    match response {
        Ok(response) => Ok(response),
        Err(err) => {
            bail!("{err:#}; cleanup after port {port}: kill={kill_result:?}, wait={wait_result:?}")
        }
    }
}

pub(super) fn wait_http_response(port: u16) -> Result<String> {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if let Ok(mut stream) = std::net::TcpStream::connect(("127.0.0.1", port)) {
            stream.write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n")?;
            let mut response = String::new();
            stream.read_to_string(&mut response)?;
            return Ok(response);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    bail!("timed out waiting for generated HTTP server on port {port}")
}

fn display_paths(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_source_paths_reports_missing_source_path() {
        let fixture = AotFixture::new().unwrap();
        let missing = fixture.path().join("src/Missing.java");

        let err = fixture
            .compile_source_paths(std::slice::from_ref(&missing))
            .unwrap_err();

        assert!(
            format!("{err:#}").contains(&missing.display().to_string()),
            "missing source error did not name `{}`: {err:#}",
            missing.display()
        );
    }

    #[test]
    fn keep_artifacts_returns_tempdir_path() {
        let mut fixture = AotFixture::new().unwrap();
        let path = fixture.keep_artifacts();

        assert!(path.exists());
        std::fs::remove_dir_all(path).unwrap();
    }
}
