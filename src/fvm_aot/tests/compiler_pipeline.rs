use crate::fvm_aot::compiler::CompilerPipeline;
use crate::fvm_aot::test_support::{
    AotFixture, ClassEntry, JarSpec, JavaSource, NativeSpec, command_available,
};
use anyhow::Result;

#[test]
fn compiler_pipeline_lowers_simple_main() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[JavaSource {
        relative_path: "AotCompilerPipeline.java",
        contents: r#"public final class AotCompilerPipeline {
    static int helper(int value) {
        return value + 1;
    }

    public static void main(String[] args) {
        helper(41);
    }
}
"#,
    }])?;
    let jar = fixture.package_jar(
        &classes,
        JarSpec {
            jar_name: "compiler-pipeline.jar",
            main_class: "AotCompilerPipeline",
            entries: &[ClassEntry {
                jar_entry: "AotCompilerPipeline.class",
                class_relative_path: "AotCompilerPipeline.class",
            }],
        },
    )?;

    let report = CompilerPipeline::from_jar(&jar, "AotCompilerPipeline")?.run()?;
    let text = report.render_text();
    println!("{text}");

    assert_eq!(
        text,
        "compiler_pipeline:\nreachable:\nclasses:\n  AotCompilerPipeline\nmethods:\n  AotCompilerPipeline.helper(I)I\n  AotCompilerPipeline.main([Ljava/lang/String;)V\nfields:\nlowered:\n  AotCompilerPipeline.helper(I)I verified blocks=1\n  AotCompilerPipeline.main([Ljava/lang/String;)V verified blocks=1\ndiagnostics:\n  <none>\n"
    );

    Ok(())
}

#[test]
fn compiler_pipeline_current_slice_keeps_compile_jar_output() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[JavaSource {
        relative_path: "AotPipelineCurrentSlice.java",
        contents: r#"public final class AotPipelineCurrentSlice {
    public static void main(String[] args) {
        System.out.println("pipeline stays inert");
    }
}
"#,
    }])?;
    let jar = fixture.package_jar(
        &classes,
        JarSpec {
            jar_name: "pipeline-current-slice.jar",
            main_class: "AotPipelineCurrentSlice",
            entries: &[ClassEntry {
                jar_entry: "AotPipelineCurrentSlice.class",
                class_relative_path: "AotPipelineCurrentSlice.class",
            }],
        },
    )?;
    let output = fixture.compile_native(NativeSpec {
        jar_path: jar,
        main_class: "AotPipelineCurrentSlice",
        output_name: "pipeline-current-slice-native",
        dry_run: true,
    })?;
    let placeholder = std::fs::read_to_string(output)?;

    assert!(placeholder.contains("dry-run fvm-aot native binary placeholder"));
    assert!(placeholder.contains("main_class=AotPipelineCurrentSlice"));
    assert!(placeholder.contains("println_count=1"));

    Ok(())
}

#[test]
fn compiler_pipeline_reports_unsupported_long_before_codegen() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[JavaSource {
        relative_path: "AotPipelineLong.java",
        contents: r#"public final class AotPipelineLong {
    public static void main(String[] args) {
        long value = 1L;
        if (value == 2L) {
            return;
        }
    }
}
"#,
    }])?;
    let jar = fixture.package_jar(
        &classes,
        JarSpec {
            jar_name: "pipeline-long.jar",
            main_class: "AotPipelineLong",
            entries: &[ClassEntry {
                jar_entry: "AotPipelineLong.class",
                class_relative_path: "AotPipelineLong.class",
            }],
        },
    )?;

    let report = CompilerPipeline::from_jar(&jar, "AotPipelineLong")?.run()?;
    let text = report.render_text();
    println!("{text}");

    for expected in [
        "diagnostics:",
        "phase=lower",
        "AotPipelineLong.main([Ljava/lang/String;)V",
        "opcode 0x0a",
        "required feature: long primitive bytecode",
        "planned milestone: primitive-completeness",
    ] {
        assert!(
            text.contains(expected),
            "pipeline diagnostic did not contain `{expected}`:\n{text}"
        );
    }
    assert!(
        !text.contains("verified blocks="),
        "unsupported long should stop before lowered-codegen-ready methods:\n{text}"
    );

    Ok(())
}

fn skip_missing_toolchain() -> bool {
    if command_available("javac") {
        return false;
    }
    println!("skipping fvm-aot compiler pipeline test because required tool is missing: javac");
    true
}
