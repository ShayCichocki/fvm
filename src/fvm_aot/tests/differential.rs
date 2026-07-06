use crate::fvm_aot::test_support::{
    AotFixture, ClassEntry, CompiledSources, JarSpec, JavaSource, NativeSpec, run_hotspot,
    run_native,
};

struct StdoutFixture<'a> {
    main_class: &'a str,
    source: &'a str,
    expected_stdout: &'a [u8],
}

struct SingleClassJar<'a> {
    main_class: &'a str,
    jar_name: &'a str,
}

#[test]
fn differential_println_matches_hotspot() {
    if skip_missing_toolchain(&["javac", "java", "cc"]) {
        return;
    }

    assert_aot_matches_hotspot(StdoutFixture {
        main_class: "AotHello",
        source: r#"public final class AotHello {
    public static void main(String[] args) {
        System.out.println("hello fvm-aot");
    }
}
"#,
        expected_stdout: b"hello fvm-aot\n",
    });
}

// Exercises the Modified UTF-8 (CESU-8) constant-pool decoder (PUNCHLIST P0.2)
// and the `String.contains("")` empty-needle case (PUNCHLIST P0.3). The
// supplementary character 😀 (U+1F600) is stored in the class file as a CESU-8
// surrogate pair; `length()` must be 2 UTF-16 units and `hashCode()` must fold
// over both surrogate code units, matching HotSpot exactly.
#[test]
fn differential_unicode_and_contains_match_hotspot() {
    if skip_missing_toolchain(&["javac", "java", "cc"]) {
        return;
    }

    assert_aot_matches_hotspot(StdoutFixture {
        main_class: "AotUnicode",
        source: "public final class AotUnicode {\n\
             \x20   public static void main(String[] args) {\n\
             \x20       System.out.println(\"\u{1F600}\");\n\
             \x20       System.out.println(\"a b\".length());\n\
             \x20       System.out.println(\"\u{1F600}\".hashCode());\n\
             \x20       System.out.println(\"x\".contains(\"\"));\n\
             \x20   }\n\
             }\n",
        expected_stdout: "\u{1F600}\n3\n1772899\ntrue\n".as_bytes(),
    });
}

#[test]
fn differential_unsupported_fixture_fails_before_native_execution() {
    if skip_missing_toolchain(&["javac"]) {
        return;
    }

    let fixture = AotFixture::new().unwrap();
    let classes = fixture
        .compile_sources(&[JavaSource {
            relative_path: "AotDifferentialUnsupported.java",
            contents: r#"public final class AotDifferentialUnsupported {
    public static void main(String[] args) {
        throw null;
    }
}
"#,
        }])
        .unwrap();
    let jar = package_single_class(
        &fixture,
        &classes,
        SingleClassJar {
            main_class: "AotDifferentialUnsupported",
            jar_name: "unsupported.jar",
        },
    );

    let err = fixture
        .compile_native(NativeSpec {
            jar_path: jar,
            main_class: "AotDifferentialUnsupported",
            output_name: "unsupported-native",
            dry_run: true,
        })
        .unwrap_err();
    let text = format!("{err:#}");

    for expected in [
        "opcode 0xbf",
        "fvm-aot exceptions/athrow are not supported yet",
        "fvm-aot bytecode error in AotDifferentialUnsupported.main([Ljava/lang/String;)V at bci",
    ] {
        assert!(
            text.contains(expected),
            "unsupported differential error did not contain `{expected}`:\n{text}"
        );
    }
}

fn assert_aot_matches_hotspot(fixture: StdoutFixture<'_>) {
    let test_fixture = AotFixture::new().unwrap();
    let source_path = format!("{}.java", fixture.main_class);
    let classes = test_fixture
        .compile_sources(&[JavaSource {
            relative_path: &source_path,
            contents: fixture.source,
        }])
        .unwrap();

    // HotSpot is the source of truth: its stdout, stderr, and exit code are what
    // the native binary must reproduce. `expected_stdout` is only a sanity
    // anchor documenting intent — the real gate is native == hotspot.
    let hotspot = run_hotspot(&classes, fixture.main_class).unwrap();
    assert_eq!(
        hotspot.stdout,
        fixture.expected_stdout,
        "HotSpot stdout diverged from the fixture's documented expectation; \
         stderr: {}",
        String::from_utf8_lossy(&hotspot.stderr)
    );

    let jar_name = format!("{}.jar", fixture.main_class);
    let jar = package_single_class(
        &test_fixture,
        &classes,
        SingleClassJar {
            main_class: fixture.main_class,
            jar_name: &jar_name,
        },
    );
    let output = test_fixture
        .compile_native(NativeSpec {
            jar_path: jar,
            main_class: fixture.main_class,
            output_name: "differential-native",
            dry_run: false,
        })
        .unwrap();

    let native = run_native(&output).unwrap();
    assert_behavior_matches(&native, &hotspot);
}

/// Differential comparison against HotSpot across the full observable surface:
/// stdout, stderr, and process exit code (PUNCHLIST P0.5). Comparing only
/// stdout let stderr divergence and wrong exit codes pass silently.
fn assert_behavior_matches(native: &std::process::Output, hotspot: &std::process::Output) {
    let describe = |output: &std::process::Output| {
        format!(
            "exit={:?} stdout={:?} stderr={:?}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        )
    };
    assert_eq!(
        native.stdout,
        hotspot.stdout,
        "stdout diverged\n native: {}\nhotspot: {}",
        describe(native),
        describe(hotspot)
    );
    assert_eq!(
        native.stderr,
        hotspot.stderr,
        "stderr diverged\n native: {}\nhotspot: {}",
        describe(native),
        describe(hotspot)
    );
    assert_eq!(
        native.status.code(),
        hotspot.status.code(),
        "exit code diverged\n native: {}\nhotspot: {}",
        describe(native),
        describe(hotspot)
    );
}

fn package_single_class(
    fixture: &AotFixture,
    classes: &CompiledSources,
    single_class: SingleClassJar<'_>,
) -> std::path::PathBuf {
    let class_entry = format!("{}.class", single_class.main_class);
    fixture
        .package_jar(
            classes,
            JarSpec {
                jar_name: single_class.jar_name,
                main_class: single_class.main_class,
                entries: &[ClassEntry {
                    jar_entry: &class_entry,
                    class_relative_path: &class_entry,
                }],
            },
        )
        .unwrap()
}

fn skip_missing_toolchain(commands: &[&str]) -> bool {
    crate::fvm_aot::test_support::skip_or_require_toolchain(commands)
}
