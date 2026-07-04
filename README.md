# FVM

## MASSIVE WARNING

> [!WARNING]
> FVM is experimental infrastructure software. It creates and runs Firecracker microVM artifacts, configures host TAP networking, and may require privileged Linux/KVM or privileged Docker access. Do not run untrusted artifacts, untrusted JARs, or untrusted generated binaries with this tool.
>
> The `fvm-aot` backend is especially experimental. It is not full Java, not a full JVM, and not a broad GraalVM Native Image replacement yet. It currently works only for a deliberately small closed-world Java subset that fails at build time for unsupported bytecode/runtime features. The current `fvm-aot` benchmark numbers are real, but they apply to the checked example shape, not arbitrary Java applications or frameworks.
>
> macOS is build/test/dry-run only. Real `fvm run` and `fvm snapshot` require a Linux x86_64 host with KVM and Firecracker access.

## Summary

FVM is a Firecracker-native deployment toolchain for Java applications. It builds Java services into native microVM artifacts instead of shipping a general-purpose JVM inside a container.

The current implementation is a Rust CLI with real orchestration for the design phases:

- `fvm build` analyzes a JAR, compiles native mode with `native-image`, assembles a minimal ext4 rootfs, writes Firecracker config, and records artifact metadata.
- `fvm run` launches Firecracker on Linux/KVM, configures TAP networking, forwards one or more host ports, waits for HTTP readiness, streams logs, records boot and memory benchmarks, and cleans up host resources.
- `fvm snapshot` boots an artifact, waits for readiness, creates Firecracker memory/vmstate snapshots, optionally verifies restore, and updates metadata.
- `fvm inspect` prints artifact metadata and verifies hashes.
- `fvm math` computes benchmark-derived density and speedup projections.
- `fvm analyze` detects framework shape and native-image compatibility signals.
- `fvm doctor` checks the local host/toolchain.

The long-term `fvm-aot` roadmap, compatibility matrix, and test strategy live in [`docs/`](docs/). The explicit target is to replace GraalVM Native Image for selected closed-world Java services first, not to implement full arbitrary JVM compatibility up front.

## Requirements

Build host and run host for real microVM execution:

- Linux x86_64 with KVM exposed at `/dev/kvm`.
- Firecracker installed as `firecracker` or passed with `--firecracker`.
- Java 25 JDK from GraalVM 25.0.2.
- `native-image` compatible with the selected Java release.
- ext4 tooling: `mkfs.ext4` or `mke2fs`.
- `ip` from iproute2 for TAP setup.
- a Firecracker-compatible Linux kernel image passed with `--kernel` or `FVM_KERNEL`.

This repository can build and test on macOS, but Firecracker run/snapshot commands require Linux/KVM. Use `--dry-run` to exercise artifact generation without host virtualization tools.

## Docker

FVM can run from Docker on a Linux host, but the container must be allowed to use host virtualization and networking devices. Docker Desktop on macOS is not enough for real Firecracker execution because FVM needs Linux KVM access.

Works:

- native-image builds inside a container
- dry-run artifact generation inside a container
- real `fvm run`/`fvm snapshot` inside a privileged Linux container with `/dev/kvm` and `/dev/net/tun`

Does not work:

- unprivileged Docker containers
- Docker Desktop on macOS for real Firecracker/KVM execution
- Docker inside the guest microVM

Example Linux host invocation:

```bash
docker run --rm -it \
  --privileged \
  --device /dev/kvm \
  --device /dev/net/tun \
  --cap-add NET_ADMIN \
  --network host \
  -v "$PWD:/work" \
  -w /work \
  fvm-dev \
  fvm run app.fvm --port 8080:8080 --memory 64M
```

If `--network host` is not used, also publish the host-facing port with Docker, for example `-p 8080:8080`.

`fvm doctor --strict` is the quickest way to verify whether the container has enough access.

This repo includes a convenience wrapper for that setup:

```bash
scripts/fvm-docker build-image
scripts/fvm-docker doctor --strict
scripts/fvm-docker build examples/plain-http/app.jar --kernel /work/.fvm-host-bin/hello-vmlinux.bin
scripts/fvm-docker run examples/plain-http/native.fvm --once --port 18080:8080 --wait-http /health
```

The wrapper copies the host Firecracker binary into `.fvm-host-bin/` before mounting it into the container. This avoids bind-mount restrictions from snap-packaged Docker.

## Current Performance Notes

Latest measurements on a Linux/KVM benchmark host using the generated `examples/perf-http` app, GraalVM CE `25.0.2`, Firecracker `v1.14.0`, a 64 MiB guest, quiet kernel boot args, and privileged Docker runner:

- FVM cold boot: median `478ms`, p90 `546ms`, p99 `578ms`, max host RSS `59.85 MiB`, max guest RSS `23.91 MiB`.
- FVM snapshot restore: median `6ms`, p90 `6ms`, p99 `7ms`, max host RSS `25.15 MiB`, max guest RSS `27.94 MiB`.

Experimental `fvm-aot` measurement on `examples/aot-http`, with no Graal Native Image payload:

- FVM AOT direct `/app` with closed-world multi-class objects, int/String arrays, interface dispatch, and javac string concat: median `142ms`, p90 `182ms`, p99 `182ms`, max host RSS `37.28 MiB`.
- FVM AOT app binary: `16 KiB`.
- FVM AOT artifact disk usage with 32 MiB rootfs and quickstart kernel: `27 MiB`.

These numbers are for the supported `fvm-aot` example shape only. For arbitrary Java services, GraalVM Native Image remains the broad compatibility backend.

The cold path uses a permanent host neighbor entry for the guest MAC to avoid TAP ARP retry stalls. Snapshot restore intentionally skips that static neighbor because restored virtio-net state responds immediately through normal ARP, while early unicast SYNs can trigger multi-second TCP backoff.

Useful performance/debug knobs:

- `--init-mode exec` keeps `fvm-init` only long enough to mount guest filesystems, then `exec`s the app. Monitor mode remains the default because it provides guest RSS metrics.
- `fvm snapshot --stabilize-ms N` waits after readiness before creating a snapshot. The default is `1000`.
- `FVM_TRACE_READINESS=1 scripts/fvm-docker run ...` prints readiness probe timing and errors.

## Benchmark Math

After `fvm run` records benchmark data, use `fvm math` to compute host memory density and baseline comparisons:

```bash
fvm run app.fvm --once --benchmark-iterations 10 --port 18080:8080 --wait-http /health
fvm math app.fvm --host-memory 32G --reserve 2G --baseline-host-rss 256M --baseline-boot-ms 5000
```

The report includes median/p90/p99 boot time, max host RSS, max guest RSS, usable host memory, projected microVM density, and optional speedup/reduction factors against the supplied baseline.

## Build The CLI

```bash
cargo build
```

This produces:

- `target/debug/fvm`
- `target/debug/fvm-init`

Install both binaries next to each other. `fvm-init` is the guest PID 1 wrapper used for proc/sys/tmp mounting, signal handling, legacy JVM launch, and `FVM_GUEST_RSS_KIB=...` serial metrics.

## MVP Flow

```bash
fvm build app.jar --mode native --java 25 --kernel /path/to/vmlinux --port 8080:8080
fvm run app.fvm --port 8080:8080 --memory 64M
fvm inspect app.fvm --verify
```

Dry-run from a non-Linux machine:

```bash
fvm build app.jar --dry-run --force --allow-missing-guest-rss
fvm run app.fvm --dry-run --once
fvm snapshot app.fvm --dry-run --verify-restore
fvm inspect app.fvm --verify
```

## Native Mode

Native mode is the primary path:

```bash
fvm build app.jar \
  --mode native \
  --java 25 \
  --kernel /path/to/vmlinux \
  --port 8080:8080 \
  --readiness-http /health
```

By default FVM uses `native-image -jar app.jar target.fvm/app.bin`. Pass `--main-class` to force classpath mode.

Experimental zero-JVM backend:

```bash
fvm build app.jar \
  --backend fvm-aot \
  --kernel /path/to/vmlinux \
  --port 8080:8080
```

`--backend fvm-aot` does not invoke GraalVM Native Image. It currently supports a deliberately tiny bytecode subset:

- closed-world multi-class JAR loading for app-owned classes
- int-compatible primitive constants and locals, including `int`, `boolean`, `char`, `byte`, and `short` at descriptor boundaries
- primitive arithmetic, primitive/reference branches, casts for `byte`/`char`/`short`, and app-owned static helper calls that can be resolved at build time
- app-owned static primitive/`String` fields initialized by `ConstantValue` or `<clinit>`
- app-owned object allocation, constructors, instance primitive/`String`/object/array fields, and instance helper calls that can be resolved at build time
- closed-world `invokevirtual`/`invokeinterface` dispatch for app-owned classes and interfaces
- one-dimensional primitive arrays and reference arrays for supported references, including `arraylength`, load/store, and array `clone`/`equals`/`hashCode`/`toString`
- selected `java.lang.String` intrinsics: `length`, `isEmpty`, `charAt`, `equals`, `hashCode`, `toString`, `startsWith`, `endsWith`, `contains`, and `substring`
- selected `java.lang.Object` intrinsics for supported references: identity `equals`, deterministic `hashCode`, and `toString`
- `invokedynamic` string concatenation emitted by javac through `StringConcatFactory` for supported primitive/String values
- `System.out.println(String|int|boolean|char)` for supported computed values
- `fvm.runtime.Http.respond(port, body)` where `port` and `body` resolve through the supported subset

String indexing follows Java UTF-16 code-unit boundaries for supported values and rejects surrogate-pair extraction or split boundaries for now. Everything else fails at build time with an unsupported bytecode diagnostic. This is the first compiler/runtime slice, not full Java compatibility.

## Snapshot Native Mode

```bash
fvm build app.jar --mode snapshot-native --java 25 --kernel /path/to/vmlinux
fvm snapshot app.fvm --wait-http /health --verify-restore
fvm run app.fvm --snapshot initialized
```

Snapshots are stored under `app.fvm/snapshots/` and recorded in `metadata.json`.

## Legacy Snapshot Mode

Legacy snapshot mode packages a JRE and JAR, then uses `fvm-init` to launch:

```bash
fvm build app.jar \
  --mode legacy-snapshot \
  --java 25 \
  --jre /path/to/jre \
  --kernel /path/to/vmlinux

fvm snapshot app.fvm --wait-http /health --verify-restore
```

Use this for applications that are not native-image compatible yet.

## Framework Detection

```bash
fvm analyze app.jar
```

The analyzer detects plain Java, Micronaut, Quarkus, Spring Boot, and Servlet/WAR shapes. Native mode fails loudly for unsupported framework shapes unless `--allow-unsupported-framework` is supplied.

## Artifact Layout

```text
app.fvm/
  kernel
  rootfs.ext4
  firecracker.json
  metadata.json
  app.bin
  snapshots/
    initialized.mem
    initialized.vmstate
```

`metadata.json` records schema version, FVM version, app/framework analysis, Java/native-image versions, file hashes, runtime defaults, security settings, snapshots, diagnostics, and benchmark reports.

## Security And Hardening Hooks

The current CLI records and wires the first hardening hooks:

- `--guest-uid` and `--guest-gid` are written into `/etc/fvm-init.conf` so the guest child can drop privileges.
- `--cgroup KEY=VALUE` records cgroup v2 settings and applies them to the Firecracker process at run/snapshot time. The default base is `/sys/fs/cgroup/fvm`; override with `FVM_CGROUP_BASE`.
- `--secret NAME=SOURCE[:GUEST_PATH]` records explicit secret mounts in metadata.
- Firecracker jailer integration is represented in the manifest schema and should be wired to host launch policy before multi-tenant production use.

## Development

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```
