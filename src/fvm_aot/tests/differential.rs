use crate::fvm_aot::test_support::{
    AotFixture, ClassEntry, CompiledSources, JarSpec, JavaSource, NativeSpec, command_available,
    run_hotspot, run_native,
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

    let hotspot = run_hotspot(&classes, fixture.main_class).unwrap();
    assert!(hotspot.status.success(), "HotSpot failed: {hotspot:?}");
    assert_eq!(hotspot.stdout, fixture.expected_stdout);

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
    assert!(native.status.success(), "native binary failed: {native:?}");
    assert_eq!(native.stdout, fixture.expected_stdout);
    assert_eq!(native.stdout, hotspot.stdout);
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
    let missing = commands
        .iter()
        .copied()
        .filter(|command| !command_available(command))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return false;
    }
    println!(
        "skipping fvm-aot differential test because required tool(s) are missing: {}",
        missing.join(", ")
    );
    true
}
