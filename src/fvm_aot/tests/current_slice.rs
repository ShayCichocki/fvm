// allow: SIZE_OK - this module intentionally groups the documented AOT-0 supported fixtures for `cargo test current_slice`.
use crate::fvm_aot::test_support::{
    AotFixture, ClassEntry, HTTP_RUNTIME_SOURCE, JarSpec, JavaSource, NativeSpec,
    command_available, run_hotspot, run_native, run_native_http,
};

#[test]
fn compiles_simple_println_when_toolchain_is_available() {
    if skip_missing_toolchain(&["javac", "cc"]) {
        return;
    }

    let fixture = AotFixture::new().unwrap();
    let classes = fixture
        .compile_sources(&[JavaSource {
            relative_path: "AotHello.java",
            contents: r#"public final class AotHello {
    public static void main(String[] args) {
        System.out.println("hello fvm-aot");
    }
}
"#,
        }])
        .unwrap();
    if command_available("java") {
        let hotspot = run_hotspot(&classes, "AotHello").unwrap();
        assert!(hotspot.status.success());
        assert_eq!(String::from_utf8_lossy(&hotspot.stdout), "hello fvm-aot\n");
    }
    let jar = fixture
        .package_jar(
            &classes,
            JarSpec {
                jar_name: "hello.jar",
                main_class: "AotHello",
                entries: &[ClassEntry {
                    jar_entry: "AotHello.class",
                    class_relative_path: "AotHello.class",
                }],
            },
        )
        .unwrap();
    let output = fixture
        .compile_native(NativeSpec {
            jar_path: jar,
            main_class: "AotHello",
            output_name: "hello-native",
            dry_run: false,
        })
        .unwrap();
    let run = run_native(&output).unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "hello fvm-aot\n");
}

#[test]
fn compiles_computed_http_intrinsic_when_toolchain_is_available() {
    if skip_missing_toolchain(&["javac", "cc"]) {
        return;
    }

    let fixture = AotFixture::new().unwrap();
    let classes = fixture
        .compile_sources(&[
            JavaSource {
                relative_path: "AotHttpEval.java",
                contents: r#"import fvm.runtime.Http;

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
            },
            JavaSource {
                relative_path: "fvm/runtime/Http.java",
                contents: HTTP_RUNTIME_SOURCE,
            },
        ])
        .unwrap();
    let jar = fixture
        .package_jar(
            &classes,
            JarSpec {
                jar_name: "http.jar",
                main_class: "AotHttpEval",
                entries: &[
                    ClassEntry {
                        jar_entry: "AotHttpEval.class",
                        class_relative_path: "AotHttpEval.class",
                    },
                    ClassEntry {
                        jar_entry: "fvm/runtime/Http.class",
                        class_relative_path: "fvm/runtime/Http.class",
                    },
                ],
            },
        )
        .unwrap();
    let output = fixture
        .compile_native(NativeSpec {
            jar_path: jar,
            main_class: "AotHttpEval",
            output_name: "http-native",
            dry_run: false,
        })
        .unwrap();
    let response = run_native_http(&output, 19091).unwrap();
    assert!(response.contains("HTTP/1.1 200 OK"));
    assert!(response.ends_with("computed fvm-aot http"));
}

#[test]
fn compiles_static_fields_and_clinit_when_toolchain_is_available() {
    if skip_missing_toolchain(&["javac", "cc"]) {
        return;
    }

    let fixture = AotFixture::new().unwrap();
    let classes = fixture
        .compile_sources(&[
            JavaSource {
                relative_path: "AotStatic.java",
                contents: r#"import fvm.runtime.Http;

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
            },
            JavaSource {
                relative_path: "fvm/runtime/Http.java",
                contents: HTTP_RUNTIME_SOURCE,
            },
        ])
        .unwrap();
    let jar = fixture
        .package_jar(
            &classes,
            JarSpec {
                jar_name: "static.jar",
                main_class: "AotStatic",
                entries: &[
                    ClassEntry {
                        jar_entry: "AotStatic.class",
                        class_relative_path: "AotStatic.class",
                    },
                    ClassEntry {
                        jar_entry: "fvm/runtime/Http.class",
                        class_relative_path: "fvm/runtime/Http.class",
                    },
                ],
            },
        )
        .unwrap();
    let output = fixture
        .compile_native(NativeSpec {
            jar_path: jar,
            main_class: "AotStatic",
            output_name: "static-native",
            dry_run: false,
        })
        .unwrap();
    let response = run_native_http(&output, 19092).unwrap();
    assert!(response.contains("HTTP/1.1 200 OK"));
    assert!(response.ends_with("static fvm-aot http"));
}

#[test]
fn compiles_objects_and_arrays_when_toolchain_is_available() {
    if skip_missing_toolchain(&["javac", "cc"]) {
        return;
    }

    let fixture = AotFixture::new().unwrap();
    let classes = fixture
        .compile_sources(&[
            JavaSource {
                relative_path: "AotObjects.java",
                contents: r#"import fvm.runtime.Http;

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
            },
            JavaSource {
                relative_path: "fvm/runtime/Http.java",
                contents: HTTP_RUNTIME_SOURCE,
            },
        ])
        .unwrap();
    let jar = fixture
        .package_jar(
            &classes,
            JarSpec {
                jar_name: "objects.jar",
                main_class: "AotObjects",
                entries: &[
                    ClassEntry {
                        jar_entry: "AotObjects.class",
                        class_relative_path: "AotObjects.class",
                    },
                    ClassEntry {
                        jar_entry: "fvm/runtime/Http.class",
                        class_relative_path: "fvm/runtime/Http.class",
                    },
                ],
            },
        )
        .unwrap();
    let output = fixture
        .compile_native(NativeSpec {
            jar_path: jar,
            main_class: "AotObjects",
            output_name: "objects-native",
            dry_run: false,
        })
        .unwrap();
    let response = run_native_http(&output, 19090).unwrap();
    assert!(response.contains("HTTP/1.1 200 OK"));
    assert!(response.ends_with("object array fvm-aot http"));
}

#[test]
fn compiles_multi_class_closed_world_when_toolchain_is_available() {
    if skip_missing_toolchain(&["javac", "cc"]) {
        return;
    }

    let fixture = AotFixture::new().unwrap();
    let classes = fixture
        .compile_sources(&[
            JavaSource {
                relative_path: "AotMulti.java",
                contents: r#"import fvm.runtime.Http;

public final class AotMulti {
    public static void main(String[] args) {
        AotConfig config = new AotConfig(19003, "multi class fvm-aot http");
        AotHandler handler = new AotHandler(config);
        Http.respond(handler.port(), handler.body());
    }
}
"#,
            },
            JavaSource {
                relative_path: "AotConfig.java",
                contents: r#"public final class AotConfig {
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
            },
            JavaSource {
                relative_path: "AotHandler.java",
                contents: r#"public final class AotHandler {
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
            },
            JavaSource {
                relative_path: "fvm/runtime/Http.java",
                contents: HTTP_RUNTIME_SOURCE,
            },
        ])
        .unwrap();
    let jar = fixture
        .package_jar(
            &classes,
            JarSpec {
                jar_name: "multi.jar",
                main_class: "AotMulti",
                entries: &[
                    ClassEntry {
                        jar_entry: "AotMulti.class",
                        class_relative_path: "AotMulti.class",
                    },
                    ClassEntry {
                        jar_entry: "AotConfig.class",
                        class_relative_path: "AotConfig.class",
                    },
                    ClassEntry {
                        jar_entry: "AotHandler.class",
                        class_relative_path: "AotHandler.class",
                    },
                    ClassEntry {
                        jar_entry: "fvm/runtime/Http.class",
                        class_relative_path: "fvm/runtime/Http.class",
                    },
                ],
            },
        )
        .unwrap();
    let output = fixture
        .compile_native(NativeSpec {
            jar_path: jar,
            main_class: "AotMulti",
            output_name: "multi-native",
            dry_run: false,
        })
        .unwrap();
    let response = run_native_http(&output, 19093).unwrap();
    assert!(response.contains("HTTP/1.1 200 OK"));
    assert!(response.ends_with("multi class fvm-aot http"));
}

#[test]
fn compiles_interface_dispatch_and_string_concat_when_toolchain_is_available() {
    if skip_missing_toolchain(&["javac", "cc"]) {
        return;
    }

    let fixture = AotFixture::new().unwrap();
    let classes = fixture
        .compile_sources(&[
            JavaSource {
                relative_path: "AotDispatch.java",
                contents: r#"import fvm.runtime.Http;

public final class AotDispatch {
    public static void main(String[] args) {
        AotResponder responder = new AotDispatchHandler(new AotDispatchConfig(19000, 94, "fvm"));
        Http.respond(responder.port(), responder.body());
    }
}
"#,
            },
            JavaSource {
                relative_path: "AotResponder.java",
                contents: r#"public interface AotResponder {
    int port();
    String body();
}
"#,
            },
            JavaSource {
                relative_path: "AotDispatchConfig.java",
                contents: r#"public final class AotDispatchConfig {
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
            },
            JavaSource {
                relative_path: "AotDispatchHandler.java",
                contents: r#"public final class AotDispatchHandler implements AotResponder {
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
            },
            JavaSource {
                relative_path: "fvm/runtime/Http.java",
                contents: HTTP_RUNTIME_SOURCE,
            },
        ])
        .unwrap();
    let jar = fixture
        .package_jar(
            &classes,
            JarSpec {
                jar_name: "dispatch.jar",
                main_class: "AotDispatch",
                entries: &[
                    ClassEntry {
                        jar_entry: "AotDispatch.class",
                        class_relative_path: "AotDispatch.class",
                    },
                    ClassEntry {
                        jar_entry: "AotResponder.class",
                        class_relative_path: "AotResponder.class",
                    },
                    ClassEntry {
                        jar_entry: "AotDispatchConfig.class",
                        class_relative_path: "AotDispatchConfig.class",
                    },
                    ClassEntry {
                        jar_entry: "AotDispatchHandler.class",
                        class_relative_path: "AotDispatchHandler.class",
                    },
                    ClassEntry {
                        jar_entry: "fvm/runtime/Http.class",
                        class_relative_path: "fvm/runtime/Http.class",
                    },
                ],
            },
        )
        .unwrap();
    let output = fixture
        .compile_native(NativeSpec {
            jar_path: jar,
            main_class: "AotDispatch",
            output_name: "dispatch-native",
            dry_run: false,
        })
        .unwrap();
    let response = run_native_http(&output, 19094).unwrap();
    assert!(response.contains("HTTP/1.1 200 OK"));
    assert!(response.ends_with("dispatch fvm #19094"));
}

#[test]
fn compiles_string_object_array_core_methods_when_toolchain_is_available() {
    if skip_missing_toolchain(&["javac", "cc"]) {
        return;
    }

    let fixture = AotFixture::new().unwrap();
    let classes = fixture
        .compile_sources(&[
            JavaSource {
                relative_path: "AotCoreMethods.java",
                contents: r#"import fvm.runtime.Http;

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
            },
            JavaSource {
                relative_path: "fvm/runtime/Http.java",
                contents: HTTP_RUNTIME_SOURCE,
            },
        ])
        .unwrap();
    let jar = fixture
        .package_jar(
            &classes,
            JarSpec {
                jar_name: "core-methods.jar",
                main_class: "AotCoreMethods",
                entries: &[
                    ClassEntry {
                        jar_entry: "AotCoreMethods.class",
                        class_relative_path: "AotCoreMethods.class",
                    },
                    ClassEntry {
                        jar_entry: "fvm/runtime/Http.class",
                        class_relative_path: "fvm/runtime/Http.class",
                    },
                ],
            },
        )
        .unwrap();
    let output = fixture
        .compile_native(NativeSpec {
            jar_path: jar,
            main_class: "AotCoreMethods",
            output_name: "core-methods-native",
            dry_run: false,
        })
        .unwrap();
    let response = run_native_http(&output, 19095).unwrap();
    assert!(response.contains("HTTP/1.1 200 OK"));
    assert!(response.ends_with("fvm-core core true true true !"));
}

fn skip_missing_toolchain(commands: &[&str]) -> bool {
    crate::fvm_aot::test_support::skip_or_require_toolchain(commands)
}
