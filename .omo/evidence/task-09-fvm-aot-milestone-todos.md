# Task 09 Evidence: fvm-aot differential stdout harness

## Baseline

- Date: 2026-07-05
- Work id: `fvm-aot-milestone-todos-15021992`
- CodeGraph: no `codegraph_*` tools were exposed in this session, so inspection continued with Read/Grep/Glob/LSP as instructed.
- `AGENTS.md`: repository search found none.
- Initial `GIT_MASTER=1 git status --short --untracked-files=all` exited 0 and showed only existing `.omo/**` planning/evidence/notepad/run-continuation files. `.omo/run-continuation/*.json` were not staged, deleted, or modified.

## Implementation Summary

- Added `src/fvm_aot/tests/differential.rs` and declared it from `src/fvm_aot/mod.rs` as `mod differential;`.
- Added `assert_aot_matches_hotspot` for non-HTTP stdout fixtures using existing T06 helpers: `javac` source compilation, JAR packaging, HotSpot execution, `compile_jar` through `compile_native`, and generated native execution.
- Started with an inline `AotHello` println fixture and asserted HotSpot stdout, native stdout, and the expected bytes are exactly `hello fvm-aot\n`.
- Added `differential_unsupported_fixture_fails_before_native_execution` using `athrow`; it calls `compile_native(... dry_run: true)` and asserts unsupported diagnostics before any `run_native` call.

## Failure QA

- Unsupported fixture result: `cargo test differential -- --nocapture` ran `differential_unsupported_fixture_fails_before_native_execution` and it passed.
- The negative path asserts diagnostic substrings `opcode 0xbf`, `fvm-aot exceptions/athrow are not supported yet`, and the bytecode location for `AotDifferentialUnsupported.main`.
- Missing-tool behavior: both differential tests call a local `skip_missing_toolchain` helper, which prints `skipping fvm-aot differential test because required tool(s) are missing: ...` and returns without panic.

## LSP / Size / Self-Review

- `lsp_diagnostics` for `src/fvm_aot/mod.rs`: timed out at `/Users/scichocki/.codex/codex-lsp/daemon/v0.1.0/daemon.sock`.
- `lsp_diagnostics` for `src/fvm_aot/tests/differential.rs`: timed out at the same daemon socket.
- Pure LOC: `src/fvm_aot/mod.rs` = 115, `src/fvm_aot/tests/differential.rs` = 141.
- Single responsibility: `differential.rs` owns HotSpot-vs-native stdout differential fixtures only.
- Boundary purity: Java fixture text stays at the test boundary and is compiled through existing typed helpers.
- Variant discrimination: no tagged variant discrimination was added.
- Escape hatches: only test-local `unwrap`/`unwrap_err` in `#[test]` code; no production escape hatches.
- Defensive layer: skip checks are environment gates for external tools, not internal defensive fallback.
- Tests: reverting the differential module or module declaration makes the requested filter fail or run zero relevant tests.

## Verification Transcript

### `cargo fmt --check`

Exit code: 0

```text

```

### `cargo test differential_println_matches_hotspot -- --nocapture`

Exit code: 0

```text
   Compiling fvm v0.1.0 (/Users/scichocki/personal/fvm)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.68s
     Running unittests src/main.rs (target/debug/deps/fvm-0820d539226aefd0)

running 1 test
test fvm_aot::tests::differential::differential_println_matches_hotspot ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 35 filtered out; finished in 0.61s

     Running unittests src/bin/fvm-init.rs (target/debug/deps/fvm_init-337b66bb2cb76031)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/cli_flow.rs (target/debug/deps/cli_flow-bac547771aafec0a)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out; finished in 0.00s
```

### `cargo test differential -- --nocapture`

Exit code: 0

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.03s
     Running unittests src/main.rs (target/debug/deps/fvm-0820d539226aefd0)

running 2 tests
test fvm_aot::tests::differential::differential_unsupported_fixture_fails_before_native_execution ... ok
test fvm_aot::tests::differential::differential_println_matches_hotspot ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 34 filtered out; finished in 2.03s

     Running unittests src/bin/fvm-init.rs (target/debug/deps/fvm_init-337b66bb2cb76031)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/cli_flow.rs (target/debug/deps/cli_flow-bac547771aafec0a)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out; finished in 0.00s
```

### `cargo test fvm_aot::tests -- --nocapture`

Exit code: 0

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.03s
     Running unittests src/main.rs (target/debug/deps/fvm-0820d539226aefd0)

running 16 tests
test fvm_aot::tests::rejects_invalid_classfile ... ok
test fvm_aot::tests::unsupported::unsupported_dynamic_class_loading_reports_required_feature_and_milestone ... ok
test fvm_aot::tests::differential::differential_unsupported_fixture_fails_before_native_execution ... ok
test fvm_aot::tests::unsupported::unsupported_multidimensional_array_reports_required_feature_and_milestone ... ok
test fvm_aot::tests::unsupported::unsupported_tableswitch_reports_primitive_completeness_milestone ... ok
test fvm_aot::tests::unsupported::unsupported_athrow_reports_class_method_and_bci ... ok
test fvm_aot::tests::unsupported::unsupported_lambda_reports_required_feature_and_milestone ... ok
test fvm_aot::tests::current_slice::compiles_multi_class_closed_world_when_toolchain_is_available ... ok
test fvm_aot::tests::unsupported::unsupported_long_and_double_report_primitive_gap_and_milestone ... ok
test fvm_aot::tests::current_slice::compiles_simple_println_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_string_object_array_core_methods_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_static_fields_and_clinit_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_objects_and_arrays_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_computed_http_intrinsic_when_toolchain_is_available ... ok
test fvm_aot::tests::differential::differential_println_matches_hotspot ... ok
test fvm_aot::tests::current_slice::compiles_interface_dispatch_and_string_concat_when_toolchain_is_available ... ok

test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 20 filtered out; finished in 2.28s
```

### `cargo test`

Exit code: 0

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.02s
     Running unittests src/main.rs (target/debug/deps/fvm-0820d539226aefd0)

running 36 tests
test artifact::tests::parses_port_mapping ... ok
test benchmark::tests::summarizes_iterations ... ok
test artifact::tests::rejects_invalid_port_mapping ... ok
test cli::tests::parses_cgroup_setting ... ok
test cli::tests::parses_secret_with_default_guest_path ... ok
test cli::tests::converts_kib_to_mib ... ok
test cli::tests::parses_memory_units ... ok
test artifact::tests::records_file_relative_to_artifact_dir ... ok
test fvm_aot::tests::rejects_invalid_classfile ... ok
test firecracker::tests::parses_latest_guest_rss ... ok
test fvm_aot::test_support::tests::compile_source_paths_reports_missing_source_path ... ok
test fvm_aot::test_support::tests::keep_artifacts_returns_tempdir_path ... ok
test firecracker::tests::writes_firecracker_config ... ok
test framework::tests::detects_plain_java_jar ... ok
test framework::tests::rejects_spring_without_native_metadata_for_native_mode ... ok
test guest_init::tests::none_is_allowed_when_guest_rss_not_required ... ok
test guest_init::tests::none_is_rejected_for_legacy_mode ... ok
test rootfs::tests::boot_args_uses_requested_init ... ok
test rootfs::tests::parses_ldd_paths ... ok
test framework::tests::accepts_spring_with_native_metadata ... ok
test toolchain::tests::default_toolchain_names_are_expected ... ok
test fvm_aot::tests::unsupported::unsupported_athrow_reports_class_method_and_bci ... ok
test fvm_aot::tests::unsupported::unsupported_dynamic_class_loading_reports_required_feature_and_milestone ... ok
test fvm_aot::tests::differential::differential_unsupported_fixture_fails_before_native_execution ... ok
test fvm_aot::tests::unsupported::unsupported_multidimensional_array_reports_required_feature_and_milestone ... ok
test fvm_aot::tests::unsupported::unsupported_lambda_reports_required_feature_and_milestone ... ok
test fvm_aot::tests::unsupported::unsupported_tableswitch_reports_primitive_completeness_milestone ... ok
test fvm_aot::tests::current_slice::compiles_computed_http_intrinsic_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_objects_and_arrays_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_static_fields_and_clinit_when_toolchain_is_available ... ok
test fvm_aot::tests::unsupported::unsupported_long_and_double_report_primitive_gap_and_milestone ... ok
test fvm_aot::tests::current_slice::compiles_multi_class_closed_world_when_toolchain_is_available ... ok
test fvm_aot::tests::differential::differential_println_matches_hotspot ... ok
test fvm_aot::tests::current_slice::compiles_string_object_array_core_methods_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_interface_dispatch_and_string_concat_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_simple_println_when_toolchain_is_available ... ok

test result: ok. 36 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.61s

     Running unittests src/bin/fvm-init.rs (target/debug/deps/fvm_init-337b66bb2cb76031)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/cli_flow.rs (target/debug/deps/cli_flow-bac547771aafec0a)

running 4 tests
test unsupported_spring_shape_fails_native_build ... ok
test doctor_non_strict_is_laptop_safe ... ok
test fvm_aot_dry_run_builds_supported_println_subset ... ok
test dry_run_artifact_lifecycle_works ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.43s
```

### `cargo clippy --all-targets -- -D warnings`

Exit code: 0

```text
    Checking fvm v0.1.0 (/Users/scichocki/personal/fvm)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.50s
```

## Adversarial Classes Checked

- Positive: byte-exact stdout agreement between HotSpot and native generated binary, including trailing newline.
- Negative: unsupported `athrow` fixture fails during AOT build setup and never reaches native execution.
- Environment: explicit missing-tool skip path for `javac`, `java`, and `cc`.
- Scope: no compiler/runtime semantics, diagnostics wording, examples, or docs were modified.

## Cleanup

- No temporary fixture mutations were left in the tree.
- `.omo/run-continuation/*.json` files were not staged or cleaned.
- Commit hash: recorded in final handoff after creating the single requested commit.
