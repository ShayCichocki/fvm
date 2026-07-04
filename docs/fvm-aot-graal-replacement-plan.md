# FVM AOT Graal Replacement Plan

## Executive Summary

`fvm-aot` should become an FVM-owned Java-to-native backend for closed-world Java services. The practical target is to replace GraalVM Native Image for selected server workloads, not to implement a full HotSpot-compatible JVM first.

The key shift is from the current build-time evaluator to a real closed-world compiler plus small runtime:

```text
JAR / classpath
  -> classfile parser
  -> closed-world reachability
  -> type and method graph
  -> bytecode IR
  -> native codegen
  -> FVM runtime objects, strings, arrays, exceptions, memory, threads, syscalls
  -> /app binary
  -> existing FVM Firecracker artifact pipeline
```

The Firecracker packaging layer already works. The hard work is Java semantics.

## Strategic Target

Primary target:

```text
Replace GraalVM Native Image for closed-world Java HTTP services that fit an explicit compatibility profile.
```

Secondary target:

```text
Provide a path toward broader Java compatibility only when measured app demand justifies it.
```

The right product contract is narrower than HotSpot and more predictable than Graal:

- no dynamic class loading unless a future profile explicitly allows it
- no Java agents, JVMTI, or runtime instrumentation
- no JIT
- reflection only through closed-world metadata
- proxies only through closed-world generation
- build-time diagnostics for unsupported features
- optimized for single-purpose Firecracker microVM artifacts

## Non-Goals

- Do not build a fully general JVM before shipping useful service support.
- Do not support arbitrary runtime bytecode generation in the first Graal replacement profile.
- Do not support Java agents, JVMTI, HotSwap, instrumentation, or dynamic attach.
- Do not preserve every HotSpot performance behavior.
- Do not chase all frameworks at once.
- Do not add a large general-purpose runtime when a closed-world intrinsic is enough.
- Do not hide unsupported Java behavior behind runtime crashes.

## Current State

Current `fvm-aot` is a proof of boundary, not a complete compiler.

Implemented capabilities:

- `--backend fvm-aot` bypasses GraalVM Native Image.
- Classfiles are parsed directly in Rust from a closed-world JAR.
- A tiny interpreter/evaluator walks selected bytecode at build time.
- Static fields and `<clinit>` are supported for the current subset.
- App-owned object allocation, constructors, fields, static helpers, instance calls, and interface dispatch work for simple closed-world examples.
- Primitive and reference arrays exist for one-dimensional supported arrays.
- Selected `String`, `Object`, and array methods are implemented as build-time intrinsics.
- `StringConcatFactory` `invokedynamic` concat is evaluated for supported values.
- `fvm.runtime.Http.respond(port, body)` is lowered to generated native C HTTP server code.
- Generated native `/app` binary has been packaged into existing FVM artifacts and booted in Firecracker.

Known limitations:

- No runtime method compilation yet.
- No general allocation during runtime execution.
- No GC.
- No exceptions.
- No real Java threads or synchronization.
- No real Java standard library implementation beyond intrinsics.
- No reflection/proxy/resources model.
- No real socket API beyond the generated HTTP intrinsic.
- No full `String` UTF-16 object model.
- No compatibility with common libraries such as Jackson, SLF4J, Netty, or servlet stacks yet.

## Architecture Principles

### Keep FVM Packaging Backend-Neutral

The artifact contract should stay the same for Graal and `fvm-aot`:

- `/app` native executable
- minimal rootfs
- Firecracker config
- metadata manifest
- optional snapshots
- benchmark records

`fvm-aot` should replace only the compiler/runtime backend, not the Firecracker orchestration.

### Fail at Build Time

Unsupported features should fail during build with precise diagnostics:

- class and method name
- bytecode offset where applicable
- unsupported opcode or runtime feature
- suggested profile or future milestone when known

No unsupported Java feature should silently compile into undefined runtime behavior.

### Closed World First

The compiler should assume the set of reachable classes, methods, resources, reflection targets, and proxies is known at build time.

Closed-world metadata must cover:

- entry points
- service providers
- reflection-accessible classes, constructors, fields, methods
- proxy interfaces
- resources
- class-initialization policy
- JNI/native calls when eventually supported

### Minimal Runtime First

Avoid shipping a general Java runtime when a smaller closed-world implementation is enough.

Examples:

- compile `String` operations directly when values are known
- generate reflection tables only for declared reachable members
- specialize virtual dispatch tables to reachable classes
- strip unused library methods
- generate proxy classes ahead of time

### Differential Correctness

For every supported Java behavior, compare against HotSpot with the same source fixture. If behavior intentionally differs, document it and make the compiler reject code that relies on unsupported semantics.

## High-Level Components

### Frontend

Responsibilities:

- read JARs and directories
- parse classfiles through Java 25 classfile format
- parse constant pool entries, attributes, annotations, signatures, records, nestmates, bootstrap methods, module info where relevant
- preserve enough metadata for reachability, diagnostics, and runtime tables

Required upgrades:

- parse exception tables
- parse line number tables for diagnostics and stack traces
- parse local variable tables when present
- parse annotations
- parse inner/nest classes
- parse record attributes
- parse sealed class attributes
- parse runtime-visible parameter annotations
- parse module metadata enough to reject unsupported module behavior cleanly

### Closed-World Analyzer

Responsibilities:

- start from entry points
- walk direct method calls
- resolve virtual/interface dispatch targets
- process class initializers
- process field and method descriptors
- process `invokedynamic` bootstrap methods
- process reflection metadata
- process ServiceLoader metadata
- process resource metadata
- determine runtime vs build-time class initialization
- emit a reachable graph for codegen

The analyzer should output:

- reachable classes
- reachable methods
- reachable fields
- vtable/itable requirements
- runtime metadata requirements
- allocated object layouts
- required intrinsics
- rejected features with source locations

### Intermediate Representation

The current evaluator should be replaced by an IR pipeline.

Minimum viable IR:

- basic blocks
- typed values
- locals and operand stack lowering
- branches
- calls
- field loads/stores
- array loads/stores
- allocation nodes
- null checks
- bounds checks
- exception edges
- runtime helper calls

IR passes:

- stack-to-SSA or stack-to-register lowering
- constant folding
- null/bounds check insertion
- virtual dispatch lowering
- intrinsic replacement
- dead code elimination
- simple inlining for small methods
- escape-analysis later if needed

### Code Generation

Recommended path:

1. Keep C emission only for generated runtime stubs and smoke tests.
2. Move real Java method compilation to Cranelift or LLVM.
3. Emit one native executable linked with the FVM runtime.

Cranelift advantages:

- Rust-native integration
- fast compile times
- enough optimization for server startup path
- direct control over calling conventions and runtime helper calls

LLVM advantages:

- better optimization maturity
- more backend features
- slower and more complex integration

Initial recommendation:

```text
Cranelift first, LLVM only if benchmarks prove a need.
```

Required codegen features:

- Java integer arithmetic semantics
- Java reference null checks
- array bounds checks
- runtime call ABI
- static field storage
- object field offsets
- vtable and itable dispatch
- exception throw paths
- safepoint hooks if GC/threading requires them
- debug symbols enough for FVM diagnostics

### Runtime

Runtime responsibilities:

- process startup
- heap allocation
- object headers
- class metadata
- arrays
- strings
- static fields
- class initialization
- virtual/interface dispatch metadata
- exceptions
- monitors and synchronization
- threads or explicit single-thread policy
- I/O, files, env, time, DNS, sockets, TLS
- shutdown hooks if supported

The runtime should be split into small modules so profiles can exclude unused features.

## Object Model

### Object Header

Minimum header fields:

- class metadata pointer or compact class id
- monitor/lock state or pointer when synchronization is enabled
- GC mark/state bits if GC exists
- identity hash code policy

Early profile can use:

```text
header = class_id + flags + identity_hash
```

Later profile can compress headers if benchmarks demand it.

### Class Metadata

Class metadata must represent:

- internal class name
- superclass
- interfaces
- field layout
- method table
- interface table
- component type for arrays
- class initialization state
- reflection metadata if declared reachable
- source file and line metadata if stack traces are enabled

### Field Layout

Requirements:

- instance fields from superclasses first
- primitive alignment policy
- reference field map for GC
- static fields in per-class storage
- final fields initialized correctly

Out of scope at first:

- exact HotSpot layout
- unsafe raw field offsets unless explicitly supported later

### Dispatch

Virtual dispatch:

- build vtables for reachable classes
- resolve `invokevirtual` to direct call when target is monomorphic
- use vtable slot when polymorphic

Interface dispatch:

- build itables for reachable classes
- resolve monomorphic interface calls directly when possible
- support default methods before framework work

Special calls:

- `invokespecial` for constructors, private methods, super calls
- `invokestatic` for static methods and intrinsics

## Type System

Required primitive support:

- `boolean`
- `byte`
- `char`
- `short`
- `int`
- `long`
- `float`
- `double`
- `void`

Current subset is int-compatible primitives only. Full Graal replacement needs `long`, `float`, and `double` because normal Java libraries use them heavily.

Required reference support:

- object references
- null
- arrays
- strings
- class objects
- interfaces
- boxed primitives
- enums
- records
- annotations metadata where needed

## Arrays

Required arrays:

- all primitive arrays
- object arrays
- string arrays
- multidimensional arrays as arrays of arrays

Required operations:

- allocation
- length
- load/store
- bounds checks
- covariance checks for object arrays
- clone
- identity equals/hashCode/toString
- `System.arraycopy`
- array reflection metadata when declared reachable

Current one-dimensional limitation should be removed before library support work.

## String Model

The current implementation stores strings as UTF-8 bytes for build-time values. Full Java compatibility requires deliberate handling of Java UTF-16 semantics.

Options:

- store strings as UTF-16 internally
- store compact strings like modern JDKs with Latin-1/UTF-16 flag
- store UTF-8 and translate at API boundaries

Recommended path:

```text
Use a simple UTF-16 representation first for correctness, optimize compact strings later.
```

Required `String` support:

- constructors used by libraries
- `length`
- `isEmpty`
- `charAt`
- `equals`
- `hashCode`
- `toString`
- `substring`
- `indexOf`
- `lastIndexOf`
- `startsWith`
- `endsWith`
- `contains`
- `compareTo`
- `concat`
- `replace`
- `trim`/`strip`
- `getBytes`
- charset conversions for UTF-8
- `StringBuilder`
- `StringBuffer` if needed
- javac string concat through `StringConcatFactory`

## Exceptions

Exceptions are mandatory for real Java libraries.

Required semantics:

- `athrow`
- exception tables
- catch matching by class hierarchy
- `try/catch/finally`
- implicit exceptions for null checks, bounds checks, arithmetic divide by zero, class cast failures
- stack trace capture policy
- `Throwable`, `Exception`, `RuntimeException`, common subclasses

Implementation path:

1. Add classfile exception table parsing.
2. Add IR exception edges.
3. Add runtime throw helper.
4. Add landing pads or setjmp/longjmp-style first implementation.
5. Add stack trace metadata after correctness works.

Early profile can support stack traces with method names and line numbers only. Full Java stack trace fidelity can come later.

## Memory Management

### Phase 1: Bump/Region Allocation

Use a simple bump allocator for short-lived startup and benchmark fixtures.

Useful for:

- proving runtime compilation
- object allocation tests
- simple request handlers with bounded allocations

Limitations:

- no long-running workloads with unbounded allocation
- no general library compatibility

### Phase 2: Stop-The-World Mark-Sweep or Mark-Compact GC

Add a simple GC before claiming real server support.

Requirements:

- root scanning for stacks, statics, thread locals
- object reference maps
- array reference maps
- class metadata reference maps
- safe allocation slow path
- deterministic failure on OOM

Recommended first GC:

```text
Stop-the-world mark-sweep, single-threaded initially.
```

### Phase 3: Generational or Region GC

Only add if benchmarks demand it.

Possible triggers:

- high allocation-rate HTTP frameworks
- JSON serialization pressure
- poor p99 latency
- unacceptable steady-state RSS

### Escape Analysis

Defer until runtime correctness exists.

Later benefits:

- scalar replacement
- stack allocation
- reduced GC pressure
- smaller heap for Firecracker guests

## Threads and Synchronization

Threading is a product choice.

Early possible profiles:

- single-threaded service loop
- fixed native worker threads without full Java `Thread`
- full Java threads

For broad Graal replacement, Java threading is required.

Required features:

- `java.lang.Thread`
- thread locals
- interrupts
- daemon flag behavior
- `synchronized`
- object monitors
- `wait`/`notify`/`notifyAll`
- `volatile`
- atomics and VarHandle subset
- Java Memory Model correctness for supported concurrent code
- `java.util.concurrent` primitives

Implementation path:

1. Explicitly reject Java thread creation outside the chosen profile.
2. Support `volatile` field accesses as compiler barriers/atomic loads/stores.
3. Add native thread mapping.
4. Add monitors.
5. Add park/unpark.
6. Add concurrent collections and executor compatibility.

## Class Initialization

Class initialization is one of the biggest Graal compatibility issues.

Needed policy:

- build-time initialization when safe and requested
- runtime initialization when side effects depend on runtime environment
- deterministic ordering
- cycle handling
- initialization locks if threads exist
- diagnostics for unsupported side effects

Metadata should allow:

```text
--initialize-at-build-time=...
--initialize-at-run-time=...
```

or FVM-specific equivalents in artifact metadata.

Build-time initialization must be sandboxed and audited. Code that reads time, environment, files, network, randomness, system properties, or native state must either be rejected or forced to runtime initialization.

## Reflection

Reflection is required for JSON libraries, frameworks, dependency injection, and configuration.

Closed-world reflection model:

- no arbitrary discovery of unreachable members
- reflection metadata generated only for declared reachable classes and members
- constructors, fields, methods, annotations available when configured
- inaccessible member behavior should match Java when supported
- unsupported deep reflection should fail at build time

Required APIs:

- `Class.forName` for known classes
- `Class` metadata methods
- `getName`, `getSimpleName`, `getPackageName`
- `isAssignableFrom`, `isInstance`
- `getDeclaredConstructors`, `getDeclaredFields`, `getDeclaredMethods`
- constructor invocation
- method invocation
- field get/set
- annotations lookup

Implementation path:

1. Add class object model.
2. Generate compact metadata tables.
3. Support read-only metadata queries.
4. Add reflective constructor/method invocation through generated trampolines.
5. Add field access through generated offset metadata.
6. Add annotation parsing and runtime representation.

## Dynamic Proxies and Lambdas

### Lambdas

Javac lambdas use `invokedynamic` and `LambdaMetafactory`.

Required support:

- parse bootstrap methods
- resolve target method handles
- generate synthetic closure classes or direct function objects
- support captured values
- support common SAM interfaces

### Dynamic Proxies

Frameworks use `java.lang.reflect.Proxy` heavily.

Closed-world approach:

- require proxy interface sets at build time
- generate proxy classes ahead of time
- dispatch through invocation handler
- reject runtime-unknown proxy interface combinations

## Method Handles and VarHandles

Method handles appear in lambdas, string concat, reflection, and frameworks.

Minimum support:

- method handle constants
- direct static/virtual/special handles
- bound handles for lambda captures
- invoke/invokeExact for generated known shapes

VarHandle support is needed for concurrent libraries and newer JDK internals.

Minimum VarHandle support:

- field get/set
- volatile get/set
- compareAndSet
- getAndSet or common atomic operations as needed

## Java Standard Library Coverage

The correct approach is not to reimplement the whole JDK by hand immediately. Options:

- compile selected OpenJDK class library classes into the closed-world binary
- provide FVM-native substitutions for VM-sensitive classes
- implement intrinsics for hot primitives
- reject unsupported modules/classes clearly

Recommended path:

```text
Reuse Java library source/bytecode where possible, replace VM/native internals with FVM substitutions.
```

Priority packages:

1. `java.lang`
2. `java.util`
3. `java.time`
4. `java.io`
5. `java.nio`
6. `java.net`
7. `java.security`
8. `javax.net.ssl`
9. `java.util.concurrent`
10. logging and service-loading support

### java.lang

Required:

- `Object`
- `Class`
- `String`
- `StringBuilder`
- `Throwable` hierarchy
- boxed primitives
- `Enum`
- `System`
- `Math`
- `Thread`
- `ThreadLocal`
- `StackTraceElement`

### java.util

Required:

- `Objects`
- collections: `ArrayList`, `HashMap`, `HashSet`, `LinkedHashMap`, `Collections`, `Arrays`
- iterators
- optional
- properties
- UUID
- regex eventually
- ServiceLoader

### java.time

Required for server apps and logging:

- `Instant`
- `Duration`
- `LocalDate`, `LocalDateTime`, `OffsetDateTime`, `ZonedDateTime`
- formatters commonly used by logging/config

### java.io and java.nio

Required:

- streams
- readers/writers
- files and paths
- buffers
- charset UTF-8
- basic file attributes
- resource reading from artifact-bundled resources

### java.net and TLS

Required:

- sockets
- server sockets
- URI/URL parsing
- DNS resolution
- HTTP client only if target apps need it
- TLS sockets
- certificate loading
- secure random

TLS is a major milestone because it touches security, native crypto, certificates, randomness, and sockets.

## Native and System Runtime

Required host/guest services:

- environment variables
- command-line args
- system properties
- current time and monotonic time
- sleep/park
- filesystem access for declared writable/readable paths
- resource loading from embedded resources
- DNS
- TCP sockets
- TLS
- randomness
- stdout/stderr
- process exit
- signal handling

Unsupported initially:

- process spawning
- arbitrary dynamic library loading
- JNI
- attach APIs
- management/JMX

## HTTP Runtime Path

Current `fvm.runtime.Http.respond` is useful for proof of boot speed but not enough for real apps.

Incremental path:

1. Keep `Http.respond` as minimal benchmark intrinsic.
2. Add FVM-native HTTP server API with request/response handling.
3. Add Java socket APIs.
4. Run a small Java HTTP framework built on sockets.
5. Run Netty or a minimal subset only when NIO and concurrency are ready.

## Framework Compatibility Plan

### Plain Java

Goal:

- manually written HTTP services
- no reflection-heavy framework
- simple config, JSON, logging

### Micronaut or Quarkus

Why next:

- already friendly to ahead-of-time analysis
- lower reflection burden than Spring
- good stepping stone for dependency injection and HTTP routing

Needs:

- annotations
- generated metadata
- reflection metadata
- resources
- JSON
- logging
- HTTP server stack

### Spring

Why later:

- reflection-heavy
- proxies
- resource scanning
- dynamic classpath conventions
- extensive `java.beans`, annotations, and configuration machinery

Needs before attempting:

- reflection metadata mature
- dynamic proxies mature
- class initialization controls
- resource scanner compatibility
- common Spring Native/Graal metadata ingestion

## Metadata and Configuration

FVM should define its own metadata format but ingest Graal-style metadata where useful.

Inputs:

- reflection config
- resources config
- proxy config
- serialization config
- class initialization config
- substitutions/intrinsics config

Outputs:

- reachability report
- unsupported feature report
- generated runtime metadata
- binary size contribution report
- benchmark report

This helps users migrate from Graal Native Image without rewriting all metadata.

## Security Model

Security goals:

- no runtime code loading by default
- no agents/instrumentation
- minimal guest rootfs
- read-only artifact by default
- explicit resource and filesystem access
- explicit network behavior
- no hidden reflection reachability

Runtime concerns:

- memory safety in Rust runtime code
- generated native code correctness
- bounds checks
- null checks
- integer semantics
- TLS correctness
- certificate handling
- random number source

## Benchmark Targets

Use Graal as both baseline and competitor.

Targets for supported services:

- cold boot faster than raw Graal native process startup
- Firecracker cold boot materially faster than Graal-backed FVM cold boot
- app binary smaller than Graal native binary
- rootfs smaller than Graal-backed rootfs
- host RSS lower than Graal-backed FVM cold run
- guest RSS lower than Graal-backed native app
- snapshot restore remains single-digit milliseconds
- compile time lower than Graal native-image for supported apps

Current benchmark anchor:

```text
fvm-aot aot-http dispatch/concat artifact:
  median: 142 ms
  p90: 182 ms
  p99: 182 ms
  host RSS max: 37.28 MiB
  app binary: 16 KiB
```

Do not claim broad superiority until real runtime-compiled apps and at least one common library stack are included.

## Milestones

### Milestone 0: Stabilize Current AOT Slice

Goal:

- keep the current build-time evaluator robust while the compiler path is developed

Deliverables:

- clear unsupported diagnostics
- docs and compatibility matrix
- benchmark fixtures checked into examples
- no regression in current `aot-http` benchmark

Acceptance criteria:

- local and Linux/KVM validation pass
- Firecracker smoke passes on a Linux/KVM host
- current AOT benchmark does not regress materially without explanation

### Milestone 1: Runtime Method Compiler Skeleton

Goal:

- compile simple Java methods into executable native code instead of evaluating everything at build time

Deliverables:

- bytecode-to-IR lowering
- Cranelift or equivalent backend selected
- runtime call ABI
- compiled static methods
- compiled integer arithmetic and branches
- compiled direct calls
- executable `/app` still packaged by FVM

Acceptance criteria:

- simple `println` and HTTP intrinsic fixtures compile through new compiler path
- generated native code executes outside Firecracker
- Firecracker smoke passes
- current evaluator remains available only as fallback for build-time constants or is removed deliberately

### Milestone 2: Runtime Object Allocation

Goal:

- support runtime object and array allocation with a minimal heap

Deliverables:

- object header
- class metadata table
- field layout
- static field storage
- primitive and reference arrays
- string object representation
- bump allocator

Acceptance criteria:

- object/array tests execute at runtime, not only at build time
- null checks and bounds checks match HotSpot on fixtures
- deterministic OOM behavior

### Milestone 3: Full Primitive and Core Bytecode Coverage

Goal:

- support normal javac output for primitive-heavy code

Deliverables:

- `long`, `float`, `double`
- conversions

- comparisons
- switch bytecodes
- all primitive arrays
- `instanceof`, `checkcast`
- stack manipulation opcodes used by javac

Acceptance criteria:

- differential bytecode fixture suite passes against HotSpot
- common `java.lang.Math` operations for supported primitives work

### Milestone 4: Dispatch, Inheritance, Interfaces, and Lambdas

Goal:

- compile object-oriented Java normally emitted by javac

Deliverables:

- superclass field/method layout
- vtables
- itables
- default methods
- lambda support through `LambdaMetafactory`
- method references for common shapes

Acceptance criteria:

- interface-heavy fixture suite passes
- lambdas and method references pass for common SAM interfaces
- monomorphic dispatch optimization works where obvious

### Milestone 5: Exceptions

Goal:

- support real Java error handling

Deliverables:

- exception table parsing

- throw helper
- catch matching
- implicit exceptions
- basic stack traces
- core `Throwable` hierarchy

Acceptance criteria:

- try/catch/finally differential tests pass
- null, bounds, class cast, divide-by-zero behavior matches supported profile
- stack traces identify method names and line numbers when debug metadata exists

### Milestone 6: Garbage Collection

Goal:

- support long-running services with dynamic allocation

Deliverables:

- reference maps
- stack maps or conservative root scan policy
- static roots
- stop-the-world mark-sweep GC
- allocation slow path
- GC metrics

Acceptance criteria:

- allocation stress tests pass
- long-running HTTP fixture handles repeated requests without unbounded RSS growth
- GC pauses are measured and reported

### Milestone 7: Core Java Library Profile

Goal:

- support enough JDK APIs for simple real apps

Deliverables:

- `java.lang` essentials
- collections
- time
- basic I/O
- resources
- UTF-8 charset
- system properties/env

Acceptance criteria:

- collection/time/string differential tests pass
- simple config and resource-loading app works
- logging facade minimal example works

### Milestone 8: Reflection and Metadata

Goal:

- support libraries that inspect application classes

Deliverables:

- `Class` objects
- reflection metadata tables
- field/method/constructor lookup
- annotation metadata
- reflective invocation through generated trampolines
- metadata configuration ingestion

Acceptance criteria:

- reflection fixture suite passes
- simple JSON binding by reflection works
- unsupported reflection fails at build time with exact missing metadata

### Milestone 9: Sockets and HTTP Runtime

Goal:

- move beyond `Http.respond` intrinsic

Deliverables:

- TCP socket API
- server socket
- DNS
- blocking I/O
- minimal Java HTTP server or FVM Java HTTP API
- resource-safe shutdown

Acceptance criteria:

- Java HTTP server fixture handles multiple requests
- Firecracker readiness uses real app socket path
- current benchmark remains competitive

### Milestone 10: Concurrency Profile

Goal:

- support common server concurrency primitives

Deliverables:

- Java threads or constrained worker model
- thread locals
- monitors
- volatile
- atomics
- park/unpark
- executors subset

Acceptance criteria:

- `java.util.concurrent` fixture subset passes
- simple multithreaded HTTP fixture works
- memory model tests for supported primitives pass

### Milestone 11: JSON, Logging, and ServiceLoader

Goal:

- support typical non-framework service dependencies

Deliverables:

- ServiceLoader
- resources
- reflection metadata for JSON
- Jackson or a smaller JSON library first
- SLF4J/simple logging path

Acceptance criteria:

- JSON request/response fixture works
- logging fixture works
- service provider lookup works

### Milestone 12: TLS and Security APIs

Goal:

- support HTTPS clients/servers and secure apps

Deliverables:

- secure random
- cert loading
- TLS sockets or OpenSSL/rustls-backed substitution
- basic `java.security` APIs required by TLS and frameworks

Acceptance criteria:

- HTTPS client fixture works
- HTTPS server fixture works if in scope
- certificate failure behavior is tested

### Milestone 13: Micronaut or Quarkus First Framework

Goal:

- prove framework compatibility with an AOT-friendly framework

Deliverables:

- annotation processing metadata support
- reflection/proxy/resources support for selected framework
- HTTP routing
- JSON
- config
- logging

Acceptance criteria:

- one real framework hello-world app builds and boots in Firecracker
- benchmark beats Graal-backed FVM cold boot for same app
- unsupported framework features documented

### Milestone 14: Spring Investigation

Goal:

- determine exact gap to Spring support

Deliverables:

- Spring sample analysis
- metadata ingestion plan
- proxy/reflection/resource gaps
- class initialization gaps
- benchmark estimate

Acceptance criteria:

- written gap report
- at least one tiny Spring-style fixture either runs or fails with complete feature list

### Milestone 15: Production Hardening

Goal:

- make `fvm-aot` dependable for selected services

Deliverables:

- stable metadata format
- reproducible builds
- deterministic diagnostics
- CI perf gates
- crash reporting
- runtime metrics
- security review
- compatibility versioning

Acceptance criteria:

- supported profile is documented and tested
- all unsupported profile escapes have build-time diagnostics
- benchmark history is tracked
- examples are reproducible from clean checkout

## Risks

### Java Library Surface Explosion

Mitigation:

- profile-based support
- reuse OpenJDK bytecode where possible
- substitutions for VM-sensitive classes
- explicit framework target ordering

### GC and Threads Complexity

Mitigation:

- single-threaded/bump allocator profiles first
- stop-the-world GC before low-latency GC
- add concurrency only when required by target apps

### Reflection Compatibility

Mitigation:

- ingest Graal metadata formats
- generate compact metadata tables
- require explicit missing metadata diagnostics

### Native TLS and Security

Mitigation:

- defer broad TLS until sockets/files/resources are stable
- use proven libraries through substitutions where possible
- test against known certificate chains and failure modes

### Scope Creep Toward Full JVM

Mitigation:

- maintain explicit compatibility profiles
- reject non-profile features
- benchmark every expansion against payload/RSS/startup goals

## Definition of Graal Replacement

`fvm-aot` can be called a Graal replacement for a workload class when all are true:

- the app builds without GraalVM Native Image
- unsupported features fail at build time
- the app boots and serves traffic in Firecracker
- correctness tests pass against HotSpot for the supported behavior
- cold boot beats the Graal-backed FVM artifact for the same app
- app binary and rootfs are materially smaller than the Graal-backed artifact
- host and guest RSS are competitive or better
- snapshots still restore in single-digit milliseconds
- documentation states the exact supported profile

Anything less should be described as experimental `fvm-aot`, not broad Graal replacement.
