# Task 11 Evidence: ignored fvm-aot Firecracker smoke

Date: 2026-07-05
Host: macOS/darwin local verification. This is not Linux/KVM evidence; the expected local result is an explicit skip.

## Scope

- Added `tests/aot_firecracker.rs` as an ignored integration test.
- The test uses `env!("CARGO_BIN_EXE_fvm")`, not an installed `fvm`.
- Non-Linux returns success after printing: `Firecracker requires Linux/KVM; macOS is build/test/dry-run only`.
- Linux/KVM gate checks `/dev/kvm`, `/dev/net/tun`, `FVM_KERNEL`, `firecracker`, `javac`, `jar`, `cc`, `ip`, and `mkfs.ext4`/`mke2fs` before running the real smoke flow.
- When the Linux/KVM gate is complete, the test compiles all `examples/aot-http/src` Java sources with `javac`, packages the JAR with `jar`, runs `fvm build --backend fvm-aot`, runs `fvm run --once --wait-http /health`, then runs `fvm inspect --verify`.
- Temp classes, JAR, and artifact paths are under `tempfile::tempdir()` and are cleaned up when the ignored test returns.

## LSP diagnostics

Command/tool: `lsp_diagnostics /Users/scichocki/personal/fvm/tests/aot_firecracker.rs`
Exit/Result: unavailable; daemon timed out.

```text
LSP daemon unreachable: daemon request timed out.
The MCP server is a thin proxy and never runs language servers in-process.
Socket: /Users/scichocki/.codex/codex-lsp/daemon/v0.1.0/daemon.sock
Logs: /Users/scichocki/.codex/codex-lsp/daemon/v0.1.0/daemon.log
The daemon is auto-started on demand and will be retried on the next request.
```

## Verification transcript

### `cargo fmt --check`

Exit code: 0

```text
```

### `cargo test aot_firecracker -- --ignored --nocapture`

Exit code: 0

```text
   Compiling fvm v0.1.0 (/Users/scichocki/personal/fvm)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.17s
     Running unittests src/main.rs (target/debug/deps/fvm-0820d539226aefd0)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 39 filtered out; finished in 0.00s

     Running unittests src/bin/fvm-init.rs (target/debug/deps/fvm_init-337b66bb2cb76031)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/aot_firecracker.rs (target/debug/deps/aot_firecracker-2d321fb3fa72d4b6)

running 1 test
skipping fvm-aot Firecracker smoke: Firecracker requires Linux/KVM; macOS is build/test/dry-run only
test aot_firecracker_smoke_requires_explicit_linux_kvm_gate ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/cli_flow.rs (target/debug/deps/cli_flow-bac547771aafec0a)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out; finished in 0.00s
```

### `cargo test --test aot_firecracker -- --ignored --nocapture`

Exit code: 0

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.03s
     Running tests/aot_firecracker.rs (target/debug/deps/aot_firecracker-2d321fb3fa72d4b6)

running 1 test
skipping fvm-aot Firecracker smoke: Firecracker requires Linux/KVM; macOS is build/test/dry-run only
test aot_firecracker_smoke_requires_explicit_linux_kvm_gate ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

### `cargo test`

Exit code: 0

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.03s
     Running unittests src/main.rs (target/debug/deps/fvm-0820d539226aefd0)

running 39 tests
test artifact::tests::parses_port_mapping ... ok
test cli::tests::converts_kib_to_mib ... ok
test cli::tests::parses_memory_units ... ok
test cli::tests::parses_cgroup_setting ... ok
test benchmark::tests::summarizes_iterations ... ok
test cli::tests::parses_secret_with_default_guest_path ... ok
test artifact::tests::rejects_invalid_port_mapping ... ok
test fvm_aot::tests::failure_artifacts::child_failure_artifact_mode ... ok
test fvm_aot::test_support::tests::keep_artifacts_returns_tempdir_path ... ok
test firecracker::tests::writes_firecracker_config ... ok
test firecracker::tests::parses_latest_guest_rss ... ok
test fvm_aot::test_support::tests::compile_source_paths_reports_missing_source_path ... ok
test artifact::tests::records_file_relative_to_artifact_dir ... ok
test fvm_aot::tests::rejects_invalid_classfile ... ok
test framework::tests::detects_plain_java_jar ... ok
test framework::tests::accepts_spring_with_native_metadata ... ok
test framework::tests::rejects_spring_without_native_metadata_for_native_mode ... ok
test guest_init::tests::none_is_allowed_when_guest_rss_not_required ... ok
test guest_init::tests::none_is_rejected_for_legacy_mode ... ok
test rootfs::tests::boot_args_uses_requested_init ... ok
test rootfs::tests::parses_ldd_paths ... ok
test toolchain::tests::default_toolchain_names_are_expected ... ok
test fvm_aot::tests::failure_artifacts::cleans_failed_artifacts_when_env_var_is_unset ... ok
test fvm_aot::tests::failure_artifacts::preserves_failed_artifacts_when_env_var_is_set ... ok
test fvm_aot::tests::unsupported::unsupported_athrow_reports_class_method_and_bci ... ok
test fvm_aot::tests::unsupported::unsupported_lambda_reports_required_feature_and_milestone ... ok
test fvm_aot::tests::differential::differential_unsupported_fixture_fails_before_native_execution ... ok
test fvm_aot::tests::unsupported::unsupported_dynamic_class_loading_reports_required_feature_and_milestone ... ok
test fvm_aot::tests::unsupported::unsupported_multidimensional_array_reports_required_feature_and_milestone ... ok
test fvm_aot::tests::unsupported::unsupported_tableswitch_reports_primitive_completeness_milestone ... ok
test fvm_aot::tests::current_slice::compiles_static_fields_and_clinit_when_toolchain_is_available ... ok
test fvm_aot::tests::unsupported::unsupported_long_and_double_report_primitive_gap_and_milestone ... ok
test fvm_aot::tests::current_slice::compiles_multi_class_closed_world_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_objects_and_arrays_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_string_object_array_core_methods_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_interface_dispatch_and_string_concat_when_toolchain_is_available ... ok
test fvm_aot::tests::differential::differential_println_matches_hotspot ... ok
test fvm_aot::tests::current_slice::compiles_computed_http_intrinsic_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_simple_println_when_toolchain_is_available ... ok

test result: ok. 39 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.23s

     Running unittests src/bin/fvm-init.rs (target/debug/deps/fvm_init-337b66bb2cb76031)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/aot_firecracker.rs (target/debug/deps/aot_firecracker-2d321fb3fa72d4b6)

running 1 test
test aot_firecracker_smoke_requires_explicit_linux_kvm_gate ... ignored

test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 4 tests
     Running tests/cli_flow.rs (target/debug/deps/cli_flow-bac547771aafec0a)
test unsupported_spring_shape_fails_native_build ... ok
test doctor_non_strict_is_laptop_safe ... ok
test fvm_aot_dry_run_builds_supported_println_subset ... ok
test dry_run_artifact_lifecycle_works ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.41s
```

### `cargo clippy --all-targets -- -D warnings`

Exit code: 0

```text
    Checking fvm v0.1.0 (/Users/scichocki/personal/fvm)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.10s
```

### Pure LOC check

Command: `awk '!/^[[:space:]]*$/ && !/^[[:space:]]*(\/\/|#|--)/' 'tests/aot_firecracker.rs' | wc -l`
Exit code: 0

```text
     161
```

## Local result

Local ignored-test execution passed by explicit macOS skip. No Linux/KVM success is claimed from this host.
