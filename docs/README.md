# FVM AOT Roadmap Docs

This folder tracks the plan for turning `fvm-aot` from a build-time evaluator for tiny closed-world examples into an FVM-owned Java-to-native backend that can replace GraalVM Native Image for constrained server workloads.

The target is not a full general-purpose JVM first. The target is a closed-world Native Image replacement for Java services that fit an explicit compatibility contract.

## Documents

- [`fvm-aot-graal-replacement-plan.md`](fvm-aot-graal-replacement-plan.md): full execution plan, architecture, milestones, risks, and acceptance criteria.
- [`fvm-aot-compatibility-matrix.md`](fvm-aot-compatibility-matrix.md): bytecode, runtime, library, reflection, framework, and system compatibility matrix.
- [`fvm-aot-test-strategy.md`](fvm-aot-test-strategy.md): test strategy for differential correctness, runtime behavior, Firecracker smoke tests, and benchmark gates.

## Strategy

FVM should continue using GraalVM Native Image as the broad compatibility backend while `fvm-aot` grows. `fvm-aot` should win by being narrower, smaller, more predictable, and tuned for Firecracker deployment.

The initial promise should be precise:

```text
Java closed-world service in -> small native /app binary -> Firecracker microVM artifact
```

The first serious target is a plain Java HTTP service with common `java.lang`, `java.util`, `java.time`, `java.io`, `java.nio`, `java.net`, logging, JSON, and TLS needs. Framework compatibility should follow in this order: plain Java, minimal embedded HTTP, Micronaut or Quarkus, then Spring only after reflection/proxy/class-initialization support is mature.

## Current Baseline

Current `fvm-aot` already proves the backend boundary:

- no Graal Native Image invocation for `--backend fvm-aot`
- closed-world multi-class JAR loading
- app-owned objects, fields, static initialization, virtual/interface dispatch
- primitive and reference arrays for the supported subset
- selected `String`, `Object`, and array intrinsics
- javac string concat through `StringConcatFactory`
- `System.out.println` for supported values
- `fvm.runtime.Http.respond(port, body)` lowered to a generated native HTTP server
- measured Firecracker cold boot for the current AOT HTTP example: median `142ms`, p90 `182ms`, p99 `182ms`, host RSS max `37.28 MiB`, app binary `16 KiB`

This is not yet a runtime compiler. Much of the current behavior is build-time evaluation. The next major step is compiling executable runtime methods and adding a real object/runtime model.

## Compatibility Terms

- **Supported**: implemented, tested, and expected to work for committed fixtures.
- **Partial**: implemented for narrow cases; unsupported cases must fail at build time.
- **Planned**: required for the Graal replacement goal but not implemented yet.
- **Rejected for now**: intentionally out of scope until a concrete app requires it.
- **Non-goal**: not part of the closed-world service target.
