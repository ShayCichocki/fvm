# FVM AOT Compatibility Matrix

This matrix tracks the intended path from the current tiny `fvm-aot` subset to a closed-world GraalVM Native Image replacement profile.

Status values:

- **Supported**: implemented and tested in committed fixtures.
- **Partial**: implemented for narrow cases; unsupported cases must fail at build time.
- **Planned**: required for the replacement target but not implemented yet.
- **Rejected for now**: intentionally out of scope for the first replacement profile.
- **Non-goal**: not planned for the closed-world service target.

## Workload Profiles

| Profile | Target | Status | Notes |
|---|---|---:|---|
| AOT-0 | Build-time evaluated examples | Supported | Current `aot-http` style examples. |
| AOT-1 | Runtime-compiled plain Java methods | Planned | First real compiler milestone. |
| AOT-2 | Plain Java HTTP service with FVM HTTP API | Planned | No framework dependency. |
| AOT-3 | Plain Java HTTP service using sockets | Planned | Requires `java.net` and I/O. |
| AOT-4 | JSON + logging + resources | Planned | First common dependency set. |
| AOT-5 | Micronaut or Quarkus minimal service | Planned | First framework target. |
| AOT-6 | Spring minimal service | Planned | Later investigation target. |
| AOT-Full-JVM | Arbitrary JVM compatibility | Non-goal | Not required to replace Graal for constrained services. |

## Classfile Format

| Feature | Status | Needed For | Notes |
|---|---:|---|---|
| Classfile magic/version parsing through Java 25 | Supported | All builds | Current parser rejects versions above Java 25. |
| Constant pool basics | Supported | All builds | Utf8, Integer, Class, String, field/method refs, invokedynamic basics. |
| Long/float/double constants | Partial | Primitive completeness | Parsed enough to skip today; value support is planned. |
| Fields and methods | Supported | All builds | Current subset only supports limited descriptors. |
| Code attribute | Supported | Bytecode execution | Exception tables not yet modeled. |
| Exception tables | Planned | Exceptions | Required for real Java. |
| LineNumberTable | Planned | Diagnostics, stack traces | Needed for useful exceptions. |
| LocalVariableTable | Planned | Diagnostics | Useful but not required for execution. |
| BootstrapMethods | Partial | String concat, lambdas | String concat supported; lambda support planned. |
| Runtime-visible annotations | Planned | Frameworks, reflection | Required for DI and JSON metadata. |
| Runtime-invisible annotations | Planned | Framework analysis | Needed for build-time metadata. |
| Signature attribute | Planned | Reflection, frameworks | Required for generic type metadata. |
| InnerClasses/NestHost/NestMembers | Planned | Modern javac output | Needed for access control and reflection. |
| Record attribute | Planned | Records, JSON | Required for Java record binding. |
| PermittedSubclasses | Planned | Sealed classes | Required for modern Java compatibility. |
| Module attributes | Partial | Diagnostics | Should reject unsupported module behavior cleanly. |

## JVM Types

| Type | Status | Runtime Representation Needed | Notes |
|---|---:|---|---|
| `boolean` | Partial | primitive 1-byte or int-compatible | Descriptor support exists in current subset. |
| `byte` | Partial | signed 8-bit | Descriptor/cast/array support exists in current subset. |
| `char` | Partial | Java UTF-16 code unit | Current Rust `char` path rejects surrogate extraction. Needs real UTF-16. |
| `short` | Partial | signed 16-bit | Descriptor/cast/array support exists in current subset. |
| `int` | Supported | 32-bit | Current primitive baseline. |
| `long` | Planned | 64-bit | Required for normal libraries. |
| `float` | Planned | IEEE 754 32-bit | Required for full primitive support. |
| `double` | Planned | IEEE 754 64-bit | Required for normal libraries. |
| references | Partial | pointer or compressed id | Current build-time refs are ids. Runtime refs planned. |
| arrays | Partial | object with length and elements | One-dimensional current subset. Multidimensional planned. |
| `void` | Supported | none | Current subset handles void returns. |

## Bytecode Opcodes

| Category | Opcodes | Status | Notes |
|---|---|---:|---|
| no-op/constants | `nop`, `aconst_null`, `iconst_*`, `bipush`, `sipush`, `ldc`, `ldc_w` | Partial | Integer/String constants supported; long/float/double planned. |
| local load/store int/ref | `iload*`, `aload*`, `istore*`, `astore*` | Supported | Current subset. |
| local load/store long/float/double | `lload*`, `fload*`, `dload*`, `lstore*`, `fstore*`, `dstore*` | Planned | Required for primitive completeness. |
| array load/store int/ref | `iaload`, `aaload`, `iastore`, `aastore` | Supported | Current subset. |
| array load/store byte/char/short | `baload`, `caload`, `saload`, `bastore`, `castore`, `sastore` | Supported | Current subset. |
| array load/store long/float/double | `laload`, `faload`, `daload`, `lastore`, `fastore`, `dastore` | Planned | Required for primitive completeness. |
| stack manipulation simple | `pop`, `dup` | Supported | Current subset. |
| stack manipulation full | `pop2`, `dup_x1`, `dup_x2`, `dup2`, `dup2_x1`, `dup2_x2`, `swap` | Planned | Required for broad javac/library output. |
| int arithmetic | `iadd`, `isub`, `imul`, `idiv`, `irem`, `ineg`, `iinc` | Supported | Current subset. |
| long arithmetic | `ladd`, `lsub`, `lmul`, `ldiv`, `lrem`, `lneg` | Planned | Required. |
| float/double arithmetic | `f*`, `d*` arithmetic | Planned | Required. |
| bitwise/shift int | `ishl`, `ishr`, `iushr`, `iand`, `ior`, `ixor` | Planned | Required for libraries. |
| bitwise/shift long | `lshl`, `lshr`, `lushr`, `land`, `lor`, `lxor` | Planned | Required for libraries. |
| primitive conversions current | `i2b`, `i2c`, `i2s` | Supported | Current subset. |
| primitive conversions full | `i2l`, `i2f`, `i2d`, `l2*`, `f2*`, `d2*` | Planned | Required. |
| integer comparisons | `ifeq` through `if_icmple` | Supported | Current subset. |
| reference comparisons | `if_acmpeq`, `if_acmpne`, `ifnull`, `ifnonnull` | Supported | Current subset. |
| long/float/double comparisons | `lcmp`, `fcmpl`, `fcmpg`, `dcmpl`, `dcmpg` | Planned | Required. |
| goto | `goto` | Supported | Current subset. |
| wide goto/jsr | `goto_w`, `jsr`, `ret`, `jsr_w` | Rejected for now | `jsr` is obsolete; reject unless old bytecode support needed. |
| switch | `tableswitch`, `lookupswitch` | Planned | Required for real javac output. |
| returns current | `ireturn`, `areturn`, `return` | Supported | Current subset. |
| returns full | `lreturn`, `freturn`, `dreturn` | Planned | Required. |
| static fields | `getstatic`, `putstatic` | Partial | Current subset for supported descriptors. Runtime storage planned. |
| instance fields | `getfield`, `putfield` | Partial | Current subset for app-owned fields. Runtime layout planned. |
| method calls | `invokevirtual`, `invokespecial`, `invokestatic`, `invokeinterface` | Partial | Current app-owned dispatch plus core intrinsics. Full runtime dispatch planned. |
| invokedynamic concat | `invokedynamic` with `StringConcatFactory` | Supported | Current subset. |
| invokedynamic lambdas | `invokedynamic` with `LambdaMetafactory` | Planned | Required for normal modern Java. |
| object allocation | `new` | Partial | Current build-time allocation. Runtime allocation planned. |
| primitive arrays | `newarray` | Partial | Current supported primitive descriptors except long/float/double runtime completeness. |
| reference arrays | `anewarray` | Partial | Current one-dimensional supported references. |
| multidimensional arrays | `multianewarray` | Planned | Required. |
| array length | `arraylength` | Supported | Current subset. |
| throw | `athrow` | Planned | Required for exceptions. |
| type checks | `checkcast` | Supported | Current subset. Runtime complete planned. |
| instance checks | `instanceof` | Planned | Required. |
| monitors | `monitorenter`, `monitorexit` | Planned | Required for synchronization. |
| wide prefix | `wide` | Planned | Required for complete bytecode support. |
| breakpoint/impdep | `breakpoint`, `impdep1`, `impdep2` | Non-goal | Should reject. |

## Core Runtime Features

| Feature | Status | Required For | Notes |
|---|---:|---|---|
| Build-time static initialization | Partial | Current examples | Needs policy and sandboxing. |
| Runtime class initialization | Planned | Real Java | Required for side-effecting `<clinit>`. |
| Object header | Planned | Runtime allocation | Current refs are evaluator ids. |
| Class metadata | Planned | Dispatch, reflection, exceptions | Required before real object runtime. |
| Vtables | Planned | Virtual dispatch | Needed for compiled methods. |
| Itables | Planned | Interface dispatch | Needed for compiled methods. |
| Static field storage | Planned | Runtime classes | Current storage is evaluator map. |
| Bump allocator | Planned | Runtime allocation | First allocation milestone. |
| GC | Planned | Long-running services | Stop-the-world first. |
| Exceptions | Planned | Libraries | Mandatory. |
| Stack traces | Planned | Diagnostics | Basic stack traces first. |
| Threads | Planned | Server frameworks | May start with constrained profile. |
| Monitors | Planned | `synchronized` | Required for Java correctness. |
| Volatile | Planned | Concurrency | Required by libraries. |
| Atomics | Planned | `java.util.concurrent` | Required. |
| Safepoints | Planned | GC/threads | Required depending on GC strategy. |

## java.lang

| Class/API | Status | Notes |
|---|---:|---|
| `Object.<init>` | Supported | Current subset treats as no-op. |
| `Object.equals` | Supported | Supported for current refs. |
| `Object.hashCode` | Supported | Deterministic current ref id/hash. Runtime policy planned. |
| `Object.toString` | Supported | Current deterministic format. Runtime class metadata planned. |
| `Object.getClass` | Planned | Requires `Class` object model. |
| `String` core methods | Partial | Selected intrinsics supported. Full UTF-16 model planned. |
| `StringBuilder` | Planned | Required for libraries and older concat patterns. |
| `StringBuffer` | Planned | May be needed by legacy libraries. |
| `Class` | Planned | Reflection, exceptions, class init. |
| `System.out` | Partial | `println` supported for selected values. |
| `System` properties/env/time | Planned | Real apps. |
| `Throwable` | Planned | Exceptions. |
| boxed primitives | Planned | Collections, reflection. |
| `Enum` | Planned | Config, JSON, frameworks. |
| `Math` | Planned | Primitive completeness. |
| `Thread` | Planned | Concurrency profile. |
| `ThreadLocal` | Planned | Frameworks and logging. |
| `StackTraceElement` | Planned | Exceptions and logging. |

## java.util

| Class/API | Status | Notes |
|---|---:|---|
| `Objects` | Planned | Common helper. |
| `Arrays` | Planned | Array operations, equality, copy. |
| `Collections` | Planned | Common helper. |
| `ArrayList` | Planned | Essential. |
| `HashMap` | Planned | Essential. |
| `HashSet` | Planned | Essential. |
| `LinkedHashMap` | Planned | JSON/config order. |
| Iterators | Planned | Collections. |
| `Optional` | Planned | Modern Java. |
| `Properties` | Planned | Config. |
| `UUID` | Planned | Common apps. |
| Regex | Planned | Many libs eventually. |
| `ServiceLoader` | Planned | Frameworks/logging/JDK services. |

## java.time

| Class/API | Status | Notes |
|---|---:|---|
| `Instant` | Planned | Logging, APIs. |
| `Duration` | Planned | Config/timeouts. |
| `LocalDate` | Planned | JSON/apps. |
| `LocalDateTime` | Planned | JSON/apps. |
| `OffsetDateTime` | Planned | JSON/apps. |
| `ZonedDateTime` | Planned | Later due to timezone data. |
| formatters | Planned | Logging/config. |
| timezone database | Planned | Needs resource strategy. |

## java.io and java.nio

| Class/API | Status | Notes |
|---|---:|---|
| `InputStream`/`OutputStream` | Planned | Resources, sockets. |
| readers/writers | Planned | Text I/O. |
| `File` | Planned | Legacy APIs. |
| `Path`/`Files` | Planned | Modern APIs. |
| `ByteBuffer` | Planned | NIO, Netty. |
| charsets UTF-8 | Planned | Strings, HTTP, JSON. |
| broader charsets | Planned | Later. |
| resource loading | Planned | Frameworks and config. |
| memory-mapped files | Rejected for now | Add only if required. |

## java.net and TLS

| Class/API | Status | Notes |
|---|---:|---|
| `URI`/`URL` parsing | Planned | Config/frameworks. |
| DNS lookup | Planned | Outbound clients. |
| `Socket` | Planned | Network runtime. |
| `ServerSocket` | Planned | HTTP servers. |
| nonblocking channels | Planned | Netty/frameworks. |
| HTTP client | Planned | Later unless app requires. |
| TLS sockets | Planned | HTTPS. |
| certificates | Planned | TLS. |
| secure random | Planned | TLS/security. |

## Reflection, Proxies, Resources

| Feature | Status | Notes |
|---|---:|---|
| `Class.forName` known classes | Planned | Reflection. |
| Class metadata queries | Planned | Reflection/frameworks. |
| constructors metadata | Planned | DI/JSON. |
| fields metadata | Planned | JSON/config. |
| methods metadata | Planned | DI/frameworks. |
| annotations | Planned | Frameworks. |
| reflective constructor invocation | Planned | DI/JSON. |
| reflective method invocation | Planned | Frameworks. |
| reflective field access | Planned | JSON/config. |
| dynamic proxies | Planned | Frameworks. |
| runtime-unknown proxies | Rejected for now | Must be closed-world declared. |
| resources include config | Planned | Frameworks/config. |
| ServiceLoader | Planned | Logging/frameworks. |

## Frameworks and Libraries

| Target | Status | Blocking Features |
|---|---:|---|
| Current `fvm.runtime.Http.respond` examples | Supported | None for current fixtures. |
| Plain Java handwritten HTTP | Planned | Runtime codegen, sockets or FVM HTTP API, allocation. |
| JSON with reflection | Planned | Reflection metadata, collections, strings, exceptions. |
| SLF4J/simple logging | Planned | ServiceLoader/resources/time/thread context. |
| Micronaut minimal HTTP | Planned | annotations, reflection metadata, resources, HTTP, JSON. |
| Quarkus minimal HTTP | Planned | similar to Micronaut, plus framework-specific substitutions. |
| Netty | Planned | NIO, ByteBuffer, selectors, concurrency. |
| Spring minimal HTTP | Planned | reflection, proxies, resources, class init, annotations. |
| Hibernate/JPA | Rejected for now | Reflection/proxies/JDBC/classpath complexity. |
| Swing/AWT/JavaFX | Non-goal | Server workload target. |
| JVM agents/instrumentation | Non-goal | Explicitly rejected. |

## Native/System APIs

| Feature | Status | Notes |
|---|---:|---|
| stdout/stderr | Partial | Current generated C uses stdout. Runtime path planned. |
| args | Planned | Standard main args. |
| environment variables | Planned | Config. |
| system properties | Planned | Libraries/frameworks. |
| current time | Planned | Logging/time APIs. |
| monotonic time | Planned | Timeouts. |
| sleep/park | Planned | Threads/concurrency. |
| file reads | Planned | Resources/config. |
| file writes | Planned | Explicit writable paths only. |
| TCP sockets | Planned | HTTP. |
| DNS | Planned | Outbound networking. |
| TLS | Planned | HTTPS. |
| randomness | Planned | TLS/security. |
| process spawning | Rejected for now | Not needed for first service profile. |
| JNI | Rejected for now | Add only with concrete need. |
| dynamic library loading | Rejected for now | Conflicts with closed-world goal. |

## Diagnostics Requirements

Every unsupported feature should report:

- class name
- method name and descriptor
- bytecode offset or metadata location when available
- unsupported feature name
- related compatibility matrix category
- suggested workaround if known

Example shape:

```text
fvm-aot unsupported opcode invokedynamic LambdaMetafactory in com/example/App.handle()V at bci 42
required feature: lambdas/method references
planned milestone: dispatch-and-lambdas
```

Current AOT-0 golden diagnostics cover exceptions/`athrow`, `LambdaMetafactory` lambdas, `Class.forName` dynamic class loading, `long`/`double` primitive bytecode gaps, and multidimensional arrays.
