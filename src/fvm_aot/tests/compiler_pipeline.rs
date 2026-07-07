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
fn cranelift_reference_arrays_match_hotspot() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    // A reference array of app objects: `anewarray`, `aastore` object refs,
    // `aaload` them back, and read a field through each — exercised at runtime.
    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[
        JavaSource {
            relative_path: "Node.java",
            contents: r#"final class Node {
    int v;

    Node(int v) {
        this.v = v;
    }
}
"#,
        },
        JavaSource {
            relative_path: "AotRefArrays.java",
            contents: r#"public final class AotRefArrays {
    static int entry() {
        Node[] ns = new Node[3];
        ns[0] = new Node(10);
        ns[1] = new Node(20);
        ns[2] = new Node(30);
        int sum = 0;
        for (int i = 0; i < ns.length; i++) {
            sum += ns[i].v;
        }
        return sum;
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
            jar_name: "cranelift-ref-arrays.jar",
            main_class: "AotRefArrays",
            entries: &[
                ClassEntry {
                    jar_entry: "AotRefArrays.class",
                    class_relative_path: "AotRefArrays.class",
                },
                ClassEntry {
                    jar_entry: "Node.class",
                    class_relative_path: "Node.class",
                },
            ],
        },
    )?;
    let hotspot = run_hotspot(&classes, "AotRefArrays")?;
    assert!(hotspot.status.success(), "HotSpot failed: {hotspot:?}");
    let expected_stdout = hotspot.stdout;

    let native = CompilerPipeline::from_jar(&jar, "AotRefArrays")?.compile_static_int_method(
        &StaticIntMethodSpec {
            class: "AotRefArrays",
            name: "entry",
            descriptor: "()I",
            cc: "cc",
            output_path: &fixture.artifact_path("cranelift-ref-arrays-native"),
        },
    )?;
    let output = Command::new(native.path()).output()?;

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, expected_stdout);
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn cranelift_null_and_bounds_traps_match_hotspot() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    // Each fixture triggers a runtime trap. HotSpot exits 1 with the exception
    // on stderr; the compiled binary must do the same (deterministic exit +
    // Java-shaped message) via the branch-to-runtime-helper trap mechanism.
    struct Trap {
        name: &'static str,
        body: &'static str,
        exception: &'static str,
    }
    let traps = [
        Trap {
            name: "AotNpe",
            body: "int[] a = maybeNull(); return a.length;",
            exception: "java.lang.NullPointerException",
        },
        Trap {
            name: "AotOob",
            body: "int[] a = new int[3]; return a[index()];",
            exception: "java.lang.ArrayIndexOutOfBoundsException",
        },
        Trap {
            name: "AotNeg",
            body: "int[] a = new int[negativeSize()]; return a.length;",
            exception: "java.lang.NegativeArraySizeException",
        },
    ];

    for trap in traps {
        let fixture = AotFixture::new()?;
        let source = format!(
            "public final class {name} {{\n\
             \x20   static int[] maybeNull() {{ return null; }}\n\
             \x20   static int index() {{ return 7; }}\n\
             \x20   static int negativeSize() {{ return -1; }}\n\
             \x20   static int entry() {{ {body} }}\n\
             \x20   public static void main(String[] args) {{ System.out.println(entry()); }}\n\
             }}\n",
            name = trap.name,
            body = trap.body,
        );
        let classes = fixture.compile_sources(&[JavaSource {
            relative_path: &format!("{}.java", trap.name),
            contents: &source,
        }])?;
        let jar = fixture.package_jar(
            &classes,
            JarSpec {
                jar_name: "trap.jar",
                main_class: trap.name,
                entries: &[ClassEntry {
                    jar_entry: &format!("{}.class", trap.name),
                    class_relative_path: &format!("{}.class", trap.name),
                }],
            },
        )?;
        let hotspot = run_hotspot(&classes, trap.name)?;
        assert_eq!(hotspot.status.code(), Some(1), "{}: {hotspot:?}", trap.name);

        let native = CompilerPipeline::from_jar(&jar, trap.name)?.compile_static_int_method(
            &StaticIntMethodSpec {
                class: trap.name,
                name: "entry",
                descriptor: "()I",
                cc: "cc",
                output_path: &fixture.artifact_path(&format!("{}-native", trap.name)),
            },
        )?;
        let output = Command::new(native.path()).output()?;

        assert_eq!(output.status.code(), Some(1), "{}", trap.name);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(trap.exception),
            "{} stderr missing {}: {stderr}",
            trap.name,
            trap.exception
        );
        assert!(output.stdout.is_empty(), "{}", trap.name);
    }
    Ok(())
}

#[test]
fn cranelift_string_concat_matches_hotspot() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    // `+` on strings compiles to a StringConcatFactory invokedynamic; the recipe
    // interleaves literal text with dynamic int/String arguments. This is what
    // makes `println("Result: " + compute())` work.
    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[JavaSource {
        relative_path: "AotConcat.java",
        contents: r#"public final class AotConcat {
    static int compute() {
        return 6 * 7;
    }

    static String label(int n) {
        return "n=" + n + " (squared " + (n * n) + ")";
    }

    static void run() {
        String who = "world";
        System.out.println("Hello, " + who + "!");
        System.out.println("The answer is " + compute());
        System.out.println(label(9));
        System.out.println(label(9) + " and " + label(2));
    }

    public static void main(String[] args) {
        run();
    }
}
"#,
    }])?;
    let jar = fixture.package_jar(
        &classes,
        JarSpec {
            jar_name: "cranelift-concat.jar",
            main_class: "AotConcat",
            entries: &[ClassEntry {
                jar_entry: "AotConcat.class",
                class_relative_path: "AotConcat.class",
            }],
        },
    )?;
    let hotspot = run_hotspot(&classes, "AotConcat")?;
    assert!(hotspot.status.success(), "HotSpot failed: {hotspot:?}");
    let expected_stdout = hotspot.stdout;

    let native = CompilerPipeline::from_jar(&jar, "AotConcat")?.compile_static_int_method(
        &StaticIntMethodSpec {
            class: "AotConcat",
            name: "run",
            descriptor: "()V",
            cc: "cc",
            output_path: &fixture.artifact_path("cranelift-concat-native"),
        },
    )?;
    let output = Command::new(native.path()).output()?;

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, expected_stdout);
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn cranelift_println_hello_world_matches_hotspot() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    // The first program that actually *says* something: string literals, int
    // printing, print-without-newline, an empty println, and a static call —
    // all through the `System.out.print/println` intrinsic path, void entry.
    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[JavaSource {
        relative_path: "AotHello.java",
        contents: r#"public final class AotHello {
    static int square(int x) {
        return x * x;
    }

    static void run() {
        System.out.println("Hello, fvm!");
        System.out.print("2 + 2 = ");
        System.out.println(2 + 2);
        System.out.println(square(7));
        System.out.println();
        System.out.println("done");
    }

    public static void main(String[] args) {
        run();
    }
}
"#,
    }])?;
    let jar = fixture.package_jar(
        &classes,
        JarSpec {
            jar_name: "cranelift-hello.jar",
            main_class: "AotHello",
            entries: &[ClassEntry {
                jar_entry: "AotHello.class",
                class_relative_path: "AotHello.class",
            }],
        },
    )?;
    let hotspot = run_hotspot(&classes, "AotHello")?;
    assert!(hotspot.status.success(), "HotSpot failed: {hotspot:?}");
    let expected_stdout = hotspot.stdout;
    assert!(
        String::from_utf8_lossy(&expected_stdout).contains("Hello, fvm!"),
        "sanity: HotSpot printed the greeting"
    );

    let native = CompilerPipeline::from_jar(&jar, "AotHello")?.compile_static_int_method(
        &StaticIntMethodSpec {
            class: "AotHello",
            name: "run",
            descriptor: "()V",
            cc: "cc",
            output_path: &fixture.artifact_path("cranelift-hello-native"),
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
fn cranelift_subword_arrays_match_hotspot() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    // Sub-word arrays: byte/short pack narrow and sign-extend on load; char/
    // boolean zero-extend; every store truncates to the element width. Values
    // out of each type's range exercise the narrowing, and a rolling hash makes
    // the single returned int sensitive to any divergence from HotSpot.
    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[JavaSource {
        relative_path: "AotSubwordArrays.java",
        contents: r#"public final class AotSubwordArrays {
    static int entry() {
        int acc = 0;

        byte[] b = new byte[4];
        b[0] = (byte) 200;              // -> -56 (sign-extended on load)
        b[1] = 127;
        b[2] = (byte) -1;
        b[3] = (byte) (b[0] + b[1]);
        for (int i = 0; i < b.length; i++) acc = acc * 31 + b[i];

        char[] c = new char[3];
        c[0] = 'A';
        c[1] = (char) 65535;            // zero-extended -> 65535
        c[2] = (char) 70000;            // truncated -> 4464
        for (int i = 0; i < c.length; i++) acc = acc * 31 + c[i];

        short[] s = new short[3];
        s[0] = (short) 40000;           // -> -25536
        s[1] = -1;
        s[2] = (short) (s[0] - 1);
        for (int i = 0; i < s.length; i++) acc = acc * 31 + s[i];

        boolean[] z = new boolean[2];
        z[0] = true;
        z[1] = false;
        acc = acc * 31 + (z[0] ? 1 : 0);
        acc = acc * 31 + (z[1] ? 1 : 0);
        acc = acc * 31 + z.length;

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
            jar_name: "cranelift-subword-arrays.jar",
            main_class: "AotSubwordArrays",
            entries: &[ClassEntry {
                jar_entry: "AotSubwordArrays.class",
                class_relative_path: "AotSubwordArrays.class",
            }],
        },
    )?;
    let hotspot = run_hotspot(&classes, "AotSubwordArrays")?;
    assert!(hotspot.status.success(), "HotSpot failed: {hotspot:?}");
    let expected_stdout = hotspot.stdout;

    let native = CompilerPipeline::from_jar(&jar, "AotSubwordArrays")?.compile_static_int_method(
        &StaticIntMethodSpec {
            class: "AotSubwordArrays",
            name: "entry",
            descriptor: "()I",
            cc: "cc",
            output_path: &fixture.artifact_path("cranelift-subword-arrays-native"),
        },
    )?;
    let output = Command::new(native.path()).output()?;

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, expected_stdout);
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn cranelift_static_fields_match_hotspot() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    // Application static fields: `<clinit>` installs the initial values
    // (putstatic of the field initializers), `bump` reads and writes them
    // (getstatic/putstatic in a helper), and `entry` reads the final value —
    // all against per-class static storage executing at runtime.
    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[JavaSource {
        relative_path: "AotStatics.java",
        contents: r#"public final class AotStatics {
    static int counter = 100;
    static int step = 5;

    static void bump() {
        counter += step;
    }

    static int entry() {
        bump();
        bump();
        bump();
        return counter;
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
            jar_name: "cranelift-statics.jar",
            main_class: "AotStatics",
            entries: &[ClassEntry {
                jar_entry: "AotStatics.class",
                class_relative_path: "AotStatics.class",
            }],
        },
    )?;
    let hotspot = run_hotspot(&classes, "AotStatics")?;
    assert!(hotspot.status.success(), "HotSpot failed: {hotspot:?}");
    let expected_stdout = hotspot.stdout;
    assert_eq!(
        String::from_utf8_lossy(&expected_stdout).trim(),
        "115",
        "sanity: 100 + 3*5"
    );

    let native = CompilerPipeline::from_jar(&jar, "AotStatics")?.compile_static_int_method(
        &StaticIntMethodSpec {
            class: "AotStatics",
            name: "entry",
            descriptor: "()I",
            cc: "cc",
            output_path: &fixture.artifact_path("cranelift-statics-native"),
        },
    )?;
    let output = Command::new(native.path()).output()?;

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, expected_stdout);
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn cranelift_cross_class_static_init_matches_hotspot() -> Result<()> {
    if skip_missing_toolchain() {
        return Ok(());
    }

    // Cross-class static initialization order is *observable*: `AotStaticInit`'s
    // `<clinit>` reads `Config.scaled`, which only holds its computed value once
    // `Config`'s `<clinit>` has run. If the initializers ran in the wrong order,
    // `Config.scaled` would still be 0 and `total` would be 5 (→ 15), not 35
    // (→ 45). This pins the dependency ordering against HotSpot's lazy init.
    let fixture = AotFixture::new()?;
    let classes = fixture.compile_sources(&[
        JavaSource {
            relative_path: "Config.java",
            contents: r#"final class Config {
    static int seed = 10;
    static int scaled = seed * 3;
}
"#,
        },
        JavaSource {
            relative_path: "AotStaticInit.java",
            contents: r#"public final class AotStaticInit {
    static int total = Config.scaled + 5;

    static int entry() {
        return total + Config.seed;
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
            jar_name: "cranelift-static-init.jar",
            main_class: "AotStaticInit",
            entries: &[
                ClassEntry {
                    jar_entry: "AotStaticInit.class",
                    class_relative_path: "AotStaticInit.class",
                },
                ClassEntry {
                    jar_entry: "Config.class",
                    class_relative_path: "Config.class",
                },
            ],
        },
    )?;
    let hotspot = run_hotspot(&classes, "AotStaticInit")?;
    assert!(hotspot.status.success(), "HotSpot failed: {hotspot:?}");
    let expected_stdout = hotspot.stdout;
    assert_eq!(
        String::from_utf8_lossy(&expected_stdout).trim(),
        "45",
        "sanity: (10*3 + 5) + 10"
    );

    let native = CompilerPipeline::from_jar(&jar, "AotStaticInit")?.compile_static_int_method(
        &StaticIntMethodSpec {
            class: "AotStaticInit",
            name: "entry",
            descriptor: "()I",
            cc: "cc",
            output_path: &fixture.artifact_path("cranelift-static-init-native"),
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
