use anyhow::{Context, Result, bail};
mod classfile;
mod diagnostics;
mod emitter;
mod evaluator;
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
    use crate::fvm_aot::test_support::{
        AotFixture, ClassEntry, JarSpec, JavaSource, NativeSpec, command_available,
    };

    mod current_slice;

    #[test]
    fn rejects_invalid_classfile() {
        let err = ClassFile::parse(b"nope").unwrap_err();
        assert!(err.to_string().contains("truncated") || err.to_string().contains("magic"));
    }

    #[test]
    fn unsupported_athrow_reports_class_method_and_bci() {
        assert_unsupported_source(
            "AotUnsupportedThrow",
            r#"public final class AotUnsupportedThrow {
    public static void main(String[] args) {
        throw null;
    }
}
"#,
            &[
                "fvm-aot bytecode error in AotUnsupportedThrow.main([Ljava/lang/String;)V at bci",
                "opcode 0xbf",
                "fvm-aot exceptions/athrow are not supported yet",
            ],
        );
    }

    #[test]
    fn unsupported_lambda_reports_required_feature() {
        assert_unsupported_source(
            "AotUnsupportedLambda",
            r#"public final class AotUnsupportedLambda {
    public static void main(String[] args) {
        Runnable runnable = () -> System.out.println("lambda");
        runnable.run();
    }
}
"#,
            &[
                "fvm-aot bytecode error in AotUnsupportedLambda.main([Ljava/lang/String;)V at bci",
                "opcode 0xba",
                "LambdaMetafactory",
                "required feature: lambdas/method references",
                "planned milestone: dispatch-and-lambdas",
            ],
        );
    }

    #[test]
    fn unsupported_dynamic_class_loading_reports_required_feature() {
        assert_unsupported_source(
            "AotUnsupportedClassForName",
            r#"public final class AotUnsupportedClassForName {
    public static void main(String[] args) throws Exception {
        Class.forName("example.Missing");
    }
}
"#,
            &[
                "fvm-aot bytecode error in AotUnsupportedClassForName.main([Ljava/lang/String;)V at bci",
                "opcode 0xb8",
                "dynamic class loading/Class.forName",
                "required feature: closed-world reflection metadata",
                "planned milestone: reflection-and-metadata",
            ],
        );
    }

    #[test]
    fn unsupported_long_and_double_report_primitive_gap() {
        assert_unsupported_source(
            "AotUnsupportedLong",
            r#"public final class AotUnsupportedLong {
    public static void main(String[] args) {
        long value = 1L;
        System.out.println(value);
    }
}
"#,
            &[
                "fvm-aot bytecode error in AotUnsupportedLong.main([Ljava/lang/String;)V at bci",
                "required feature: long primitive bytecode",
                "planned milestone: primitive-completeness",
            ],
        );

        assert_unsupported_source(
            "AotUnsupportedDouble",
            r#"public final class AotUnsupportedDouble {
    public static void main(String[] args) {
        double value = 1.0d;
        System.out.println(value);
    }
}
"#,
            &[
                "fvm-aot bytecode error in AotUnsupportedDouble.main([Ljava/lang/String;)V at bci",
                "required feature: double primitive bytecode",
                "planned milestone: primitive-completeness",
            ],
        );
    }

    #[test]
    fn unsupported_multidimensional_array_reports_required_feature() {
        assert_unsupported_source(
            "AotUnsupportedMultiArray",
            r#"public final class AotUnsupportedMultiArray {
    public static void main(String[] args) {
        int[][] values = new int[1][1];
        System.out.println(values.length);
    }
}
"#,
            &[
                "fvm-aot bytecode error in AotUnsupportedMultiArray.main([Ljava/lang/String;)V at bci",
                "opcode 0xc5",
                "required feature: multidimensional arrays",
                "planned milestone: primitive-completeness",
            ],
        );
    }

    fn assert_unsupported_source(main_class: &str, source: &str, expected: &[&str]) {
        if !command_available("javac") {
            return;
        }

        let fixture = AotFixture::new().unwrap();
        let classes = fixture
            .compile_sources(&[JavaSource {
                relative_path: &format!("{main_class}.java"),
                contents: source,
            }])
            .unwrap();
        let class_entry = format!("{main_class}.class");
        let jar = fixture
            .package_jar(
                &classes,
                JarSpec {
                    jar_name: &format!("{main_class}.jar"),
                    main_class,
                    entries: &[ClassEntry {
                        jar_entry: &class_entry,
                        class_relative_path: &class_entry,
                    }],
                },
            )
            .unwrap();
        let err = fixture
            .compile_native(NativeSpec {
                jar_path: jar,
                main_class,
                output_name: "unsupported-native",
                dry_run: true,
            })
            .unwrap_err();
        let text = format!("{err:#}");

        for expected in expected {
            assert!(
                text.contains(expected),
                "error did not contain `{expected}`:\n{text}"
            );
        }
    }
}
