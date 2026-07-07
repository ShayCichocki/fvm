use crate::fvm_aot::compiler::{CompilerPipeline, StaticIntMethodSpec};
use crate::fvm_aot::test_support::{
    AotFixture, ClassEntry, JarSpec, JavaSource, NativeSpec, run_hotspot,
};
use anyhow::Result;
use std::process::Command;

#[test]
fn cranelift_static_int_method_matches_hotspot() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[JavaSource {
        relative_path: "AotCraneliftStaticInt.java",
        contents: r#"public final class AotCraneliftStaticInt {
    static int helper() {
        return 40 + 2;
    }

    static int entry() {
        return helper();
    }

    public static void main(String[] args) {
        System.out.println(entry());
    }
}
"#,
    }])?;
    let jar = fixture.package_jar(
        &classes,
        JarSpec {
            jar_name: "cranelift-static-int.jar",
            main_class: "AotCraneliftStaticInt",
            entries: &[ClassEntry {
                jar_entry: "AotCraneliftStaticInt.class",
                class_relative_path: "AotCraneliftStaticInt.class",
            }],
        },
    )?;
    let hotspot = run_hotspot(&classes, "AotCraneliftStaticInt")?;
    assert!(hotspot.status.success(), "HotSpot failed: {hotspot:?}");
    let expected_stdout = hotspot.stdout;

    let native = CompilerPipeline::from_jar(&jar, "AotCraneliftStaticInt")?
        .compile_static_int_method(&StaticIntMethodSpec {
            class: "AotCraneliftStaticInt",
            name: "entry",
            descriptor: "()I",
            cc: "cc",
            output_path: &fixture.artifact_path("cranelift-static-int-native"),
        })?;
    let output = Command::new(native.path()).output()?;

    // Result delivered by printing (P1.6), so native stdout matches HotSpot's
    // `System.out.println(entry())` byte-for-byte and the process exits 0 — no
    // more 8-bit exit-code truncation.
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, expected_stdout);
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn cranelift_loop_with_branch_matches_hotspot() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    // A `for` loop (header CondBranch + back-edge) wrapping an `if`/`else`
    // (CondBranch + diamond merge) exercises multi-block Cranelift lowering:
    // block parameters thread the loop-carried `total`/`i` across every edge.
    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[JavaSource {
        relative_path: "AotCraneliftLoop.java",
        contents: r#"public final class AotCraneliftLoop {
    static int entry() {
        int total = 0;
        for (int i = 0; i < 10; i++) {
            if (i < 5) {
                total = total + i;
            } else {
                total = total + 2;
            }
        }
        return total;
    }

    public static void main(String[] args) {
        System.out.println(entry());
    }
}
"#,
    }])?;
    let jar = fixture.package_jar(
        &classes,
        JarSpec {
            jar_name: "cranelift-loop.jar",
            main_class: "AotCraneliftLoop",
            entries: &[ClassEntry {
                jar_entry: "AotCraneliftLoop.class",
                class_relative_path: "AotCraneliftLoop.class",
            }],
        },
    )?;
    let hotspot = run_hotspot(&classes, "AotCraneliftLoop")?;
    assert!(hotspot.status.success(), "HotSpot failed: {hotspot:?}");
    let expected_stdout = hotspot.stdout;

    let native = CompilerPipeline::from_jar(&jar, "AotCraneliftLoop")?.compile_static_int_method(
        &StaticIntMethodSpec {
            class: "AotCraneliftLoop",
            name: "entry",
            descriptor: "()I",
            cc: "cc",
            output_path: &fixture.artifact_path("cranelift-loop-native"),
        },
    )?;
    let output = Command::new(native.path()).output()?;

    // Result delivered by printing (P1.6), so native stdout matches HotSpot's
    // `System.out.println(entry())` byte-for-byte and the process exits 0 — no
    // more 8-bit exit-code truncation.
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, expected_stdout);
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn cranelift_wide_locals_and_iinc_match_hotspot() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    // 260 simultaneously-live int locals force `wide` iload/istore for the
    // slots above 255, and `+= 1000` (constant beyond a signed byte) forces a
    // `wide iinc`.
    let mut source =
        String::from("public final class AotCraneliftWide {\n    static int entry() {\n");
    for index in 0..260 {
        source.push_str(&format!("        int v{index} = {index};\n"));
    }
    source.push_str("        v0 += 1000;\n");
    source.push_str("        return ");
    for index in 0..260 {
        if index > 0 {
            source.push_str(" + ");
        }
        source.push_str(&format!("v{index}"));
    }
    source.push_str(
        ";\n    }\n\n    public static void main(String[] args) {\n        System.out.println(entry());\n    }\n}\n",
    );

    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[JavaSource {
        relative_path: "AotCraneliftWide.java",
        contents: &source,
    }])?;
    let jar = fixture.package_jar(
        &classes,
        JarSpec {
            jar_name: "cranelift-wide.jar",
            main_class: "AotCraneliftWide",
            entries: &[ClassEntry {
                jar_entry: "AotCraneliftWide.class",
                class_relative_path: "AotCraneliftWide.class",
            }],
        },
    )?;
    let hotspot = run_hotspot(&classes, "AotCraneliftWide")?;
    assert!(hotspot.status.success(), "HotSpot failed: {hotspot:?}");
    let expected_stdout = hotspot.stdout;

    let native = CompilerPipeline::from_jar(&jar, "AotCraneliftWide")?.compile_static_int_method(
        &StaticIntMethodSpec {
            class: "AotCraneliftWide",
            name: "entry",
            descriptor: "()I",
            cc: "cc",
            output_path: &fixture.artifact_path("cranelift-wide-native"),
        },
    )?;
    let output = Command::new(native.path()).output()?;

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, expected_stdout);
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn cranelift_switch_tableswitch_and_lookupswitch_match_hotspot() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    // `dense` compiles to `tableswitch` (contiguous cases), `sparse` to
    // `lookupswitch` (scattered cases). `entry` drives hits and defaults through
    // both, exercising the multi-way branch lowering.
    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[JavaSource {
        relative_path: "AotCraneliftSwitch.java",
        contents: r#"public final class AotCraneliftSwitch {
    static int dense(int x) {
        switch (x) {
            case 0: return 100;
            case 1: return 101;
            case 2: return 102;
            case 3: return 103;
            default: return -1;
        }
    }

    static int sparse(int x) {
        switch (x) {
            case 1: return 10;
            case 100: return 20;
            case 1000: return 30;
            default: return 40;
        }
    }

    static int entry() {
        return dense(0) + dense(2) + dense(9)
             + sparse(1) + sparse(1000) + sparse(7);
    }

    public static void main(String[] args) {
        System.out.println(entry());
    }
}
"#,
    }])?;
    let jar = fixture.package_jar(
        &classes,
        JarSpec {
            jar_name: "cranelift-switch.jar",
            main_class: "AotCraneliftSwitch",
            entries: &[ClassEntry {
                jar_entry: "AotCraneliftSwitch.class",
                class_relative_path: "AotCraneliftSwitch.class",
            }],
        },
    )?;
    let hotspot = run_hotspot(&classes, "AotCraneliftSwitch")?;
    assert!(hotspot.status.success(), "HotSpot failed: {hotspot:?}");
    let expected_stdout = hotspot.stdout;

    let native = CompilerPipeline::from_jar(&jar, "AotCraneliftSwitch")?
        .compile_static_int_method(&StaticIntMethodSpec {
            class: "AotCraneliftSwitch",
            name: "entry",
            descriptor: "()I",
            cc: "cc",
            output_path: &fixture.artifact_path("cranelift-switch-native"),
        })?;
    let output = Command::new(native.path()).output()?;

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, expected_stdout);
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn cranelift_dup_via_assignment_chain_matches_hotspot() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    // `a = b = 7` compiles to `iconst; dup; istore b; istore a`, exercising the
    // stack-manipulation `dup`.
    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[JavaSource {
        relative_path: "AotCraneliftDup.java",
        contents: r#"public final class AotCraneliftDup {
    static int entry() {
        int a;
        int b;
        a = b = 7;
        return a + b;
    }

    public static void main(String[] args) {
        System.out.println(entry());
    }
}
"#,
    }])?;
    let jar = fixture.package_jar(
        &classes,
        JarSpec {
            jar_name: "cranelift-dup.jar",
            main_class: "AotCraneliftDup",
            entries: &[ClassEntry {
                jar_entry: "AotCraneliftDup.class",
                class_relative_path: "AotCraneliftDup.class",
            }],
        },
    )?;
    let hotspot = run_hotspot(&classes, "AotCraneliftDup")?;
    assert!(hotspot.status.success(), "HotSpot failed: {hotspot:?}");
    let expected_stdout = hotspot.stdout;

    let native = CompilerPipeline::from_jar(&jar, "AotCraneliftDup")?.compile_static_int_method(
        &StaticIntMethodSpec {
            class: "AotCraneliftDup",
            name: "entry",
            descriptor: "()I",
            cc: "cc",
            output_path: &fixture.artifact_path("cranelift-dup-native"),
        },
    )?;
    let output = Command::new(native.path()).output()?;

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, expected_stdout);
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn cranelift_shifts_bitwise_and_conversions_match_hotspot() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    // Exercises the P1.9 int-adjacent batch: shifts (with Java's `& 0x1f`
    // shift-count mask — `shiftCount()` returns 33, so `<< 33` must act like
    // `<< 1`), bitwise and/or/xor, and the i2b/i2s/i2c narrowing conversions.
    // Helper calls keep the operands out of javac's constant folder.
    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[JavaSource {
        relative_path: "AotCraneliftBits.java",
        contents: r#"public final class AotCraneliftBits {
    static int seed() {
        return -1234567;
    }

    static int shiftCount() {
        return 33;
    }

    static int entry() {
        int x = seed();
        int a = x << shiftCount();
        int b = x >> 3;
        int c = x >>> 3;
        int d = x & 0x00ff00ff;
        int e = x | 0x0f0f0f0f;
        int f = x ^ 0x12345678;
        int g = (byte) x;
        int h = (short) x;
        int i = (char) x;
        return a + b + c + d + e + f + g + h + i;
    }

    public static void main(String[] args) {
        System.out.println(entry());
    }
}
"#,
    }])?;
    let jar = fixture.package_jar(
        &classes,
        JarSpec {
            jar_name: "cranelift-bits.jar",
            main_class: "AotCraneliftBits",
            entries: &[ClassEntry {
                jar_entry: "AotCraneliftBits.class",
                class_relative_path: "AotCraneliftBits.class",
            }],
        },
    )?;
    let hotspot = run_hotspot(&classes, "AotCraneliftBits")?;
    assert!(hotspot.status.success(), "HotSpot failed: {hotspot:?}");
    let expected_stdout = hotspot.stdout;

    let native = CompilerPipeline::from_jar(&jar, "AotCraneliftBits")?.compile_static_int_method(
        &StaticIntMethodSpec {
            class: "AotCraneliftBits",
            name: "entry",
            descriptor: "()I",
            cc: "cc",
            output_path: &fixture.artifact_path("cranelift-bits-native"),
        },
    )?;
    let output = Command::new(native.path()).output()?;

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, expected_stdout);
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn cranelift_int_arrays_match_hotspot() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    // A heap-allocated int array: `new int[n]`, `arraylength`, iastore in one
    // loop and iaload in another — all executing at runtime against HotSpot.
    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[JavaSource {
        relative_path: "AotArrays.java",
        contents: r#"public final class AotArrays {
    static int entry() {
        int[] a = new int[5];
        int i = 0;
        while (i < a.length) {
            a[i] = i * i;
            i++;
        }
        int sum = 0;
        for (int j = 0; j < a.length; j++) {
            sum += a[j];
        }
        return sum;
    }

    public static void main(String[] args) {
        System.out.println(entry());
    }
}
"#,
    }])?;
    let jar = fixture.package_jar(
        &classes,
        JarSpec {
            jar_name: "cranelift-arrays.jar",
            main_class: "AotArrays",
            entries: &[ClassEntry {
                jar_entry: "AotArrays.class",
                class_relative_path: "AotArrays.class",
            }],
        },
    )?;
    let hotspot = run_hotspot(&classes, "AotArrays")?;
    assert!(hotspot.status.success(), "HotSpot failed: {hotspot:?}");
    let expected_stdout = hotspot.stdout;

    let native = CompilerPipeline::from_jar(&jar, "AotArrays")?.compile_static_int_method(
        &StaticIntMethodSpec {
            class: "AotArrays",
            name: "entry",
            descriptor: "()I",
            cc: "cc",
            output_path: &fixture.artifact_path("cranelift-arrays-native"),
        },
    )?;
    let output = Command::new(native.path()).output()?;

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, expected_stdout);
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn cranelift_object_new_fields_and_constructor_match_hotspot() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    // A first real object: `new` allocates on the runtime heap, the constructor
    // (an instance method receiving `this`) writes fields via putfield, and
    // `entry` reads them back via getfield — all executing at runtime.
    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[
        JavaSource {
            relative_path: "Point.java",
            contents: r#"final class Point {
    int x;
    int y;

    Point(int x, int y) {
        this.x = x;
        this.y = y;
    }
}
"#,
        },
        JavaSource {
            relative_path: "AotObjects.java",
            contents: r#"public final class AotObjects {
    static int entry() {
        Point p = new Point(3, 4);
        Point q = new Point(p.x + p.y, p.x * p.y);
        return q.x * 1000 + q.y;
    }

    public static void main(String[] args) {
        System.out.println(entry());
    }
}
"#,
        },
    ])?;
    let jar = fixture.package_jar(
        &classes,
        JarSpec {
            jar_name: "cranelift-objects.jar",
            main_class: "AotObjects",
            entries: &[
                ClassEntry {
                    jar_entry: "AotObjects.class",
                    class_relative_path: "AotObjects.class",
                },
                ClassEntry {
                    jar_entry: "Point.class",
                    class_relative_path: "Point.class",
                },
            ],
        },
    )?;
    let hotspot = run_hotspot(&classes, "AotObjects")?;
    assert!(hotspot.status.success(), "HotSpot failed: {hotspot:?}");
    let expected_stdout = hotspot.stdout;

    let native = CompilerPipeline::from_jar(&jar, "AotObjects")?.compile_static_int_method(
        &StaticIntMethodSpec {
            class: "AotObjects",
            name: "entry",
            descriptor: "()I",
            cc: "cc",
            output_path: &fixture.artifact_path("cranelift-objects-native"),
        },
    )?;
    let output = Command::new(native.path()).output()?;

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, expected_stdout);
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn cranelift_int_edge_case_corpus_matches_hotspot() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    // P1.10 integer edge-case corpus. Every case folds into a rolling hash so a
    // single returned int is sensitive to any one mismatch; helper calls keep
    // operands out of javac's constant folder. HotSpot is the source of truth.
    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[JavaSource {
        relative_path: "AotCraneliftIntEdges.java",
        contents: r#"public final class AotCraneliftIntEdges {
    static int min() { return Integer.MIN_VALUE; }
    static int max() { return Integer.MAX_VALUE; }
    static int negOne() { return -1; }
    static int one() { return 1; }

    static int entry() {
        int acc = 0;
        acc = acc * 31 + (max() + one());     // overflow wrap: MAX + 1 == MIN
        acc = acc * 31 + (min() - one());     // underflow wrap: MIN - 1 == MAX
        acc = acc * 31 + (-min());            // -MIN_VALUE == MIN
        acc = acc * 31 + (min() * negOne());  // multiply overflow: MIN * -1 == MIN
        acc = acc * 31 + (byte) max();        // narrowing cast out of range
        acc = acc * 31 + (short) max();
        acc = acc * 31 + (char) negOne();     // (char) -1 == 65535
        acc = acc * 31 + (one() << negOne()); // shift by negative: amount & 0x1f == 31
        acc = acc * 31 + (max() >> negOne());
        acc = acc * 31 + (max() >>> negOne());
        acc = acc * 31 + (min() / negOne());  // division overflow: MIN / -1 == MIN
        acc = acc * 31 + (min() % negOne());  // MIN % -1 == 0
        int i = max();
        i++;                                  // iinc wrap: MAX + 1 == MIN
        acc = acc * 31 + i;
        return acc;
    }

    public static void main(String[] args) {
        System.out.println(entry());
    }
}
"#,
    }])?;
    let jar = fixture.package_jar(
        &classes,
        JarSpec {
            jar_name: "cranelift-int-edges.jar",
            main_class: "AotCraneliftIntEdges",
            entries: &[ClassEntry {
                jar_entry: "AotCraneliftIntEdges.class",
                class_relative_path: "AotCraneliftIntEdges.class",
            }],
        },
    )?;
    let hotspot = run_hotspot(&classes, "AotCraneliftIntEdges")?;
    assert!(hotspot.status.success(), "HotSpot failed: {hotspot:?}");
    let expected_stdout = hotspot.stdout;

    let native = CompilerPipeline::from_jar(&jar, "AotCraneliftIntEdges")?
        .compile_static_int_method(&StaticIntMethodSpec {
            class: "AotCraneliftIntEdges",
            name: "entry",
            descriptor: "()I",
            cc: "cc",
            output_path: &fixture.artifact_path("cranelift-int-edges-native"),
        })?;
    let output = Command::new(native.path()).output()?;

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, expected_stdout);
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn cranelift_division_matches_hotspot() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    // `denom()` keeps the divisor out of javac's constant folder, so `entry`
    // emits a real `idiv` guarded by a `ZeroCheck`. A non-zero divisor must sail
    // through the guard and match HotSpot.
    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[JavaSource {
        relative_path: "AotCraneliftDivide.java",
        contents: r#"public final class AotCraneliftDivide {
    static int denom() {
        return 4;
    }

    static int entry() {
        return 100 / denom() + 100 % denom();
    }

    public static void main(String[] args) {
        System.out.println(entry());
    }
}
"#,
    }])?;
    let jar = fixture.package_jar(
        &classes,
        JarSpec {
            jar_name: "cranelift-divide.jar",
            main_class: "AotCraneliftDivide",
            entries: &[ClassEntry {
                jar_entry: "AotCraneliftDivide.class",
                class_relative_path: "AotCraneliftDivide.class",
            }],
        },
    )?;
    let hotspot = run_hotspot(&classes, "AotCraneliftDivide")?;
    assert!(hotspot.status.success(), "HotSpot failed: {hotspot:?}");
    let expected_stdout = hotspot.stdout;

    let native = CompilerPipeline::from_jar(&jar, "AotCraneliftDivide")?
        .compile_static_int_method(&StaticIntMethodSpec {
            class: "AotCraneliftDivide",
            name: "entry",
            descriptor: "()I",
            cc: "cc",
            output_path: &fixture.artifact_path("cranelift-divide-native"),
        })?;
    let output = Command::new(native.path()).output()?;

    // Result delivered by printing (P1.6), so native stdout matches HotSpot's
    // `System.out.println(entry())` byte-for-byte and the process exits 0 — no
    // more 8-bit exit-code truncation.
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, expected_stdout);
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn cranelift_min_value_over_minus_one_wraps_like_hotspot() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    // `Integer.MIN_VALUE / -1` overflows: Java wraps it back to MIN_VALUE (and
    // `% -1` is 0), whereas a raw `sdiv`/`srem` would trap. The helpers keep the
    // operands non-constant; `entry` self-checks the wrap and returns a small
    // sentinel so the exit-code ABI can carry the answer (a full-width result
    // waits on P1.6).
    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[JavaSource {
        relative_path: "AotCraneliftDivOverflow.java",
        contents: r#"public final class AotCraneliftDivOverflow {
    static int minValue() {
        return Integer.MIN_VALUE;
    }

    static int negOne() {
        return -1;
    }

    static int entry() {
        int quotient = minValue() / negOne();
        int remainder = minValue() % negOne();
        if (quotient == minValue() && remainder == 0) {
            return 42;
        }
        return 0;
    }

    public static void main(String[] args) {
        System.out.println(entry());
    }
}
"#,
    }])?;
    let jar = fixture.package_jar(
        &classes,
        JarSpec {
            jar_name: "cranelift-div-overflow.jar",
            main_class: "AotCraneliftDivOverflow",
            entries: &[ClassEntry {
                jar_entry: "AotCraneliftDivOverflow.class",
                class_relative_path: "AotCraneliftDivOverflow.class",
            }],
        },
    )?;
    let hotspot = run_hotspot(&classes, "AotCraneliftDivOverflow")?;
    assert!(hotspot.status.success(), "HotSpot failed: {hotspot:?}");
    let expected_stdout = hotspot.stdout;
    assert_eq!(
        String::from_utf8_lossy(&expected_stdout),
        "42\n",
        "HotSpot should confirm the wrap"
    );

    let native = CompilerPipeline::from_jar(&jar, "AotCraneliftDivOverflow")?
        .compile_static_int_method(&StaticIntMethodSpec {
            class: "AotCraneliftDivOverflow",
            name: "entry",
            descriptor: "()I",
            cc: "cc",
            output_path: &fixture.artifact_path("cranelift-div-overflow-native"),
        })?;
    let output = Command::new(native.path()).output()?;

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, expected_stdout);
    assert!(
        output.stderr.is_empty(),
        "no trap expected on the wrap case"
    );
    Ok(())
}

#[test]
fn cranelift_division_by_zero_traps_like_hotspot() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[JavaSource {
        relative_path: "AotCraneliftDivZero.java",
        contents: r#"public final class AotCraneliftDivZero {
    static int denom() {
        return 0;
    }

    static int entry() {
        return 100 / denom();
    }

    public static void main(String[] args) {
        System.out.println(entry());
    }
}
"#,
    }])?;
    let jar = fixture.package_jar(
        &classes,
        JarSpec {
            jar_name: "cranelift-div-zero.jar",
            main_class: "AotCraneliftDivZero",
            entries: &[ClassEntry {
                jar_entry: "AotCraneliftDivZero.class",
                class_relative_path: "AotCraneliftDivZero.class",
            }],
        },
    )?;
    // HotSpot throws an uncaught ArithmeticException and exits 1.
    let hotspot = run_hotspot(&classes, "AotCraneliftDivZero")?;
    assert_eq!(hotspot.status.code(), Some(1), "HotSpot: {hotspot:?}");

    let native = CompilerPipeline::from_jar(&jar, "AotCraneliftDivZero")?
        .compile_static_int_method(&StaticIntMethodSpec {
            class: "AotCraneliftDivZero",
            name: "entry",
            descriptor: "()I",
            cc: "cc",
            output_path: &fixture.artifact_path("cranelift-div-zero-native"),
        })?;
    let output = Command::new(native.path()).output()?;

    // The runtime abort helper diverts the zero divisor: deterministic exit 1
    // and a Java-shaped message, rather than a CPU trap / signal-shaped exit.
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("java.lang.ArithmeticException: / by zero"),
        "unexpected trap stderr: {stderr}"
    );
    assert!(output.stdout.is_empty());
    Ok(())
}

#[test]
fn cranelift_rejects_allocating_class_outside_closed_world() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    // App-owned classes get an object layout (see the object fixture above), but
    // JDK classes like `java/lang/Object` are not in the closed world yet, so
    // allocating one is rejected loudly rather than emitting a bad allocation.
    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[JavaSource {
        relative_path: "AotCraneliftAlloc.java",
        contents: r#"public final class AotCraneliftAlloc {
    static int entry() {
        Object value = new Object();
        return value == null ? 0 : 1;
    }

    public static void main(String[] args) {
        System.out.println(entry());
    }
}
"#,
    }])?;
    let jar = fixture.package_jar(
        &classes,
        JarSpec {
            jar_name: "cranelift-allocation.jar",
            main_class: "AotCraneliftAlloc",
            entries: &[ClassEntry {
                jar_entry: "AotCraneliftAlloc.class",
                class_relative_path: "AotCraneliftAlloc.class",
            }],
        },
    )?;

    let err = CompilerPipeline::from_jar(&jar, "AotCraneliftAlloc")?
        .compile_static_int_method(&StaticIntMethodSpec {
            class: "AotCraneliftAlloc",
            name: "entry",
            descriptor: "()I",
            cc: "cc",
            output_path: &fixture.artifact_path("cranelift-allocation-native"),
        })
        .unwrap_err();
    let message = format!("{err:#}");
    println!("{message}");

    assert!(message.contains("no object layout"), "{message}");
    assert!(message.contains("java/lang/Object"), "{message}");
    Ok(())
}

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
    crate::fvm_aot::test_support::skip_or_require_toolchain(&["javac", "java", "cc"])
}
