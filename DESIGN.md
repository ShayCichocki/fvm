# FVM Design

FVM is a Firecracker-native deployment toolchain for Java applications. The goal is to make Java services fast to start, memory tight, and cheap to run by compiling them into native microVM artifacts instead of shipping a general-purpose JVM inside a container.

## Core Idea

Traditional Java deployment usually looks like this:

```text
container runtime -> Linux userspace -> JVM -> application bytecode
```

FVM targets this shape instead:

```text
Firecracker -> minimal kernel -> native application binary
```

The application is still written in Java, but the deployed artifact should not require a HotSpot JVM process. Java is treated as an input language and compiled ahead of time into a closed-world native executable.

Near term, FVM uses GraalVM Native Image because it gives a working Java-to-native baseline today. Long term, the sharper goal is stricter: Java in, Firecracker-native artifact out, with no HotSpot JVM and no Graal/SubstrateVM runtime dependency in the guest payload.

## Goals

- Run Java applications with a much lower memory floor than conventional JVM containers.
- Start applications quickly from cold boot.
- Restore initialized applications even faster using Firecracker snapshots.
- Preserve a migration path for existing Java services and frameworks.
- Package each service as a single-purpose microVM artifact.
- Avoid rebuilding Firecracker or a JVM unless benchmarks prove it is necessary.
- Optimize first for Linux hosts with KVM and Firecracker.
- Grow toward an FVM-owned Java AOT backend that can replace Graal for constrained workloads.

## Non-Goals

- Do not build a full JVM from scratch in the first version.
- Do not attempt full Java compatibility in the first FVM-owned compiler/runtime backend.
- Do not support every dynamic Java feature in native mode on day one.
- Do not run Docker inside the microVM.
- Do not include a full Linux distribution in the guest image.
- Do not optimize for arbitrary guest operating systems.
- Do not prioritize Kubernetes integration before the core runtime works.

Docker may be used as a host-side packaging or build wrapper, but only when the container has the Linux host privileges Firecracker needs. This is different from running Docker inside the guest, which remains out of scope.

## MVP Contract

The first version should be a narrow vertical slice that ships, not a broad platform preview.

Initial constraints:

- Target the newest generally available Java release that is supported by GraalVM Native Image, starting with Java 25 / GraalVM 25.0.2.
- Use GraalVM Native Image or a compatible native-image compiler that supports the selected Java release.
- Support Linux x86_64 hosts with KVM.
- Use upstream Firecracker without rebuilding it.
- Support one plain Java HTTP service packaged as a JAR.
- Support one exposed TCP port.
- Build one read-only root filesystem artifact.
- Run the application as PID 1 unless a tiny init wrapper proves necessary.
- Emit repeatable benchmark output from the first runnable build.

If the native-image compiler cannot support the newest Java release yet, FVM should fail with a clear diagnostic instead of silently downgrading the application target.

Everything outside this contract is secondary until the first service builds, boots, listens, and produces benchmark data.

## System Architecture

FVM has four core commands in the near-term product. The first ship milestone should include `build`, `run`, and `inspect`; `snapshot` follows once cold boot works end to end.

```text
fvm build
  -> inspect the JAR
  -> compile a native binary
  -> assemble rootfs
  -> write artifact metadata

fvm run
  -> validate artifact
  -> prepare networking
  -> generate Firecracker config
  -> launch VM
  -> stream logs and readiness status

fvm snapshot
  -> boot artifact
  -> wait for readiness
  -> pause VM
  -> write snapshot files and metadata

fvm inspect
  -> print artifact inputs, sizes, hashes, defaults, and benchmark data
```

The implementation should keep build-time and run-time responsibilities separate. Build commands produce immutable artifacts. Run commands create host resources such as TAP devices, API sockets, logs, temporary configs, and cleanup state.

## Runtime Shape

The ideal runtime is intentionally minimal:

```text
Firecracker VMM
  -> tiny Linux kernel
  -> init = /app
  -> native Java-compiled binary
```

The guest should contain no systemd, package manager, shell, SSH daemon, logging daemon, or container runtime. The application should run as PID 1 unless there is a concrete need for a tiny init wrapper.

## Build Pipeline

FVM takes a Java application artifact and produces a Firecracker-ready microVM artifact.

```text
Java app / JAR
  -> dependency and framework analysis
  -> native-image metadata generation
  -> ahead-of-time compilation
  -> binary optimization
  -> rootfs assembly
  -> kernel and Firecracker config selection
  -> optional boot snapshot
  -> .fvm artifact
```

The first implementation should use GraalVM Native Image or a compatible native-image compiler. FVM should own the packaging, framework detection, metadata generation, Firecracker configuration, and benchmark workflow.

The compiler boundary must stay explicit. GraalVM Native Image is the first backend, not the product identity. FVM artifacts should be structured so a future `fvm-aot` backend can produce the same `/app` contract without changing the Firecracker packaging, networking, snapshot, or benchmark layers.

## Toolchain and Build Environment

The first shipping path is intentionally direct and local. FVM should run on a Linux x86_64 host with KVM and require the host to have the needed build and runtime tools installed or explicitly configured.

Required toolchain for the first version:

- Java 25 JDK from GraalVM 25.0.2.
- `native-image` compatible with the selected Java release.
- Firecracker.
- Linux kernel image selected by FVM.
- ext4 image creation tooling.
- host networking permissions for TAP/NAT setup.

FVM should validate tool versions before a build or run starts and record those versions in the artifact metadata. Missing tools should produce actionable errors with the exact executable or permission that is missing.

Containerized, remote, and cross-architecture builds can come later. The first version should optimize for the shortest path to a working Linux/KVM developer and benchmark loop.

Docker execution is allowed for the host toolchain if the container runs on a Linux host with `/dev/kvm`, `/dev/net/tun`, `CAP_NET_ADMIN`, and enough cgroup access. Docker Desktop on macOS should be treated as build/dry-run only unless it can expose nested KVM correctly.

## Artifact Layout

A built artifact may look like this:

```text
my-service.fvm/
  kernel
  rootfs.ext4
  firecracker.json
  metadata.json
  snapshots/
    initialized.mem
    initialized.vmstate
```

The root filesystem should be read-only by default. Writable paths should be explicit and backed by tmpfs or a configured block device.

`metadata.json` is the stable artifact manifest. It should include at least:

- artifact schema version
- FVM version
- application name and version
- Java target version
- native-image compiler name and version
- target architecture and operating system
- build timestamp
- input JAR path, size, and hash
- app binary path, size, and hash
- kernel path, size, and hash
- rootfs path, size, and hash
- default memory size and vCPU count
- exposed guest ports
- rootfs mount mode
- snapshot files and compatibility data when present
- benchmark results when present

All file paths inside the manifest should be relative to the `.fvm/` artifact directory. Artifacts should be movable between compatible hosts without rewriting metadata.

## Rootfs and Kernel Strategy

The first rootfs format should be ext4 because it is simple, debuggable, and directly supported by Firecracker as a virtio-block device. The rootfs should be mounted read-only by default.

The initial rootfs should contain only what the application needs:

- `/app` native executable
- required dynamic linker and shared libraries if the binary is not fully static
- minimal `/dev` entries required for normal process execution
- `/proc` and `/sys` mount points when required by the app or runtime
- `/tmp` as tmpfs only when requested or required
- `/etc/resolv.conf` only when DNS is needed
- CA certificates only when outbound TLS is needed

The kernel should be a small Linux kernel configured for Firecracker with only required virtio devices enabled. Kernel modules should be avoided in the first version.

The boot command line should be explicit and generated by FVM. Native artifacts currently boot through a tiny init wrapper by default because Graal native executables need minimal guest filesystems such as `/proc` to be mounted before the application starts:

```text
init=/init root=/dev/vda ro console=ttyS0 quiet loglevel=0 reboot=k panic=1 pci=off
```

Direct `init=/app` is supported only when the app/runtime does not need those mounts. `--init-mode exec` keeps the tiny init only for mount setup and then replaces it with the app. Monitor mode remains the default because it also emits guest RSS metrics over serial.

## Networking Strategy

The first network model should expose one TCP service using a host TAP device with NAT or host port forwarding managed by FVM.

Initial behavior:

- the guest receives one IPv4 address from FVM-managed configuration
- the service listens on one guest TCP port
- the host maps one local TCP port to that guest port
- readiness is checked from the host against the guest endpoint when a TAP exists, avoiding host port-forwarder timing artifacts
- vsock is reserved for future host control paths

The first CLI should make port exposure explicit:

```bash
fvm run app.fvm --port 8080:8080 --memory 64M
```

Advanced networking, multiple NICs, Kubernetes CNI, IPv6, and service mesh integration are out of scope until the core runtime works.

Cold boots install a permanent host neighbor entry for the configured guest MAC to avoid TAP ARP retry stalls. Snapshot restores intentionally skip that static neighbor; restored virtio-net state responds immediately through normal ARP, while early unicast SYNs can trigger multi-second host TCP backoff.

## Firecracker Lifecycle

FVM owns the host-side lifecycle for each run.

For every VM launch, FVM should create or manage:

- a temporary Firecracker API socket
- a generated Firecracker config
- a log file path
- metrics file path when enabled
- TAP device or equivalent host networking resources
- process cleanup state

`fvm run` should stream guest serial output and return a clear non-zero exit when boot, networking, or readiness fails. Shutdown should attempt a graceful guest stop first and then clean up host resources.

The first version may run Firecracker directly. The jailer, cgroups, rate limiters, and stricter host isolation should be added before production multi-tenant use.

## Security Model

FVM should minimize the guest and make host trust boundaries explicit.

Initial security assumptions:

- artifacts are trusted inputs from the local developer or CI system
- the guest rootfs is read-only by default
- writable paths must be declared
- secrets should not be baked into the artifact
- secrets injection is out of scope for the first version
- the app may initially run as guest root if required for boot simplicity

Before production use, FVM should support running the app as a non-root guest user, Firecracker jailer integration, seccomp profiles, cgroup limits, and explicit secrets injection.

## Failure and Diagnostics

FVM should fail loudly and keep diagnostics close to the command that failed.

Required diagnostics:

- missing host tool or permission
- unsupported Java version or unsupported native-image compiler
- native-image compilation failure with preserved compiler logs
- unsupported dynamic Java feature when detected
- rootfs assembly failure with the missing file or dependency
- Firecracker boot failure with serial output
- readiness timeout with guest logs and host networking details
- benchmark failure with partial measurements preserved

`fvm inspect` should help debug failures by showing artifact structure, file hashes, default runtime config, exposed ports, and any recorded build or benchmark errors.

## Execution Modes

### Native Mode

Native mode is the primary target.

```text
Java app -> native executable -> minimal Firecracker microVM
```

This mode removes the HotSpot JVM process from production. It relies on closed-world analysis and ahead-of-time compilation.

The current native backend is GraalVM Native Image. This removes HotSpot, but the resulting binary still carries a general-purpose Java native runtime: object model, GC, class initialization machinery, reflection support when configured, exception machinery, thread/runtime services, and other SubstrateVM support code. That is acceptable for the first working product, but it is not the final form.

### FVM AOT Mode

FVM AOT mode is the long-term zero-JVM backend.

```text
Java bytecode / source subset -> FVM closed-world analysis -> FVM native codegen -> tiny FVM runtime -> Firecracker microVM
```

This mode keeps Firecracker as the isolation boundary but removes both HotSpot and Graal/SubstrateVM from the guest payload. The goal is not to build a general JVM. The goal is to compile a deliberately constrained Java service shape into a small native binary that satisfies the same `/app` runtime contract as native mode.

Initial `fvm-aot` constraints should be aggressive:

- support plain Java HTTP services first
- support Java bytecode input before building a Java source parser
- support a closed-world class graph only
- reject dynamic class loading
- reject agents, JVMTI, JNI, and runtime instrumentation
- reject unconstrained reflection unless declared in build metadata
- support a small, explicit subset of `java.lang`, `java.util`, `java.io`, `java.net`, and HTTP/server APIs
- support single-process execution first
- start with a simple allocator or region model before a production GC
- make unsupported Java features fail at build time with precise diagnostics

The first FVM AOT runtime should own only the semantics needed by the supported subset:

- object layout
- strings and arrays
- static fields and static initialization
- virtual/interface dispatch
- exceptions
- basic synchronization policy or an explicit no-threading restriction
- memory allocation and reclamation
- minimal system calls for env, time, files, sockets, and HTTP readiness

The payoff target is a smaller guest payload, lower guest RSS, faster cold boot, fewer native-image configuration footguns, and a compiler/runtime stack designed specifically for Firecracker deployment instead of general Java native execution.

### Snapshot Native Mode

Snapshot native mode boots the native application once, waits for readiness, then captures a Firecracker snapshot.

```text
restore initialized native microVM -> accept traffic
```

This should provide the fastest practical startup path.

### Legacy Snapshot Mode

Legacy snapshot mode is a fallback for applications that cannot native-compile yet.

```text
trimmed JVM -> application initialized -> Firecracker snapshot
```

This mode does not satisfy the ideal no-JVM target, but it gives existing Java users a migration path while preserving fast restore behavior.

## Closed-World Assumption

Native mode requires knowing the application shape at build time. FVM must discover or generate metadata for:

- reachable classes and methods
- reflection usage
- dynamic proxies
- service loaders
- resources
- JNI usage
- serialization
- framework initialization
- dependency injection graphs
- HTTP routes and handlers

If FVM cannot safely prove compatibility, it should fail with actionable diagnostics or recommend legacy snapshot mode.

For the MVP, metadata handling should stay simple:

- accept user-provided native-image config files when present
- generate only the metadata needed for the supported plain Java HTTP shape
- preserve native-image diagnostics without hiding them behind generic FVM errors
- do not attempt broad framework magic until the plain Java path is reliable

Later framework support should come through explicit analyzers or plugins that generate reflection, resource, proxy, service-loader, and initialization metadata.

## Framework Strategy

FVM should support frameworks incrementally.

Initial target order:

1. Plain Java HTTP service.
2. Micronaut or Quarkus service.
3. Spring Boot native-compatible service.
4. Spring Boot legacy service through snapshot fallback.
5. Servlet or WAR-style legacy applications.

The core product value is making Java frameworks native-image friendly with minimal user configuration.

## Memory Strategy

Memory must be reduced at every layer.

Application/runtime optimizations:

- Use native image instead of a HotSpot JVM process.
- Strip unused classes, methods, metadata, and resources.
- Prefer build-time initialization where safe.
- Use a small GC profile suitable for service workloads.
- Remove unused reflection and proxy metadata.
- Strip production symbols where possible.
- Avoid runtime classpath scanning when build-time indexing is possible.

Guest optimizations:

- Use a minimal kernel config.
- Avoid kernel modules unless required.
- Use a static or minimal dynamically linked application binary.
- Exclude shell, package manager, systemd, SSH, cron, and other distro services.
- Keep the rootfs read-only by default.
- Use tmpfs only where needed.
- Include CA certificates only when outbound TLS is required.

Firecracker optimizations:

- Enable only required devices.
- Prefer vsock for host control paths when possible.
- Use virtio-net only when network exposure is required.
- Use a read-only virtio-block rootfs by default.
- Configure memory explicitly per workload.
- Apply host-side rate limits where appropriate.

## Startup Strategy

FVM should support two startup paths.

Cold boot path:

```text
kernel boot -> /app starts -> readiness endpoint passes
```

Snapshot restore path:

```text
restore VM state -> application is already initialized -> accept traffic
```

Cold boot should be optimized, but snapshot restore is expected to be the fastest path for framework-heavy applications.

## CLI Sketch

The first CLI should stay small. MVP commands:

```bash
fvm build app.jar --mode native --java 25
fvm run app.fvm --port 8080:8080 --memory 64M
fvm inspect app.fvm
```

Snapshot command added after the MVP:

```bash
fvm snapshot app.fvm --wait-http /health
```

Possible modes:

```text
native
snapshot-native
legacy-snapshot
```

## Configuration Model

The first version should work with CLI flags alone. A project config file can be added once repeated flags become annoying.

Likely future config file:

```toml
[app]
name = "my-service"
jar = "build/libs/my-service.jar"
java = 25

[runtime]
memory = "64M"
vcpus = 1
port = "8080:8080"

[readiness]
http = "/health"
timeout = "10s"
```

CLI flags should override config file values. Artifact metadata should record the resolved configuration used for the build.

## MVP

The MVP should prove the core claim with one Java 25 HTTP service and should be treated as one combined ship milestone.

Required capabilities:

- Compile a simple Java HTTP service to a native binary.
- Build a minimal rootfs with the app as PID 1 or a justified tiny init wrapper.
- Boot it in Firecracker.
- Expose one HTTP port.
- Wait for an HTTP readiness check.
- Stream serial logs to the user.
- Clean up host resources after shutdown or failure.
- Measure boot-to-listen time.
- Measure host memory per microVM.
- Measure guest RSS.
- Produce repeatable benchmark output.
- Write a complete `.fvm/` artifact with metadata and hashes.

This is not four phases. The first shippable milestone is complete only when a user can run this loop end to end:

```bash
fvm build app.jar --mode native --java 25
fvm run app.fvm --port 8080:8080 --memory 64M
fvm inspect app.fvm
```

and see a native Java service listening from inside a Firecracker microVM with benchmark data attached to the artifact.

A tiny init or metrics wrapper is acceptable in the MVP if it is required to mount procfs, handle signals correctly, reap child processes, or report guest RSS. Direct `/app` as PID 1 remains the preferred runtime shape when those requirements are satisfied without a wrapper.

Out of scope for the MVP:

- Kubernetes integration.
- Multi-framework support.
- Custom Java compiler/runtime.
- Production orchestration.
- Advanced networking.
- Snapshot support.
- Secrets management.
- Multi-tenant host hardening.

## Benchmarks

FVM should track benchmarks from the first prototype.

Primary metrics:

- cold boot to listening socket
- snapshot restore to listening socket
- host memory per microVM
- guest RSS
- binary size
- rootfs size
- CPU used during startup
- steady-state request latency
- requests per second per vCPU
- maximum microVM density per host

Comparison targets:

```text
Docker + JVM
Docker + GraalVM Native Image
Firecracker + JVM cold boot
Firecracker + JVM snapshot restore
FVM native cold boot
FVM native snapshot restore
FVM AOT cold boot
FVM AOT snapshot restore
```

Benchmark output should be both human-readable and machine-readable. FVM should write benchmark data into `metadata.json` and optionally a separate benchmark JSON file for CI comparison.

Default benchmark methodology:

- run one smoke iteration before measured iterations
- run at least 10 measured iterations by default
- record median, p90, p99, min, and max where useful
- record host CPU model, host memory, kernel version, Firecracker version, Java version, native-image version, FVM version, and artifact hashes
- measure boot-to-listen from Firecracker process start to successful host readiness probe
- measure host memory from the Firecracker process on the host
- measure guest RSS from an in-guest procfs reader or tiny metrics wrapper

Benchmarks should fail the command only when the benchmark process itself fails. Performance regressions should be reported clearly but should not prevent artifact creation unless the user supplies an explicit threshold.

Current measured baseline on a Linux/KVM benchmark host for `examples/perf-http` with GraalVM CE `25.0.2`, Firecracker `v1.14.0`, 64 MiB guest memory, quiet kernel boot args, direct guest readiness, cold-only static neighbor, and 30 measured iterations:

```text
FVM cold boot:       median 478 ms, p90 546 ms, p99 578 ms, host RSS max 59.85 MiB, guest RSS max 23.91 MiB
FVM snapshot restore: median 6 ms, p90 6 ms, p99 7 ms, host RSS max 25.15 MiB, guest RSS max 27.94 MiB
FVM AOT direct /app with closed-world multi-class objects, int/String arrays, interface dispatch, and javac string concat: median 142 ms, p90 182 ms, p99 182 ms, host RSS max 37.28 MiB, app binary 16 KiB
```

Previous cold boot was approximately `1100ms`; the largest measured win came from avoiding TAP ARP retry stalls with a permanent host neighbor entry on cold boots. Snapshot restore regressed to approximately `8290ms` when that static neighbor was applied to restored VMs, so restore now deliberately uses normal ARP.

The first `fvm-aot` slice supports a tiny Java bytecode subset: closed-world multi-class JAR loading for app-owned classes, int-compatible primitive constants/locals/fields for `int`, `boolean`, `char`, `byte`, and `short`, primitive arithmetic and branches, app-owned static helpers, primitive/`String` static fields initialized by `ConstantValue` or `<clinit>`, app-owned object allocation/constructors/instance fields/instance helper calls, closed-world `invokevirtual`/`invokeinterface` dispatch for app-owned classes and interfaces, one-dimensional primitive arrays and supported reference arrays, selected `java.lang.String` and `java.lang.Object` intrinsics, array `clone`/`equals`/`hashCode`/`toString`, javac `invokedynamic` string concatenation through `StringConcatFactory`, `System.out.println(String|int|boolean|char)`, and `fvm.runtime.Http.respond(port, body)` when arguments resolve through that subset. It lowers the HTTP intrinsic to a generated native HTTP server. This is not full Java yet, but it proves the backend boundary and shows the upside of removing Graal/SubstrateVM payloads from supported service shapes.

FVM AOT benchmark targets should be stricter than Graal-backed native mode:

- cold boot should beat raw Graal native process startup for supported apps
- guest RSS should be materially below the Graal-backed native artifact
- app binary size should be materially below the Graal-backed native artifact
- snapshot restore should remain single-digit milliseconds
- unsupported Java features should fail at build time, not leak into runtime surprises

## Roadmap

Milestone 1 is the ship milestone:

- Java 25 plain HTTP JAR to native binary
- minimal read-only ext4 rootfs
- Firecracker boot
- one exposed HTTP port
- readiness wait
- log streaming
- artifact metadata and hashes
- benchmark output with host memory and guest RSS
- cleanup after run

Milestone 2 adds snapshot native mode:

- boot the native artifact
- wait for readiness
- create Firecracker snapshot files
- restore snapshot and verify readiness
- record snapshot compatibility metadata

Milestone 3 adds first framework support:

- Micronaut or Quarkus native-compatible service
- generated native-image metadata
- framework-specific diagnostics

Milestone 4 adds Spring and legacy migration:

- Spring Boot native-compatible service
- legacy JVM snapshot fallback
- explicit migration diagnostics

Milestone 5 adds production hardening:

- jailer integration
- cgroups and resource limits
- non-root guest user
- secrets injection
- stronger host cleanup and supervision

Milestone 6 starts the zero-JVM compiler/runtime track:

- add `--backend graal` and experimental `--backend fvm-aot`
- parse Java bytecode and build a closed-world reachability graph
- support one plain Java HTTP service without GraalVM Native Image
- generate a native `/app` binary through Cranelift, LLVM, or direct codegen
- implement the smallest FVM runtime needed for objects, strings, arrays, dispatch, exceptions, allocation, and sockets
- reject dynamic Java features with precise diagnostics
- package the FVM AOT binary into the existing Firecracker artifact format
- benchmark against raw Graal native, Graal-backed FVM cold boot, and FVM snapshots

## Implementation Principles

- Prefer the smallest correct artifact.
- Fail loudly when native mode cannot safely support an application.
- Keep compatibility fallbacks explicit.
- Avoid hidden background services in the guest.
- Make memory and startup costs visible in every build.
- Use existing proven compiler/runtime technology before building a new JVM replacement.
- Design the compiler/runtime boundary so a future custom runtime can replace native-image later.
- Treat Graal as a backend, not as the identity of FVM.
- Prefer restricted, measurable Java semantics over broad compatibility that recreates a JVM by accident.

## Long-Term Direction

If native-image and snapshotting cannot reach the required memory and startup targets, FVM should grow its own Java AOT/runtime layer. That is a later compiler/runtime project, not the first milestone, but the artifact model should be designed for it now.

The near-term product is a Firecracker-native Java deployment toolchain. The long-term ambition is a specialized Java-to-microVM compilation platform: a constrained Java compiler/runtime stack that emits Firecracker-native artifacts without a JVM, without Graal/SubstrateVM in the payload, and without pretending to support every dynamic Java feature.
