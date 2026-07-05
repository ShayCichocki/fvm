# Task 10 Evidence: FVM AOT Failure Artifacts

## Baseline

- Scope read: `.omo/plans/fvm-aot-milestone-todos.md` T10 lines 206-212, `docs/fvm-aot-test-strategy.md` lines 501-516, `src/fvm_aot/mod.rs`, `src/fvm_aot/test_support.rs`, current-slice/unsupported/differential test modules, and notepads.
- CodeGraph tools were not available in this session, so exploration used direct read/search/LSP as instructed.
- Baseline command: `cargo test failure_artifacts -- --nocapture`
- Exit: 0
- Output showed zero meaningful failure-artifact tests before implementation:

```text
Finished `test` profile [unoptimized + debuginfo] target(s) in 0.06s
Running unittests src/main.rs (target/debug/deps/fvm-0820d539226aefd0)
running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 36 filtered out; finished in 0.00s
Running unittests src/bin/fvm-init.rs (target/debug/deps/fvm_init-337b66bb2cb76031)
running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
Running tests/cli_flow.rs (target/debug/deps/cli_flow-bac547771aafec0a)
running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out; finished in 0.00s
```

## Red Test

- Added `src/fvm_aot/tests/failure_artifacts.rs` and `mod failure_artifacts;`.
- Command: `cargo test failure_artifacts -- --nocapture`
- Exit: 101
- Expected failure:

```text
error[E0599]: no method named `root_path` found for struct `test_support::AotFixture` in the current scope
error[E0599]: no method named `preserve_failed_artifacts` found for struct `test_support::AotFixture` in the current scope
error[E0599]: no method named `write_artifact` found for reference `&test_support::AotFixture` in the current scope
error: could not compile `fvm` (bin "fvm" test) due to 3 previous errors
```

## Implementation Summary

- `src/fvm_aot/test_support.rs` now has `FailedAotArtifacts` and `AotFixture::preserve_failed_artifacts(reason)`, which returns `Some(path)` and emits `preserved AOT test artifacts at ...` only when `FVM_KEEP_FAILED_AOT=1`.
- `AotFixture` panic-drop retention is now gated by `FVM_KEEP_FAILED_AOT=1`; default panic/drop cleanup remains the `TempDir` behavior.
- `AotFixture::artifact_path` exposes fixture-relative paths for controlled tests without exposing the owned `TempDir`.
- `failure_artifacts` uses child test processes with `Command::env` / `env_remove` rather than parent-process env mutation. This avoids Rust 2024 `std::env::set_var` unsafety and keeps tests deterministic under the normal parallel test harness.
- Controlled artifacts written under the fixture root: `src/AotFailure.java`, `classes/AotFailure.class`, `app.jar`, `native/app`, and `logs/compiler.log`.

## Verification Commands

### `cargo fmt --check`

- Exit: 0
- Output: no output.

### `cargo test failure_artifacts -- --nocapture`

- Exit: 0

```text
running 3 tests
test fvm_aot::tests::failure_artifacts::child_failure_artifact_mode ... ok
test fvm_aot::tests::failure_artifacts::preserves_failed_artifacts_when_env_var_is_set ... ok
test fvm_aot::tests::failure_artifacts::cleans_failed_artifacts_when_env_var_is_unset ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 36 filtered out; finished in 0.01s
```

### `cargo test fvm_aot::tests -- --nocapture`

- Exit: 0

```text
running 19 tests
test fvm_aot::tests::failure_artifacts::child_failure_artifact_mode ... ok
test fvm_aot::tests::rejects_invalid_classfile ... ok
test fvm_aot::tests::failure_artifacts::cleans_failed_artifacts_when_env_var_is_unset ... ok
test fvm_aot::tests::failure_artifacts::preserves_failed_artifacts_when_env_var_is_set ... ok
test fvm_aot::tests::unsupported::unsupported_lambda_reports_required_feature_and_milestone ... ok
test fvm_aot::tests::unsupported::unsupported_tableswitch_reports_primitive_completeness_milestone ... ok
test fvm_aot::tests::unsupported::unsupported_dynamic_class_loading_reports_required_feature_and_milestone ... ok
test fvm_aot::tests::unsupported::unsupported_athrow_reports_class_method_and_bci ... ok
test fvm_aot::tests::differential::differential_unsupported_fixture_fails_before_native_execution ... ok
test fvm_aot::tests::unsupported::unsupported_multidimensional_array_reports_required_feature_and_milestone ... ok
test fvm_aot::tests::differential::differential_println_matches_hotspot ... ok
test fvm_aot::tests::unsupported::unsupported_long_and_double_report_primitive_gap_and_milestone ... ok
test fvm_aot::tests::current_slice::compiles_static_fields_and_clinit_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_objects_and_arrays_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_string_object_array_core_methods_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_computed_http_intrinsic_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_interface_dispatch_and_string_concat_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_multi_class_closed_world_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_simple_println_when_toolchain_is_available ... ok
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 20 filtered out; finished in 2.18s
```

### `cargo test`

- Exit: 0

```text
running 39 tests
test artifact::tests::parses_port_mapping ... ok
test artifact::tests::rejects_invalid_port_mapping ... ok
test benchmark::tests::summarizes_iterations ... ok
test cli::tests::parses_memory_units ... ok
test cli::tests::parses_secret_with_default_guest_path ... ok
test cli::tests::parses_cgroup_setting ... ok
test cli::tests::converts_kib_to_mib ... ok
test fvm_aot::tests::failure_artifacts::child_failure_artifact_mode ... ok
test fvm_aot::test_support::tests::keep_artifacts_returns_tempdir_path ... ok
test firecracker::tests::parses_latest_guest_rss ... ok
test fvm_aot::tests::rejects_invalid_classfile ... ok
test firecracker::tests::writes_firecracker_config ... ok
test artifact::tests::records_file_relative_to_artifact_dir ... ok
test fvm_aot::test_support::tests::compile_source_paths_reports_missing_source_path ... ok
test framework::tests::detects_plain_java_jar ... ok
test framework::tests::rejects_spring_without_native_metadata_for_native_mode ... ok
test framework::tests::accepts_spring_with_native_metadata ... ok
test guest_init::tests::none_is_allowed_when_guest_rss_not_required ... ok
test guest_init::tests::none_is_rejected_for_legacy_mode ... ok
test rootfs::tests::boot_args_uses_requested_init ... ok
test rootfs::tests::parses_ldd_paths ... ok
test toolchain::tests::default_toolchain_names_are_expected ... ok
test fvm_aot::tests::failure_artifacts::cleans_failed_artifacts_when_env_var_is_unset ... ok
test fvm_aot::tests::failure_artifacts::preserves_failed_artifacts_when_env_var_is_set ... ok
test fvm_aot::tests::differential::differential_unsupported_fixture_fails_before_native_execution ... ok
test fvm_aot::tests::unsupported::unsupported_athrow_reports_class_method_and_bci ... ok
test fvm_aot::tests::unsupported::unsupported_multidimensional_array_reports_required_feature_and_milestone ... ok
test fvm_aot::tests::unsupported::unsupported_tableswitch_reports_primitive_completeness_milestone ... ok
test fvm_aot::tests::unsupported::unsupported_lambda_reports_required_feature_and_milestone ... ok
test fvm_aot::tests::unsupported::unsupported_dynamic_class_loading_reports_required_feature_and_milestone ... ok
test fvm_aot::tests::current_slice::compiles_interface_dispatch_and_string_concat_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_objects_and_arrays_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_multi_class_closed_world_when_toolchain_is_available ... ok
test fvm_aot::tests::unsupported::unsupported_long_and_double_report_primitive_gap_and_milestone ... ok
test fvm_aot::tests::current_slice::compiles_computed_http_intrinsic_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_static_fields_and_clinit_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_string_object_array_core_methods_when_toolchain_is_available ... ok
test fvm_aot::tests::differential::differential_println_matches_hotspot ... ok
test fvm_aot::tests::current_slice::compiles_simple_println_when_toolchain_is_available ... ok
test result: ok. 39 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.58s

running 4 tests
test unsupported_spring_shape_fails_native_build ... ok
test doctor_non_strict_is_laptop_safe ... ok
test fvm_aot_dry_run_builds_supported_println_subset ... ok
test dry_run_artifact_lifecycle_works ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.43s
```

### `cargo clippy --all-targets -- -D warnings`

- Exit: 0

```text
Checking fvm v0.1.0 (/Users/scichocki/personal/fvm)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.62s
```

## Failure QA

- Keep path: child process runs with `FVM_KEEP_FAILED_AOT=1`; helper returns `Some(retained_dir)`, emits the path, parent verifies the retained dir still exists, checks all representative files, then removes it.
- Default cleanup path: child process runs with `FVM_KEEP_FAILED_AOT` removed; helper returns `None`, prints `retained_dir=<none>`, drops the fixture, and parent verifies the fixture root no longer exists.

## Adversarial Classes And Risks Checked

- Env isolation: no parent-process `std::env::set_var` / `remove_var`; child env is set through `Command`, so no global env leak remains after tests.
- Default cleanup: no artifact retention unless the exact value `FVM_KEEP_FAILED_AOT=1` is present.
- Panic retention: the existing panic-drop path is now gated by the same env var.
- Artifact classes: representative source, class, JAR, native output, and compiler-log files are preserved together under one fixture tempdir.
- Unsafe audit: `grep unsafe src/fvm_aot/tests/failure_artifacts.rs` found no matches; no new unsafe code was added.
- Size check: `src/fvm_aot/mod.rs` 116 pure LOC, `src/fvm_aot/test_support.rs` 246 pure LOC, `src/fvm_aot/tests/failure_artifacts.rs` 126 pure LOC.

## LSP Diagnostics

- Attempted for `src/fvm_aot/mod.rs`, `src/fvm_aot/test_support.rs`, and `src/fvm_aot/tests/failure_artifacts.rs`.
- Result: daemon timed out / unreachable at `/Users/scichocki/.codex/codex-lsp/daemon/v0.1.0/daemon.sock`; cargo, clippy, and rustfmt were used as effective diagnostics.

## Cleanup

- The retained keep-mode directory is removed by the parent test after asserting its contents.
- The default cleanup test verifies the child fixture root no longer exists after drop.
- No `.omo/run-continuation/*.json` files were staged, deleted, or cleaned.

## Commit

- Commit hash: `1c3e52c`
- Message: `test: preserve failed aot artifacts on demand`
