use crate::fvm_aot::test_support::{
    AotFixture, ClassEntry, JarSpec, JavaSource, NativeSpec, command_available,
};

struct UnsupportedFixture<'a> {
    main_class: &'a str,
    source: &'a str,
    diagnostics: &'a [&'a str],
    context: &'a [&'a str],
}

#[test]
fn unsupported_athrow_reports_class_method_and_bci() {
    assert_unsupported_source(UnsupportedFixture {
        main_class: "AotUnsupportedThrow",
        source: r#"public final class AotUnsupportedThrow {
    public static void main(String[] args) {
        throw null;
    }
}
"#,
        diagnostics: &[
            "opcode 0xbf",
            "fvm-aot exceptions/athrow are not supported yet",
        ],
        context: &[
            "fvm-aot bytecode error in AotUnsupportedThrow.main([Ljava/lang/String;)V at bci",
        ],
    });
}

#[test]
fn unsupported_lambda_reports_required_feature_and_milestone() {
    assert_unsupported_source(UnsupportedFixture {
        main_class: "AotUnsupportedLambda",
        source: r#"public final class AotUnsupportedLambda {
    public static void main(String[] args) {
        Runnable runnable = () -> System.out.println("lambda");
        runnable.run();
    }
}
"#,
        diagnostics: &[
            "opcode 0xba",
            "LambdaMetafactory",
            "required feature: lambdas/method references",
        ],
        context: &[
            "fvm-aot bytecode error in AotUnsupportedLambda.main([Ljava/lang/String;)V at bci",
            "planned milestone: dispatch-and-lambdas",
        ],
    });
}

#[test]
fn unsupported_dynamic_class_loading_reports_required_feature_and_milestone() {
    assert_unsupported_source(UnsupportedFixture {
        main_class: "AotUnsupportedClassForName",
        source: r#"public final class AotUnsupportedClassForName {
    public static void main(String[] args) throws Exception {
        Class.forName("example.Missing");
    }
}
"#,
        diagnostics: &[
            "opcode 0xb8",
            "dynamic class loading/Class.forName",
            "required feature: closed-world reflection metadata",
        ],
        context: &[
            "fvm-aot bytecode error in AotUnsupportedClassForName.main([Ljava/lang/String;)V at bci",
            "planned milestone: reflection-and-metadata",
        ],
    });
}

#[test]
fn unsupported_long_and_double_report_primitive_gap_and_milestone() {
    for fixture in [
        UnsupportedFixture {
            main_class: "AotUnsupportedLong",
            source: r#"public final class AotUnsupportedLong {
    public static void main(String[] args) {
        long value = 1L;
        System.out.println(value);
    }
}
"#,
            diagnostics: &["opcode 0x0a", "required feature: long primitive bytecode"],
            context: &[
                "fvm-aot bytecode error in AotUnsupportedLong.main([Ljava/lang/String;)V at bci",
                "planned milestone: primitive-completeness",
            ],
        },
        UnsupportedFixture {
            main_class: "AotUnsupportedDouble",
            source: r#"public final class AotUnsupportedDouble {
    public static void main(String[] args) {
        double value = 1.0d;
        System.out.println(value);
    }
}
"#,
            diagnostics: &["opcode 0x0f", "required feature: double primitive bytecode"],
            context: &[
                "fvm-aot bytecode error in AotUnsupportedDouble.main([Ljava/lang/String;)V at bci",
                "planned milestone: primitive-completeness",
            ],
        },
    ] {
        assert_unsupported_source(fixture);
    }
}

#[test]
fn unsupported_multidimensional_array_reports_required_feature_and_milestone() {
    assert_unsupported_source(UnsupportedFixture {
        main_class: "AotUnsupportedMultiArray",
        source: r#"public final class AotUnsupportedMultiArray {
    public static void main(String[] args) {
        int[][] values = new int[1][1];
        System.out.println(values.length);
    }
}
"#,
        diagnostics: &["opcode 0xc5", "required feature: multidimensional arrays"],
        context: &[
            "fvm-aot bytecode error in AotUnsupportedMultiArray.main([Ljava/lang/String;)V at bci",
            "planned milestone: primitive-completeness",
        ],
    });
}

#[test]
fn unsupported_tableswitch_reports_primitive_completeness_milestone() {
    assert_unsupported_source(UnsupportedFixture {
        main_class: "AotUnsupportedTableSwitch",
        source: r#"public final class AotUnsupportedTableSwitch {
    static int value = 1;

    public static void main(String[] args) {
        int selected = value;
        switch (selected) {
            case 0:
                System.out.println("zero");
                break;
            case 1:
                System.out.println("one");
                break;
            case 2:
                System.out.println("two");
                break;
            case 3:
                System.out.println("three");
                break;
            case 4:
                System.out.println("four");
                break;
            case 5:
                System.out.println("five");
                break;
            default:
                System.out.println("many");
                break;
        }
    }
}
"#,
        diagnostics: &["opcode 0xaa", "required feature: switch bytecodes"],
        context: &[
            "fvm-aot bytecode error in AotUnsupportedTableSwitch.main([Ljava/lang/String;)V at bci",
            "planned milestone: primitive-completeness",
        ],
    });
}

fn assert_unsupported_source(fixture: UnsupportedFixture<'_>) {
    assert!(
        fixture.diagnostics.len() >= 2,
        "unsupported fixture {} must assert at least two diagnostic substrings",
        fixture.main_class
    );
    assert!(
        !fixture.context.is_empty(),
        "unsupported fixture {} must assert location or milestone context",
        fixture.main_class
    );

    if !command_available("javac") {
        println!(
            "skipping fvm-aot unsupported test for {} because required tool is missing: javac",
            fixture.main_class
        );
        return;
    }

    let test_fixture = AotFixture::new().unwrap();
    let source_path = format!("{}.java", fixture.main_class);
    let classes = test_fixture
        .compile_sources(&[JavaSource {
            relative_path: &source_path,
            contents: fixture.source,
        }])
        .unwrap();
    let class_entry = format!("{}.class", fixture.main_class);
    let jar_name = format!("{}.jar", fixture.main_class);
    let jar = test_fixture
        .package_jar(
            &classes,
            JarSpec {
                jar_name: &jar_name,
                main_class: fixture.main_class,
                entries: &[ClassEntry {
                    jar_entry: &class_entry,
                    class_relative_path: &class_entry,
                }],
            },
        )
        .unwrap();
    let err = test_fixture
        .compile_native(NativeSpec {
            jar_path: jar,
            main_class: fixture.main_class,
            output_name: "unsupported-native",
            dry_run: true,
        })
        .unwrap_err();
    let text = format!("{err:#}");

    for expected in fixture
        .diagnostics
        .iter()
        .chain(fixture.context.iter())
        .copied()
    {
        assert!(
            text.contains(expected),
            "error for {} did not contain `{expected}`:\n{text}",
            fixture.main_class
        );
    }
}
