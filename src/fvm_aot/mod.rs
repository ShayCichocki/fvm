use anyhow::{Context, Result, bail};
mod classfile;
mod diagnostics;
mod emitter;
mod evaluator;
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
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::{Duration, Instant};

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

    #[test]
    fn compiles_simple_println_when_toolchain_is_available() {
        if !command_available("javac") || !command_available("cc") {
            return;
        }

        let temp = tempfile::tempdir().unwrap();
        let src_dir = temp.path().join("src");
        let classes_dir = temp.path().join("classes");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::create_dir_all(&classes_dir).unwrap();
        let src = src_dir.join("AotHello.java");
        std::fs::write(
            &src,
            r#"public final class AotHello {
    public static void main(String[] args) {
        System.out.println("hello fvm-aot");
    }
}
"#,
        )
        .unwrap();

        let javac = Command::new("javac")
            .arg("--release")
            .arg("17")
            .arg("-d")
            .arg(&classes_dir)
            .arg(&src)
            .status()
            .unwrap();
        if !javac.success() {
            return;
        }

        let jar = temp.path().join("hello.jar");
        write_test_jar(&jar, &classes_dir.join("AotHello.class"));
        let output = temp.path().join("hello-native");
        compile_jar(&CompileSpec {
            jar_path: jar,
            main_class: Some("AotHello".to_string()),
            output_path: output.clone(),
            cc: "cc".to_string(),
            dry_run: false,
        })
        .unwrap();

        let run = Command::new(output).output().unwrap();
        assert!(run.status.success());
        assert_eq!(String::from_utf8_lossy(&run.stdout), "hello fvm-aot\n");
    }

    #[test]
    fn compiles_computed_http_intrinsic_when_toolchain_is_available() {
        if !command_available("javac") || !command_available("cc") {
            return;
        }

        let temp = tempfile::tempdir().unwrap();
        let src_dir = temp.path().join("src");
        let runtime_dir = src_dir.join("fvm/runtime");
        let classes_dir = temp.path().join("classes");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::create_dir_all(&classes_dir).unwrap();
        let src = src_dir.join("AotHttpEval.java");
        let http = runtime_dir.join("Http.java");
        std::fs::write(
            &src,
            r#"import fvm.runtime.Http;

public final class AotHttpEval {
    static int port() {
        int base = 19000;
        int offset = 91;
        if (offset > 0) {
            return base + offset;
        }
        return 1;
    }

    static String body() {
        return "computed fvm-aot http";
    }

    public static void main(String[] args) {
        Http.respond(port(), body());
    }
}
"#,
        )
        .unwrap();
        std::fs::write(
            &http,
            r#"package fvm.runtime;

public final class Http {
    private Http() {}
    public static void respond(int port, String body) {}
}
"#,
        )
        .unwrap();

        let javac = Command::new("javac")
            .arg("--release")
            .arg("17")
            .arg("-d")
            .arg(&classes_dir)
            .arg(&src)
            .arg(&http)
            .status()
            .unwrap();
        if !javac.success() {
            return;
        }

        let jar = temp.path().join("http.jar");
        write_test_jar_entries(
            &jar,
            "AotHttpEval",
            &[
                ("AotHttpEval.class", classes_dir.join("AotHttpEval.class")),
                (
                    "fvm/runtime/Http.class",
                    classes_dir.join("fvm/runtime/Http.class"),
                ),
            ],
        );
        let output = temp.path().join("http-native");
        compile_jar(&CompileSpec {
            jar_path: jar,
            main_class: Some("AotHttpEval".to_string()),
            output_path: output.clone(),
            cc: "cc".to_string(),
            dry_run: false,
        })
        .unwrap();

        let mut child = Command::new(&output).spawn().unwrap();
        let response = wait_http_response(19091);
        let _ = child.kill();
        let _ = child.wait();
        let response = response.unwrap();
        assert!(response.contains("HTTP/1.1 200 OK"));
        assert!(response.ends_with("computed fvm-aot http"));
    }

    #[test]
    fn compiles_static_fields_and_clinit_when_toolchain_is_available() {
        if !command_available("javac") || !command_available("cc") {
            return;
        }

        let temp = tempfile::tempdir().unwrap();
        let src_dir = temp.path().join("src");
        let runtime_dir = src_dir.join("fvm/runtime");
        let classes_dir = temp.path().join("classes");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::create_dir_all(&classes_dir).unwrap();
        let src = src_dir.join("AotStatic.java");
        let http = runtime_dir.join("Http.java");
        std::fs::write(
            &src,
            r#"import fvm.runtime.Http;

public final class AotStatic {
    static int base = 19000;
    static int offset;
    static String body;

    static {
        offset = 92;
        body = "static fvm-aot http";
    }

    static int port() {
        return base + offset;
    }

    public static void main(String[] args) {
        Http.respond(port(), body);
    }
}
"#,
        )
        .unwrap();
        std::fs::write(
            &http,
            r#"package fvm.runtime;

public final class Http {
    private Http() {}
    public static void respond(int port, String body) {}
}
"#,
        )
        .unwrap();

        let javac = Command::new("javac")
            .arg("--release")
            .arg("17")
            .arg("-d")
            .arg(&classes_dir)
            .arg(&src)
            .arg(&http)
            .status()
            .unwrap();
        if !javac.success() {
            return;
        }

        let jar = temp.path().join("static.jar");
        write_test_jar_entries(
            &jar,
            "AotStatic",
            &[
                ("AotStatic.class", classes_dir.join("AotStatic.class")),
                (
                    "fvm/runtime/Http.class",
                    classes_dir.join("fvm/runtime/Http.class"),
                ),
            ],
        );
        let output = temp.path().join("static-native");
        compile_jar(&CompileSpec {
            jar_path: jar,
            main_class: Some("AotStatic".to_string()),
            output_path: output.clone(),
            cc: "cc".to_string(),
            dry_run: false,
        })
        .unwrap();

        let mut child = Command::new(&output).spawn().unwrap();
        let response = wait_http_response(19092);
        let _ = child.kill();
        let _ = child.wait();
        let response = response.unwrap();
        assert!(response.contains("HTTP/1.1 200 OK"));
        assert!(response.ends_with("static fvm-aot http"));
    }

    #[test]
    fn compiles_objects_and_arrays_when_toolchain_is_available() {
        if !command_available("javac") || !command_available("cc") {
            return;
        }

        let temp = tempfile::tempdir().unwrap();
        let src_dir = temp.path().join("src");
        let runtime_dir = src_dir.join("fvm/runtime");
        let classes_dir = temp.path().join("classes");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::create_dir_all(&classes_dir).unwrap();
        let src = src_dir.join("AotObjects.java");
        let http = runtime_dir.join("Http.java");
        std::fs::write(
            &src,
            r#"import fvm.runtime.Http;

public final class AotObjects {
    int base;
    int[] offsets;
    String[] bodies;

    AotObjects(int base, String body) {
        this.base = base;
        this.offsets = new int[] { 40, 50 };
        this.bodies = new String[] { body };
    }

    int port() {
        return base + offsets[0] + offsets[1] + offsets.length - 2;
    }

    String body() {
        return bodies[0];
    }

    public static void main(String[] args) {
        AotObjects app = new AotObjects(19000, "object array fvm-aot http");
        Http.respond(app.port(), app.body());
    }
}
"#,
        )
        .unwrap();
        std::fs::write(
            &http,
            r#"package fvm.runtime;

public final class Http {
    private Http() {}
    public static void respond(int port, String body) {}
}
"#,
        )
        .unwrap();

        let javac = Command::new("javac")
            .arg("--release")
            .arg("17")
            .arg("-d")
            .arg(&classes_dir)
            .arg(&src)
            .arg(&http)
            .status()
            .unwrap();
        if !javac.success() {
            return;
        }

        let jar = temp.path().join("objects.jar");
        write_test_jar_entries(
            &jar,
            "AotObjects",
            &[
                ("AotObjects.class", classes_dir.join("AotObjects.class")),
                (
                    "fvm/runtime/Http.class",
                    classes_dir.join("fvm/runtime/Http.class"),
                ),
            ],
        );
        let output = temp.path().join("objects-native");
        compile_jar(&CompileSpec {
            jar_path: jar,
            main_class: Some("AotObjects".to_string()),
            output_path: output.clone(),
            cc: "cc".to_string(),
            dry_run: false,
        })
        .unwrap();

        let mut child = Command::new(&output).spawn().unwrap();
        let response = wait_http_response(19090);
        let _ = child.kill();
        let _ = child.wait();
        let response = response.unwrap();
        assert!(response.contains("HTTP/1.1 200 OK"));
        assert!(response.ends_with("object array fvm-aot http"));
    }

    #[test]
    fn compiles_multi_class_closed_world_when_toolchain_is_available() {
        if !command_available("javac") || !command_available("cc") {
            return;
        }

        let temp = tempfile::tempdir().unwrap();
        let src_dir = temp.path().join("src");
        let runtime_dir = src_dir.join("fvm/runtime");
        let classes_dir = temp.path().join("classes");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::create_dir_all(&classes_dir).unwrap();
        let app = src_dir.join("AotMulti.java");
        let config = src_dir.join("AotConfig.java");
        let handler = src_dir.join("AotHandler.java");
        let http = runtime_dir.join("Http.java");
        std::fs::write(
            &app,
            r#"import fvm.runtime.Http;

public final class AotMulti {
    public static void main(String[] args) {
        AotConfig config = new AotConfig(19003, "multi class fvm-aot http");
        AotHandler handler = new AotHandler(config);
        Http.respond(handler.port(), handler.body());
    }
}
"#,
        )
        .unwrap();
        std::fs::write(
            &config,
            r#"public final class AotConfig {
    int base;
    int[] offsets;
    String body;

    AotConfig(int base, String body) {
        this.base = base;
        this.offsets = new int[] { 30, 60 };
        this.body = body;
    }

    int port() {
        return base + offsets[0] + offsets[1];
    }
}
"#,
        )
        .unwrap();
        std::fs::write(
            &handler,
            r#"public final class AotHandler {
    AotConfig config;
    String[] bodies;

    AotHandler(AotConfig config) {
        this.config = config;
        this.bodies = new String[] { config.body };
    }

    int port() {
        return config.port();
    }

    String body() {
        return bodies[0];
    }
}
"#,
        )
        .unwrap();
        std::fs::write(
            &http,
            r#"package fvm.runtime;

public final class Http {
    private Http() {}
    public static void respond(int port, String body) {}
}
"#,
        )
        .unwrap();

        let javac = Command::new("javac")
            .arg("--release")
            .arg("17")
            .arg("-d")
            .arg(&classes_dir)
            .arg(&app)
            .arg(&config)
            .arg(&handler)
            .arg(&http)
            .status()
            .unwrap();
        if !javac.success() {
            return;
        }

        let jar = temp.path().join("multi.jar");
        write_test_jar_entries(
            &jar,
            "AotMulti",
            &[
                ("AotMulti.class", classes_dir.join("AotMulti.class")),
                ("AotConfig.class", classes_dir.join("AotConfig.class")),
                ("AotHandler.class", classes_dir.join("AotHandler.class")),
                (
                    "fvm/runtime/Http.class",
                    classes_dir.join("fvm/runtime/Http.class"),
                ),
            ],
        );
        let output = temp.path().join("multi-native");
        compile_jar(&CompileSpec {
            jar_path: jar,
            main_class: Some("AotMulti".to_string()),
            output_path: output.clone(),
            cc: "cc".to_string(),
            dry_run: false,
        })
        .unwrap();

        let mut child = Command::new(&output).spawn().unwrap();
        let response = wait_http_response(19093);
        let _ = child.kill();
        let _ = child.wait();
        let response = response.unwrap();
        assert!(response.contains("HTTP/1.1 200 OK"));
        assert!(response.ends_with("multi class fvm-aot http"));
    }

    #[test]
    fn compiles_interface_dispatch_and_string_concat_when_toolchain_is_available() {
        if !command_available("javac") || !command_available("cc") {
            return;
        }

        let temp = tempfile::tempdir().unwrap();
        let src_dir = temp.path().join("src");
        let runtime_dir = src_dir.join("fvm/runtime");
        let classes_dir = temp.path().join("classes");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::create_dir_all(&classes_dir).unwrap();
        let app = src_dir.join("AotDispatch.java");
        let responder = src_dir.join("AotResponder.java");
        let config = src_dir.join("AotDispatchConfig.java");
        let handler = src_dir.join("AotDispatchHandler.java");
        let http = runtime_dir.join("Http.java");
        std::fs::write(
            &app,
            r#"import fvm.runtime.Http;

public final class AotDispatch {
    public static void main(String[] args) {
        AotResponder responder = new AotDispatchHandler(new AotDispatchConfig(19000, 94, "fvm"));
        Http.respond(responder.port(), responder.body());
    }
}
"#,
        )
        .unwrap();
        std::fs::write(
            &responder,
            r#"public interface AotResponder {
    int port();
    String body();
}
"#,
        )
        .unwrap();
        std::fs::write(
            &config,
            r#"public final class AotDispatchConfig {
    int base;
    int offset;
    String name;

    AotDispatchConfig(int base, int offset, String name) {
        this.base = base;
        this.offset = offset;
        this.name = name;
    }

    int port() {
        return base + offset;
    }
}
"#,
        )
        .unwrap();
        std::fs::write(
            &handler,
            r#"public final class AotDispatchHandler implements AotResponder {
    AotDispatchConfig config;

    AotDispatchHandler(AotDispatchConfig config) {
        this.config = config;
    }

    public int port() {
        return config.port();
    }

    public String body() {
        return "dispatch " + config.name + " #" + port();
    }
}
"#,
        )
        .unwrap();
        std::fs::write(
            &http,
            r#"package fvm.runtime;

public final class Http {
    private Http() {}
    public static void respond(int port, String body) {}
}
"#,
        )
        .unwrap();

        let javac = Command::new("javac")
            .arg("--release")
            .arg("17")
            .arg("-d")
            .arg(&classes_dir)
            .arg(&app)
            .arg(&responder)
            .arg(&config)
            .arg(&handler)
            .arg(&http)
            .status()
            .unwrap();
        if !javac.success() {
            return;
        }

        let jar = temp.path().join("dispatch.jar");
        write_test_jar_entries(
            &jar,
            "AotDispatch",
            &[
                ("AotDispatch.class", classes_dir.join("AotDispatch.class")),
                ("AotResponder.class", classes_dir.join("AotResponder.class")),
                (
                    "AotDispatchConfig.class",
                    classes_dir.join("AotDispatchConfig.class"),
                ),
                (
                    "AotDispatchHandler.class",
                    classes_dir.join("AotDispatchHandler.class"),
                ),
                (
                    "fvm/runtime/Http.class",
                    classes_dir.join("fvm/runtime/Http.class"),
                ),
            ],
        );
        let output = temp.path().join("dispatch-native");
        compile_jar(&CompileSpec {
            jar_path: jar,
            main_class: Some("AotDispatch".to_string()),
            output_path: output.clone(),
            cc: "cc".to_string(),
            dry_run: false,
        })
        .unwrap();

        let mut child = Command::new(&output).spawn().unwrap();
        let response = wait_http_response(19094);
        let _ = child.kill();
        let _ = child.wait();
        let response = response.unwrap();
        assert!(response.contains("HTTP/1.1 200 OK"));
        assert!(response.ends_with("dispatch fvm #19094"));
    }

    #[test]
    fn compiles_string_object_array_core_methods_when_toolchain_is_available() {
        if !command_available("javac") || !command_available("cc") {
            return;
        }

        let temp = tempfile::tempdir().unwrap();
        let src_dir = temp.path().join("src");
        let runtime_dir = src_dir.join("fvm/runtime");
        let classes_dir = temp.path().join("classes");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::create_dir_all(&classes_dir).unwrap();
        let app = src_dir.join("AotCoreMethods.java");
        let http = runtime_dir.join("Http.java");
        std::fs::write(
            &app,
            r#"import fvm.runtime.Http;

public final class AotCoreMethods {
    static boolean enabled = true;
    static char marker = '!';

    int value;

    AotCoreMethods(int value) {
        this.value = value;
    }

    public static void main(String[] args) {
        String base = "fvm-core";
        String suffix = base.substring(4);
        boolean stringOk = enabled
            && base.length() == 8
            && !base.isEmpty()
            && base.charAt(3) == '-'
            && base.startsWith("fvm")
            && base.endsWith("core")
            && base.contains("m-c")
            && base.equals("fvm-core")
            && suffix.equals("core");

        AotCoreMethods app = new AotCoreMethods(7);
        Object same = app;
        Object sameAgain = app;
        Object other = new AotCoreMethods(7);
        boolean objectOk = same.equals(app)
            && !same.equals(other)
            && same.hashCode() == sameAgain.hashCode()
            && same.toString().startsWith("AotCoreMethods@");

        int[] ports = new int[] { 19000, 95 };
        int[] cloned = ports.clone();
        boolean arrayOk = !ports.equals(cloned)
            && ports.hashCode() != cloned.hashCode()
            && ports.toString().startsWith("[I@");

        String body = base + " " + suffix + " " + stringOk + " " + objectOk + " " + arrayOk + " " + marker;
        Http.respond(ports[0] + cloned[1], body);
    }
}
"#,
        )
        .unwrap();
        std::fs::write(
            &http,
            r#"package fvm.runtime;

public final class Http {
    private Http() {}
    public static void respond(int port, String body) {}
}
"#,
        )
        .unwrap();

        let javac = Command::new("javac")
            .arg("--release")
            .arg("17")
            .arg("-d")
            .arg(&classes_dir)
            .arg(&app)
            .arg(&http)
            .status()
            .unwrap();
        if !javac.success() {
            return;
        }

        let jar = temp.path().join("core-methods.jar");
        write_test_jar_entries(
            &jar,
            "AotCoreMethods",
            &[
                (
                    "AotCoreMethods.class",
                    classes_dir.join("AotCoreMethods.class"),
                ),
                (
                    "fvm/runtime/Http.class",
                    classes_dir.join("fvm/runtime/Http.class"),
                ),
            ],
        );
        let output = temp.path().join("core-methods-native");
        compile_jar(&CompileSpec {
            jar_path: jar,
            main_class: Some("AotCoreMethods".to_string()),
            output_path: output.clone(),
            cc: "cc".to_string(),
            dry_run: false,
        })
        .unwrap();

        let mut child = Command::new(&output).spawn().unwrap();
        let response = wait_http_response(19095);
        let _ = child.kill();
        let _ = child.wait();
        let response = response.unwrap();
        assert!(response.contains("HTTP/1.1 200 OK"));
        assert!(response.ends_with("fvm-core core true true true !"));
    }

    fn command_available(name: &str) -> bool {
        Command::new(name).arg("--version").output().is_ok()
    }

    fn write_test_jar(path: &Path, class_file: &Path) {
        write_test_jar_entries(
            path,
            "AotHello",
            &[("AotHello.class", class_file.to_path_buf())],
        );
    }

    fn assert_unsupported_source(main_class: &str, source: &str, expected: &[&str]) {
        if !command_available("javac") {
            return;
        }

        let temp = tempfile::tempdir().unwrap();
        let Some(jar) = compile_single_source_jar(temp.path(), main_class, source) else {
            return;
        };
        let err = compile_jar(&CompileSpec {
            jar_path: jar,
            main_class: Some(main_class.to_string()),
            output_path: temp.path().join("unsupported-native"),
            cc: "cc".to_string(),
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

    fn compile_single_source_jar(temp: &Path, main_class: &str, source: &str) -> Option<PathBuf> {
        let src_dir = temp.join("src");
        let classes_dir = temp.join("classes");
        std::fs::create_dir_all(&src_dir).ok()?;
        std::fs::create_dir_all(&classes_dir).ok()?;
        let src = src_dir.join(format!("{main_class}.java"));
        std::fs::write(&src, source).ok()?;

        let javac = Command::new("javac")
            .arg("--release")
            .arg("17")
            .arg("-d")
            .arg(&classes_dir)
            .arg(&src)
            .status()
            .ok()?;
        if !javac.success() {
            return None;
        }

        let jar = temp.join(format!("{main_class}.jar"));
        let class_entry = format!("{main_class}.class");
        write_test_jar_entries(
            &jar,
            main_class,
            &[(&class_entry, classes_dir.join(&class_entry))],
        );
        Some(jar)
    }

    fn write_test_jar_entries(path: &Path, main_class: &str, entries: &[(&str, PathBuf)]) {
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::FileOptions::<()>::default();
        zip.start_file("META-INF/MANIFEST.MF", options).unwrap();
        zip.write_all(format!("Manifest-Version: 1.0\nMain-Class: {main_class}\n").as_bytes())
            .unwrap();
        for (name, path) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(&std::fs::read(path).unwrap()).unwrap();
        }
        zip.finish().unwrap();
    }

    fn wait_http_response(port: u16) -> Result<String> {
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) {
                stream.write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n")?;
                let mut response = String::new();
                stream.read_to_string(&mut response)?;
                return Ok(response);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        bail!("timed out waiting for generated HTTP server on {port}")
    }
}
