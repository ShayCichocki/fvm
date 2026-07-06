use crate::fvm_aot::test_support::{
    AotFixture, ClassEntry, HTTP_RUNTIME_SOURCE, JarSpec, JavaSource, NativeSpec,
    command_available, run_native, run_native_http,
};
use anyhow::Result;

#[test]
fn m1_compiler_path_println_fixture_runs_native() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[JavaSource {
        relative_path: "AotM1Println.java",
        contents: r#"public final class AotM1Println {
    public static void main(String[] args) {
        System.out.println("m1 compiler println");
    }
}
"#,
    }])?;
    let jar = fixture.package_jar(
        &classes,
        JarSpec {
            jar_name: "m1-println.jar",
            main_class: "AotM1Println",
            entries: &[ClassEntry {
                jar_entry: "AotM1Println.class",
                class_relative_path: "AotM1Println.class",
            }],
        },
    )?;

    let output = fixture.compile_native_compiler_required(NativeSpec {
        jar_path: jar,
        main_class: "AotM1Println",
        output_name: "m1-println-native",
        dry_run: false,
    })?;
    let run = run_native(&output)?;

    assert!(run.status.success(), "native failed: {run:?}");
    assert_eq!(String::from_utf8_lossy(&run.stdout), "m1 compiler println\n");
    assert!(run.stderr.is_empty(), "stderr: {}", String::from_utf8_lossy(&run.stderr));
    Ok(())
}

#[test]
fn m1_compiler_path_http_intrinsic_runs_native() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[
        JavaSource {
            relative_path: "AotM1Http.java",
            contents: r#"import fvm.runtime.Http;

public final class AotM1Http {
    public static void main(String[] args) {
        Http.respond(19124, "m1 compiler http");
    }
}
"#,
        },
        JavaSource {
            relative_path: "fvm/runtime/Http.java",
            contents: HTTP_RUNTIME_SOURCE,
        },
    ])?;
    let jar = fixture.package_jar(
        &classes,
        JarSpec {
            jar_name: "m1-http.jar",
            main_class: "AotM1Http",
            entries: &[
                ClassEntry {
                    jar_entry: "AotM1Http.class",
                    class_relative_path: "AotM1Http.class",
                },
                ClassEntry {
                    jar_entry: "fvm/runtime/Http.class",
                    class_relative_path: "fvm/runtime/Http.class",
                },
            ],
        },
    )?;

    let output = fixture.compile_native_compiler_required(NativeSpec {
        jar_path: jar,
        main_class: "AotM1Http",
        output_name: "m1-http-native",
        dry_run: false,
    })?;
    let response = run_native_http(&output, 19124)?;

    assert!(response.contains("HTTP/1.1 200 OK"));
    assert!(response.ends_with("m1 compiler http"));
    Ok(())
}

#[test]
fn m1_compiler_path_required_fixture_rejects_evaluator_only_fallback() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[JavaSource {
        relative_path: "AotM1Fallback.java",
        contents: r#"public final class AotM1Fallback {
    public static void main(String[] args) {
        Object value = new Object();
        System.out.println(value == null ? "bad" : "fallback-only");
    }
}
"#,
    }])?;
    let jar = fixture.package_jar(
        &classes,
        JarSpec {
            jar_name: "m1-fallback.jar",
            main_class: "AotM1Fallback",
            entries: &[ClassEntry {
                jar_entry: "AotM1Fallback.class",
                class_relative_path: "AotM1Fallback.class",
            }],
        },
    )?;

    let err = fixture
        .compile_native_compiler_required(NativeSpec {
            jar_path: jar,
            main_class: "AotM1Fallback",
            output_name: "m1-fallback-native",
            dry_run: false,
        })
        .unwrap_err();
    let message = format!("{err:#}");
    println!("{message}");

    assert!(message.contains("compiler-required"), "{message}");
    assert!(message.contains("runtime allocation") || message.contains("opcode 0xbb"), "{message}");
    Ok(())
}

fn skip_missing_toolchain() -> bool {
    if command_available("javac") && command_available("cc") {
        return false;
    }
    println!("skipping fvm-aot M1 compiler path test because javac or cc is missing");
    true
}
