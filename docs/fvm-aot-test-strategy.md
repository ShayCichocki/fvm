# FVM AOT Test Strategy

`fvm-aot` needs a test strategy that proves Java correctness, compiler correctness, runtime stability, and Firecracker performance. Unit tests alone are not enough. Every supported Java behavior should be compared against HotSpot, exercised in generated native code, and smoke-tested in a microVM when it affects deployment behavior.

## Goals

- Prevent regressions in the current AOT subset.
- Prove each new bytecode/runtime feature against HotSpot behavior.
- Keep unsupported Java features failing at build time with clear diagnostics.
- Measure startup, RSS, binary size, rootfs size, and request readiness continuously.
- Avoid broad compatibility claims without executable evidence.

## Test Pyramid

```text
Compatibility sample apps and benchmarks
Firecracker smoke tests
Native executable runtime tests
Differential Java fixture tests
Compiler IR/codegen tests
Classfile parser and analyzer unit tests
```

## Test Categories

### Parser Tests

Purpose:

- prove classfile parsing correctness
- reject malformed inputs cleanly
- preserve metadata needed by later stages

Coverage:

- Java classfile versions through Java 25
- constant pool entries
- fields and methods
- code attributes
- exception tables
- bootstrap methods
- annotations
- records
- inner/nest classes
- line number tables
- unsupported future classfile versions

Acceptance:

- malformed files produce deterministic errors
- parsed fixtures match expected metadata snapshots
- parser does not panic on fuzzed/truncated data

### Closed-World Analyzer Tests

Purpose:

- prove reachability and rejection behavior

Coverage:

- direct calls
- virtual calls
- interface calls
- class initialization
- field reachability
- lambda bootstrap reachability when implemented
- reflection metadata reachability when implemented
- resource reachability
- unsupported dynamic class loading

Acceptance:

- reachable graph snapshots are stable
- missing metadata errors identify the exact feature
- unsupported dynamic behavior fails before codegen

### Bytecode Lowering Tests

Purpose:

- prove bytecode-to-IR lowering before native execution

Coverage:

- operand stack to IR values
- locals
- branches
- switch
- field loads/stores
- array loads/stores
- method calls
- exception edges
- monitor bytecodes when implemented

Acceptance:

- IR snapshots are stable for representative methods
- unsupported bytecodes include class/method/bci diagnostics
- verifier-like checks reject impossible stack/type states

### Differential Fixture Tests

Purpose:

- compare supported Java behavior against HotSpot

Test flow:

```text
javac fixture.java
run on HotSpot -> expected stdout/exit/result
compile with fvm-aot
run native executable -> actual stdout/exit/result
compare
```

Coverage groups:

- primitives
- arithmetic and conversions
- branches and switches
- object fields
- inheritance
- virtual/interface dispatch
- arrays
- strings
- exceptions
- collections
- reflection
- lambdas
- resources
- sockets
- concurrency

Acceptance:

- supported behavior exactly matches HotSpot or documented profile semantics
- unsupported behavior fails at build time, not at runtime

### Runtime Native Executable Tests

Purpose:

- prove generated `/app` works outside Firecracker before VM smoke

Coverage:

- process startup
- stdout/stderr
- args
- environment variables
- allocation and GC
- exceptions
- sockets on localhost
- file/resource access
- TLS when implemented

Acceptance:

- native executable exits with expected code
- memory stress tests complete under configured heap
- runtime diagnostics are deterministic

### Firecracker Smoke Tests

Purpose:

- prove deployment path still works

Coverage:

- artifact build
- `inspect --verify`
- one-shot boot
- readiness probe
- host port forwarding
- direct guest readiness
- guest serial output
- shutdown/cleanup

Acceptance:

- smoke app boots on a Linux/KVM host
- readiness completes within threshold
- no leaked TAP devices, sockets, or Firecracker processes
- artifact hashes verify after run

### Benchmark Tests

Purpose:

- keep AOT honest against Graal and prior runs

Metrics:

- boot-to-listen median/p90/p99
- host RSS max
- guest RSS max when available
- app binary size
- rootfs size
- build time
- request latency for steady-state fixtures
- request throughput for server fixtures

Acceptance:

- current AOT smoke benchmark is recorded before and after major runtime changes
- regressions above agreed thresholds are explained or fixed
- benchmark reports include host and toolchain versions

## Fixture Organization

Recommended layout:

```text
tests/aot-fixtures/
  primitives/
  arrays/
  strings/
  objects/
  dispatch/
  exceptions/
  lambdas/
  reflection/
  collections/
  resources/
  sockets/
  concurrency/
  frameworks/
```

Each fixture should include:

- Java source
- expected stdout or expected HTTP response
- expected failure if unsupported
- optional metadata config
- optional benchmark profile

For now, small javac-backed fixtures can live inside Rust tests. As coverage grows, move them into fixture directories and add a shared harness.

## Differential Harness

The harness should support:

- compile Java source with selected `javac --release`
- package JAR with explicit `Main-Class`
- run HotSpot baseline
- run `fvm-aot` native executable
- compare stdout/stderr/exit code
- compare thrown exception type and message where applicable
- optionally run in Firecracker
- preserve generated files on failure for debugging

Suggested command shape:

```bash
cargo test aot_diff
```

Longer-running variants:

```bash
cargo test aot_firecracker -- --ignored
cargo test aot_frameworks -- --ignored
```

## Golden Diagnostics Tests

Unsupported feature tests are as important as supported feature tests.

Examples:

- dynamic class loading before support
- Java agents
- JNI
- unsupported reflection
- unsupported bytecode
- unsupported classfile version
- missing resource metadata
- unsupported proxy interface set

Acceptance:

- error contains class name
- error contains method name and descriptor when applicable
- error contains bytecode offset when applicable
- error names the unsupported feature
- error suggests the relevant profile/milestone when known

## Correctness Suites By Milestone

### Current AOT Slice

Required tests:

- invalid classfile rejection
- simple println
- computed HTTP intrinsic
- static fields and `<clinit>`
- objects and arrays
- multi-class closed world
- interface dispatch and string concat
- String/Object/array core intrinsics
- golden unsupported diagnostics for exceptions, lambdas, dynamic class loading, primitive gaps, and multidimensional arrays

Current validation commands:

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

Linux/KVM validation:

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
scripts/fvm-docker build-image
```

### Runtime Compiler Skeleton

Add tests for:

- compiled direct static method calls
- compiled branches
- compiled integer arithmetic
- compiled field access
- compiled array access
- mixed build-time constants and runtime values
- native executable disassembly/symbol sanity if useful

Acceptance:

- same fixture can run through HotSpot and compiled native path
- evaluator-only fallback is not hiding compiler gaps unless explicitly marked

### Object Runtime

Add tests for:

- object allocation at runtime
- field default values
- constructor ordering
- superclass fields
- reference equality
- object arrays
- null checks
- array bounds checks
- deterministic identity hash behavior

### Primitive Completeness

Add tests for:

- `long` arithmetic and comparisons
- `float` and `double` arithmetic
- NaN comparison behavior
- conversions and overflow behavior
- bitwise and shift operations
- switch bytecodes

### Exceptions

Add tests for:

- explicit `throw`
- catch by exact class
- catch by superclass
- finally blocks
- implicit `NullPointerException`
- implicit `ArrayIndexOutOfBoundsException`
- implicit `ArithmeticException`
- `ClassCastException`
- stack trace method and line metadata

### GC

Add tests for:

- allocation stress
- object graph retention
- array graph retention
- static roots
- stack roots
- cyclic references
- finalizers should be rejected unless explicitly supported
- OOM behavior

### Reflection

Add tests for:

- class lookup
- constructor lookup/invocation
- field lookup/get/set
- method lookup/invocation
- annotations
- missing metadata failure
- generated metadata size reporting

### Concurrency

Add tests for:

- thread creation if supported
- thread join
- thread local
- synchronized method/block
- wait/notify
- volatile visibility
- atomic compare-and-set
- executor basics

### Networking

Add tests for:

- server socket bind/listen/accept
- client socket connect/read/write
- DNS success/failure
- timeout behavior
- concurrent requests
- graceful shutdown

### Frameworks

Add tests for:

- plain Java HTTP app
- JSON serialization/deserialization
- logging initialization
- ServiceLoader provider lookup
- first Micronaut or Quarkus hello-world
- later Spring minimal app

## Benchmark Matrix

Every serious milestone should benchmark at least:

| Benchmark | Purpose | Required |
|---|---|---:|
| `aot-println` | minimal process path | Yes |
| `aot-http` | current closed-world HTTP path | Yes |
| runtime-objects | allocation/runtime dispatch path | After compiler runtime |
| json-http | JSON dependency path | After reflection/collections |
| framework-http | first framework path | After framework milestone |

Compare against:

- host JVM
- Docker JVM
- raw Graal native process
- Docker Graal native
- Graal-backed FVM cold
- Graal-backed FVM snapshot
- `fvm-aot` cold
- `fvm-aot` snapshot when supported

## Performance Gates

Initial soft gates for supported AOT HTTP examples:

- median cold boot should stay below `200ms` unless runtime semantics explain regression
- p90 should stay below `250ms`
- app binary should stay materially below Graal native binary
- host RSS should stay below Graal-backed FVM cold RSS
- artifact verification must pass

Future framework gates should be per app because dependencies will change payload size and boot behavior.

## CI Strategy

Local CI tier:

- formatting
- unit tests
- clippy
- parser/analyzer/IR tests
- differential tests that do not require KVM

Linux/KVM CI tier:

- release build
- Docker runner image build
- Firecracker smoke tests
- benchmark smoke with low iteration count

Nightly or manual benchmark tier:

- 30+ iteration benchmarks
- baseline comparisons
- framework sample apps
- memory stress tests

## Failure Artifact Capture

On failure, preserve:

- generated Java classes
- JAR
- AOT metadata
- generated IR
- generated native object/source where applicable
- compiler diagnostics
- native executable
- Firecracker logs
- rootfs path
- benchmark JSON

This should be optional by default and automatic under `FVM_KEEP_FAILED_AOT=1` or a future test harness flag.

## Release Criteria For A Supported Profile

A profile can be documented as supported only when:

- compatibility matrix entries are marked Supported or explicitly Rejected for now
- differential tests cover every supported language/runtime feature
- at least one sample app uses the profile end to end
- Firecracker smoke passes on Linux/KVM
- benchmark numbers are recorded
- unsupported features produce golden diagnostics
- docs state limitations clearly

Until then, call it experimental.
