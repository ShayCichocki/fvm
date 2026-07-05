# T12 Evidence: scripts/perf-aot

Task: `scripts/perf-aot: Add current aot-http benchmark wrapper - expect baseline records median/p90/p99 and sizes`

## Changed Files

- `scripts/perf-aot`
- `.omo/evidence/task-12-fvm-aot-milestone-todos.md`
- `.omo/notepads/fvm-aot-milestone-todos/learnings.md`

No product files besides `scripts/perf-aot` were modified for T12. `scripts/perf-native` was read but not edited. `.omo/run-continuation/*.json` files were not staged, deleted, or used as evidence.

## Implementation Notes

- `scripts/perf-aot` mirrors the strict bash shape of `scripts/perf-native`: `set -euo pipefail`, repo `ROOT`, configurable `FVM`, `FVM_KERNEL`, ports, `ITERATIONS`, `ROOTFS_SIZE`, result directory, `measure`, `size_line`, doctor/build/run/math/inspect, and final result path.
- The wrapper compiles the current `examples/aot-http/src` Java sources, including `fvm/runtime/Http.java`, packages `aot-http.jar` with main class `AotHttp`, and builds `aot-http.fvm` with `fvm build --backend fvm-aot`.
- The default guest port is `9090`, derived from `AotHttp -> AotConfig(9000, ...)` and `AotConfig.port() = base + 40 + 50 + 2 - 2`.
- `sizes.env` records `jar_bytes`, `artifact_app_bin_bytes`, `artifact_rootfs_bytes`, `artifact_kernel_bytes`, and `artifact_du_mib`.
- `timings.env` records wrapper step timings and, after `fvm math`, parsed `aot_cold_boot_median_ms`, `aot_cold_boot_p90_ms`, `aot_cold_boot_p99_ms`, and RSS summary keys when present.
- The benchmark run uses `--benchmark-iterations "$ITERATIONS"` and readiness `/health`.
- `inspect.txt` comes from `fvm inspect "$ARTIFACT" --verify`; `aot-math.txt` comes from `fvm math "$ARTIFACT"`.
- `--help` exits successfully without requiring FVM, kernel, Linux, or KVM.

## Local Host Limitation

Local verification ran on macOS. macOS is build/test/dry-run only for this repository; real `fvm run` benchmarking requires Linux/KVM and a usable `FVM_KERNEL`. No Linux/KVM benchmark success is claimed from this evidence.

## Command Transcripts

### `bash -n scripts/perf-aot`

Command:

```bash
bash -n "scripts/perf-aot"; rc=$?; printf 'exit=%s\n' "$rc"; exit "$rc"
```

Output:

```text
exit=0
```

Exit code: 0

### `scripts/perf-aot --help`

Command:

```bash
"scripts/perf-aot" --help; rc=$?; printf 'exit=%s\n' "$rc"; exit "$rc"
```

Output excerpt:

```text
Usage: ITERATIONS=3 FVM_KERNEL=/path/to/vmlinux scripts/perf-aot

Builds examples/aot-http with fvm build --backend fvm-aot, runs the current
cold HTTP benchmark, and writes timings.env, sizes.env, inspect.txt, and
aot-math.txt under RESULT_DIR.

Required host prerequisites for benchmark execution:
  Linux x86_64 with KVM available at /dev/kvm
  Firecracker and TAP networking access verified by fvm doctor --strict
  FVM_KERNEL pointing to a Firecracker-compatible Linux kernel image
  javac, jar, cc, and ext4 tooling on PATH
...
exit=0
```

Exit code: 0

### Missing Kernel Failure Path

Command:

```bash
FVM_KERNEL=/definitely/missing "scripts/perf-aot"; rc=$?; printf 'exit=%s\n' "$rc"; exit 0
```

Output:

```text
missing kernel: /definitely/missing
set FVM_KERNEL=/path/to/firecracker/kernel
real fvm-aot benchmarks require Linux/KVM; macOS is build/test/dry-run only
exit=1
```

Observed script exit code: 1. The shell wrapper exited 0 only so the transcript could be captured in this evidence run.

### Cargo Verification

Command:

```bash
cargo fmt --check && cargo test && cargo clippy --all-targets -- -D warnings
```

Output excerpt:

```text
running 39 tests
...
test result: ok. 39 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.56s

running 1 test
test aot_firecracker_smoke_requires_explicit_linux_kvm_gate ... ignored

test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s

running 4 tests
test unsupported_spring_shape_fails_native_build ... ok
test doctor_non_strict_is_laptop_safe ... ok
test fvm_aot_dry_run_builds_supported_println_subset ... ok
test dry_run_artifact_lifecycle_works ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.37s

    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.05s
```

Exit code: 0

## Linux/KVM Happy Path To Run Later

On a Linux x86_64 host with `/dev/kvm`, Firecracker, `javac`, `jar`, `cc`, ext4 tooling, and a valid kernel:

```bash
ITERATIONS=3 FVM_KERNEL=/path/to/vmlinux scripts/perf-aot
```

Expected outputs:

- `perf-results/aot-*/timings.env`
- `perf-results/aot-*/sizes.env`
- `perf-results/aot-*/inspect.txt`
- `perf-results/aot-*/aot-math.txt`
