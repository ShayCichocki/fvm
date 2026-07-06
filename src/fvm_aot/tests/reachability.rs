use crate::fvm_aot::classfile::ClassFile;
use crate::fvm_aot::reachability::analyze_main;
use crate::fvm_aot::test_support::{AotFixture, ClassEntry, JarSpec, JavaSource};
use crate::fvm_aot::{ClassWorld, read_class_world};
use anyhow::{Context, Result};
use std::collections::HashMap;

#[test]
fn reachability_direct_static_helper() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[
        JavaSource {
            relative_path: "AotReachability.java",
            contents: r#"public final class AotReachability {
    static int seed = 7;

    static {
        seed = 41;
    }

    public static void main(String[] args) {
        helper();
    }

    static int helper() {
        return seed + 1;
    }
}
"#,
        },
        JavaSource {
            relative_path: "AotUnused.java",
            contents: r#"public final class AotUnused {
    static int helper() {
        return 0;
    }
}
"#,
        },
    ])?;
    let jar = fixture.package_jar(
        &classes,
        JarSpec {
            jar_name: "reachability.jar",
            main_class: "AotReachability",
            entries: &[
                ClassEntry {
                    jar_entry: "AotReachability.class",
                    class_relative_path: "AotReachability.class",
                },
                ClassEntry {
                    jar_entry: "AotUnused.class",
                    class_relative_path: "AotUnused.class",
                },
            ],
        },
    )?;
    let world = read_class_world(&jar)?;
    let graph = analyze_main(&world, "AotReachability")?;
    let text = graph.render_text();
    println!("{text}");

    assert_eq!(
        text,
        "classes:\n  AotReachability\nmethods:\n  AotReachability.<clinit>()V\n  AotReachability.helper()I\n  AotReachability.main([Ljava/lang/String;)V\nfields:\n  AotReachability.seed:I\n"
    );
    Ok(())
}

#[test]
fn reachability_dynamic_class_loading_keeps_existing_diagnostic() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[JavaSource {
        relative_path: "AotReachabilityForName.java",
        contents: r#"public final class AotReachabilityForName {
    public static void main(String[] args) throws Exception {
        Class.forName("example.Missing");
    }
}
"#,
    }])?;
    let class_file = parse_class(&classes.class_path("AotReachabilityForName.class"))?;
    let world = ClassWorld {
        classes: HashMap::from([("AotReachabilityForName".to_string(), class_file)]),
    };
    let err = match analyze_main(&world, "AotReachabilityForName") {
        Ok(graph) => anyhow::bail!(
            "dynamic Class.forName was reachable unexpectedly:\n{}",
            graph.render_text()
        ),
        Err(err) => err,
    };
    let text = format!("{err:#}");
    for expected in [
        "dynamic class loading/Class.forName",
        "required feature: closed-world reflection metadata",
        "planned milestone: reflection-and-metadata",
    ] {
        assert!(
            text.contains(expected),
            "reachability error did not contain `{expected}`:\n{text}"
        );
    }
    Ok(())
}

fn parse_class(class_path: &std::path::Path) -> Result<ClassFile> {
    let bytes = std::fs::read(class_path)
        .with_context(|| format!("failed to read class file {}", class_path.display()))?;
    ClassFile::parse(&bytes)
}

fn skip_missing_toolchain() -> bool {
    crate::fvm_aot::test_support::skip_or_require_toolchain(&["javac"])
}
