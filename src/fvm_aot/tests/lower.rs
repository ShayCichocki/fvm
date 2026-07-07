use crate::fvm_aot::classfile::{ClassFile, Method};
use crate::fvm_aot::lower::lower_method_to_ir;
use crate::fvm_aot::test_support::{AotFixture, JavaSource, run_hotspot};
use anyhow::{Context, Result};

#[test]
fn lower_int_arithmetic_to_ir() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[JavaSource {
        relative_path: "AotLowerInt.java",
        contents: r#"public final class AotLowerInt {
    static int arithmetic(int left, int right) {
        int value = left + right;
        value = value + 100;
        value = value + 300;
        value = value + 70000;
        value = value - 3;
        value = value * 2;
        value = value / right;
        value = value % 5;
        value = -value;
        value++;
        return value;
    }

    public static void main(String[] args) {
        System.out.println(arithmetic(10, 4));
    }
}
"#,
    }])?;
    let hotspot = run_hotspot(&classes, "AotLowerInt")?;
    assert!(hotspot.status.success(), "HotSpot failed: {hotspot:?}");
    assert_eq!(String::from_utf8_lossy(&hotspot.stdout), "1\n");

    let class_file = parse_class(&classes.class_path("AotLowerInt.class"))?;
    let method = find_method(&class_file, "arithmetic", "(II)I")?;
    let ir = lower_method_to_ir(&class_file, method)?;
    let text = ir.render_text();
    println!("{text}");

    assert_eq!(
        text,
        "fn AotLowerInt.arithmetic(v0: int, v1: int) -> int {\nbb0 -> []:\n  param local0 = v0: int\n  param local1 = v1: int\n  v2 = add v0, v1\n  v3 = const int 100\n  v4 = add v2, v3\n  v5 = const int 300\n  v6 = add v4, v5\n  v7 = const int 70000\n  v8 = add v6, v7\n  v9 = const int 3\n  v10 = sub v8, v9\n  v11 = const int 2\n  v12 = mul v10, v11\n  check_nonzero v1 else trap divide_by_zero\n  v13 = div v12, v1\n  v14 = const int 5\n  check_nonzero v14 else trap divide_by_zero\n  v15 = rem v13, v14\n  v16 = neg v15\n  v17 = const int 1\n  v18 = add v16, v17\n  return v18\n}\n"
    );
    ir.verify()?;

    Ok(())
}

#[test]
fn lower_unsupported_long_reports_primitive_completeness() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[JavaSource {
        relative_path: "AotLowerLong.java",
        contents: r#"public final class AotLowerLong {
    static void usesLong() {
        long value = 1L;
        if (value == 2L) {
            System.out.println(value);
        }
    }

    public static void main(String[] args) {
        usesLong();
    }
}
"#,
    }])?;
    let class_file = parse_class(&classes.class_path("AotLowerLong.class"))?;
    let method = find_method(&class_file, "usesLong", "()V")?;
    let err = match lower_method_to_ir(&class_file, method) {
        Ok(ir) => anyhow::bail!("long bytecode lowered unexpectedly:\n{}", ir.render_text()),
        Err(err) => err,
    };
    let text = format!("{err:#}");

    for expected in [
        "opcode 0x0a",
        "required feature: long primitive bytecode",
        "planned milestone: primitive-completeness",
        "AotLowerLong.usesLong()V",
    ] {
        assert!(
            text.contains(expected),
            "lowering error did not contain `{expected}`:\n{text}"
        );
    }

    Ok(())
}

#[test]
fn lower_branches_to_blocks() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[JavaSource {
        relative_path: "AotLowerBranches.java",
        contents: r#"public final class AotLowerBranches {
    static int loop(int limit) {
        int total = 0;
        int index = 0;
        while (index < limit) {
            if (index < 2) {
                total = total + index;
            } else {
                total = total - 1;
            }
            index++;
        }
        return total;
    }

    static int isNull(Object value) {
        if (value == null) {
            return 1;
        }
        return 0;
    }

    public static void main(String[] args) {
        System.out.println(loop(4));
    }
}
"#,
    }])?;
    let hotspot = run_hotspot(&classes, "AotLowerBranches")?;
    assert!(hotspot.status.success(), "HotSpot failed: {hotspot:?}");
    assert_eq!(String::from_utf8_lossy(&hotspot.stdout), "-1\n");

    let class_file = parse_class(&classes.class_path("AotLowerBranches.class"))?;
    let method = find_method(&class_file, "loop", "(I)I")?;
    let ir = lower_method_to_ir(&class_file, method)?;
    let text = ir.render_text();
    println!("{text}");

    let block_headers = text
        .lines()
        .filter(|line| line.starts_with("bb"))
        .collect::<Vec<_>>();
    assert_eq!(
        block_headers,
        [
            "bb0 -> [bb1]:",
            "bb1(v3: int, v4: int, v5: int) -> [bb6, bb2]:",
            "bb2(v10: int, v11: int, v12: int) -> [bb4, bb3]:",
            "bb3(v18: int, v19: int, v20: int) -> [bb5]:",
            "bb4(v15: int, v16: int, v17: int) -> [bb5]:",
            "bb5(v22: int, v23: int, v24: int) -> [bb1]:",
            "bb6(v7: int, v8: int, v9: int) -> []:",
        ]
    );
    // The loop's back-edge must thread the *updated* loop-carried values as
    // block arguments (the phi), not silently reuse the first predecessor's.
    for expected in [
        "branch_if",
        "cmp_int_ge",
        "branch bb1(v22, v23, v28)",
        "return",
    ] {
        assert!(
            text.contains(expected),
            "missing `{expected}` in IR:\n{text}"
        );
    }
    ir.verify()?;

    let ref_method = find_method(&class_file, "isNull", "(Ljava/lang/Object;)I")?;
    let ref_ir = lower_method_to_ir(&class_file, ref_method)?;
    let ref_text = ref_ir.render_text();
    println!("{ref_text}");
    assert!(
        ref_text.contains("cmp_ref_is_non_null_placeholder"),
        "missing reference branch placeholder in IR:\n{ref_text}"
    );
    ref_ir.verify()?;

    Ok(())
}

#[test]
fn lower_diamond_merge_threads_both_predecessor_values() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[JavaSource {
        relative_path: "AotMerge.java",
        contents: r#"public final class AotMerge {
    static int pick(int c, int a, int b) {
        int r;
        if (c != 0) {
            r = a;
        } else {
            r = b;
        }
        return r + 1;
    }

    public static void main(String[] args) {
        System.out.println(pick(1, 7, 9));
        System.out.println(pick(0, 7, 9));
    }
}
"#,
    }])?;
    let hotspot = run_hotspot(&classes, "AotMerge")?;
    assert!(hotspot.status.success(), "HotSpot failed: {hotspot:?}");
    assert_eq!(String::from_utf8_lossy(&hotspot.stdout), "8\n10\n");

    let class_file = parse_class(&classes.class_path("AotMerge.class"))?;
    let method = find_method(&class_file, "pick", "(III)I")?;
    let ir = lower_method_to_ir(&class_file, method)?;
    let text = ir.render_text();
    println!("{text}");
    ir.verify()?;

    // The merge block reached from both arms of the `if` must receive `r` as a
    // block parameter, and each predecessor edge must pass its own value for it.
    // First-writer-wins snapshots silently dropped one arm; block parameters do
    // not.
    let merge_header = text
        .lines()
        .find(|line| line.starts_with("bb3("))
        .expect("merge block bb3 should declare parameters");
    assert_eq!(
        merge_header.matches(": int").count(),
        4,
        "merge block should carry all four live locals as params:\n{text}"
    );
    let edges = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("branch bb3"))
        .map(str::to_string)
        .collect::<Vec<_>>();
    assert_eq!(
        edges.len(),
        2,
        "merge block bb3 should have two edges:\n{text}"
    );
    assert_ne!(
        edges[0], edges[1],
        "each predecessor must thread its own value for r:\n{text}"
    );

    Ok(())
}

#[test]
fn lower_rejects_branch_target_out_of_range() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[JavaSource {
        relative_path: "AotLowerBadBranch.java",
        contents: r#"public final class AotLowerBadBranch {
    static int branch(int value) {
        if (value == 0) {
            return 1;
        }
        return 2;
    }
}
"#,
    }])?;
    let class_file = parse_class(&classes.class_path("AotLowerBadBranch.class"))?;
    let method = find_method(&class_file, "branch", "(I)I")?;
    let mut corrupt_method = method.clone();
    let code = corrupt_method
        .code
        .as_mut()
        .context("compiled branch method had no Code attribute")?;
    let branch_bci = find_branch_bci(&code.bytes)?;
    let offset_bytes = code
        .bytes
        .get_mut(branch_bci + 1..branch_bci + 3)
        .context("branch offset bytes missing")?;
    offset_bytes.copy_from_slice(&i16::MAX.to_be_bytes());

    let err = match lower_method_to_ir(&class_file, &corrupt_method) {
        Ok(ir) => anyhow::bail!("corrupt branch lowered unexpectedly:\n{}", ir.render_text()),
        Err(err) => err,
    };
    let text = format!("{err:#}");
    for expected in [
        "AotLowerBadBranch.branch(I)I",
        "bci",
        "opcode 0x",
        "branch target",
        "out of range",
    ] {
        assert!(
            text.contains(expected),
            "branch error did not contain `{expected}`:\n{text}"
        );
    }

    Ok(())
}

#[test]
fn lower_rejects_exception_table_pointing_at_phase_five() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[JavaSource {
        relative_path: "AotLowerTryCatch.java",
        contents: r#"public final class AotLowerTryCatch {
    static int guarded(int value) {
        try {
            return 100 / value;
        } catch (ArithmeticException e) {
            return -1;
        }
    }

    public static void main(String[] args) {
        System.out.println(guarded(0));
    }
}
"#,
    }])?;
    let class_file = parse_class(&classes.class_path("AotLowerTryCatch.class"))?;
    let method = find_method(&class_file, "guarded", "(I)I")?;
    let err = match lower_method_to_ir(&class_file, method) {
        Ok(ir) => anyhow::bail!("try/catch lowered unexpectedly:\n{}", ir.render_text()),
        Err(err) => err,
    };
    let text = format!("{err:#}");
    for expected in [
        "AotLowerTryCatch.guarded(I)I",
        "exception handlers",
        "Phase 5",
    ] {
        assert!(
            text.contains(expected),
            "exception-table error did not contain `{expected}`:\n{text}"
        );
    }

    Ok(())
}

fn parse_class(class_path: &std::path::Path) -> Result<ClassFile> {
    let bytes = std::fs::read(class_path)
        .with_context(|| format!("failed to read class file {}", class_path.display()))?;
    ClassFile::parse(&bytes)
}

fn find_method<'a>(class_file: &'a ClassFile, name: &str, descriptor: &str) -> Result<&'a Method> {
    class_file
        .methods
        .iter()
        .find(|method| method.name == name && method.descriptor == descriptor)
        .with_context(|| format!("method {name}{descriptor} not found"))
}

fn find_branch_bci(code: &[u8]) -> Result<usize> {
    code.iter()
        .position(|opcode| matches!(*opcode, 0x99..=0xa7 | 0xc6 | 0xc7))
        .context("branch opcode not found")
}

fn skip_missing_toolchain() -> bool {
    crate::fvm_aot::test_support::skip_or_require_toolchain(&["javac", "java"])
}
