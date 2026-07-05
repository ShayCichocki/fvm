# Task 07 Evidence - Group Current FVM AOT Slice

## Baseline

- Plan acceptance read: `.omo/plans/fvm-aot-milestone-todos.md` lines 182-188.
- Supported subset read: `README.md` lines 176-191 and `docs/README.md` lines 27-39.
- Test strategy read: `docs/fvm-aot-test-strategy.md` lines 294-306.
- CodeGraph: no `codegraph_*` tools were available in this session, so I continued with direct Read/rg/LSP as instructed.
- Baseline command before edits:

```text
$ cargo test current_slice -- --nocapture
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.02s
     Running unittests src/main.rs (target/debug/deps/fvm-0820d539226aefd0)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 33 filtered out; finished in 0.00s

     Running unittests src/bin/fvm-init.rs (target/debug/deps/fvm_init-337b66bb2cb76031)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/cli_flow.rs (target/debug/deps/cli_flow-bac547771aafec0a)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out; finished in 0.00s
```

## Implementation Summary

- Added `src/fvm_aot/tests/current_slice.rs` and declared it from `src/fvm_aot/mod.rs` under `fvm_aot::tests`.
- Moved only supported behavior tests into `current_slice`: simple println, computed HTTP intrinsic, static fields and `<clinit>`, objects/arrays, multi-class closed world, interface dispatch/string concat, and String/Object/array core methods.
- Preserved exact supported assertions: native stdout `hello fvm-aot\n`, HTTP status `HTTP/1.1 200 OK`, and all expected response suffixes.
- Left unsupported diagnostic tests in `src/fvm_aot/mod.rs` unchanged.
- Reused `src/fvm_aot/test_support.rs` fixture helpers; no dependency or runtime/compiler behavior changes.
- Added explicit supported-test skip output when `javac` or `cc` is missing.

## Verification Commands

```text
$ cargo fmt --check; rc=$?; printf '\nEXIT_CODE=%s\n' "$rc"; exit "$rc"

EXIT_CODE=0
```

```text
$ cargo test current_slice -- --nocapture; rc=$?; printf '\nEXIT_CODE=%s\n' "$rc"; exit "$rc"
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.05s
     Running unittests src/main.rs (target/debug/deps/fvm-0820d539226aefd0)

running 7 tests
test fvm_aot::tests::current_slice::compiles_computed_http_intrinsic_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_objects_and_arrays_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_simple_println_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_static_fields_and_clinit_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_interface_dispatch_and_string_concat_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_multi_class_closed_world_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_string_object_array_core_methods_when_toolchain_is_available ... ok

test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 26 filtered out; finished in 2.03s

     Running unittests src/bin/fvm-init.rs (target/debug/deps/fvm_init-337b66bb2cb76031)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/cli_flow.rs (target/debug/deps/cli_flow-bac547771aafec0a)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out; finished in 0.00s


EXIT_CODE=0
```

```text
$ cargo test fvm_aot::tests -- --nocapture; rc=$?; printf '\nEXIT_CODE=%s\n' "$rc"; exit "$rc"
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.03s
     Running unittests src/main.rs (target/debug/deps/fvm-0820d539226aefd0)

running 13 tests
test fvm_aot::tests::rejects_invalid_classfile ... ok
test fvm_aot::tests::unsupported_lambda_reports_required_feature ... ok
test fvm_aot::tests::unsupported_multidimensional_array_reports_required_feature ... ok
test fvm_aot::tests::unsupported_dynamic_class_loading_reports_required_feature ... ok
test fvm_aot::tests::unsupported_athrow_reports_class_method_and_bci ... ok
test fvm_aot::tests::current_slice::compiles_multi_class_closed_world_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_string_object_array_core_methods_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_simple_println_when_toolchain_is_available ... ok
test fvm_aot::tests::unsupported_long_and_double_report_primitive_gap ... ok
test fvm_aot::tests::current_slice::compiles_static_fields_and_clinit_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_computed_http_intrinsic_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_interface_dispatch_and_string_concat_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_objects_and_arrays_when_toolchain_is_available ... ok

test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 20 filtered out; finished in 2.43s

     Running unittests src/bin/fvm-init.rs (target/debug/deps/fvm_init-337b66bb2cb76031)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/cli_flow.rs (target/debug/deps/cli_flow-bac547771aafec0a)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out; finished in 0.00s


EXIT_CODE=0
```

```text
$ cargo test; rc=$?; printf '\nEXIT_CODE=%s\n' "$rc"; exit "$rc"
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.05s
     Running unittests src/main.rs (target/debug/deps/fvm-0820d539226aefd0)

running 33 tests
test cli::tests::converts_kib_to_mib ... ok
test artifact::tests::parses_port_mapping ... ok
test benchmark::tests::summarizes_iterations ... ok
test artifact::tests::rejects_invalid_port_mapping ... ok
test cli::tests::parses_cgroup_setting ... ok
test cli::tests::parses_secret_with_default_guest_path ... ok
test cli::tests::parses_memory_units ... ok
test fvm_aot::tests::rejects_invalid_classfile ... ok
test fvm_aot::test_support::tests::keep_artifacts_returns_tempdir_path ... ok
test firecracker::tests::parses_latest_guest_rss ... ok
test fvm_aot::test_support::tests::compile_source_paths_reports_missing_source_path ... ok
test firecracker::tests::writes_firecracker_config ... ok
test artifact::tests::records_file_relative_to_artifact_dir ... ok
test guest_init::tests::none_is_rejected_for_legacy_mode ... ok
test rootfs::tests::parses_ldd_paths ... ok
test guest_init::tests::none_is_allowed_when_guest_rss_not_required ... ok
test rootfs::tests::boot_args_uses_requested_init ... ok
test toolchain::tests::default_toolchain_names_are_expected ... ok
test framework::tests::detects_plain_java_jar ... ok
test framework::tests::rejects_spring_without_native_metadata_for_native_mode ... ok
test framework::tests::accepts_spring_with_native_metadata ... ok
test fvm_aot::tests::unsupported_lambda_reports_required_feature ... ok
test fvm_aot::tests::unsupported_dynamic_class_loading_reports_required_feature ... ok
test fvm_aot::tests::unsupported_multidimensional_array_reports_required_feature ... ok
test fvm_aot::tests::unsupported_athrow_reports_class_method_and_bci ... ok
test fvm_aot::tests::current_slice::compiles_computed_http_intrinsic_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_objects_and_arrays_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_string_object_array_core_methods_when_toolchain_is_available ... ok
test fvm_aot::tests::unsupported_long_and_double_report_primitive_gap ... ok
test fvm_aot::tests::current_slice::compiles_static_fields_and_clinit_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_multi_class_closed_world_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_interface_dispatch_and_string_concat_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_simple_println_when_toolchain_is_available ... ok

test result: ok. 33 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.46s

     Running unittests src/bin/fvm-init.rs (target/debug/deps/fvm_init-337b66bb2cb76031)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/cli_flow.rs (target/debug/deps/cli_flow-bac547771aafec0a)

running 4 tests
test unsupported_spring_shape_fails_native_build ... ok
test doctor_non_strict_is_laptop_safe ... ok
test dry_run_artifact_lifecycle_works ... ok
test fvm_aot_dry_run_builds_supported_println_subset ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.38s


EXIT_CODE=0
```

```text
$ cargo clippy --all-targets -- -D warnings; rc=$?; printf '\nEXIT_CODE=%s\n' "$rc"; exit "$rc"
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.15s

EXIT_CODE=0
```

## Failure QA

Controlled PATH removed `cc` while preserving `cargo`, Rust tools, `java`, and `javac` through symlinks in `/var/folders/13/plbf88cn6rx69npgwtgrcs400000gn/T/opencode/fvm-aot-no-cc-path`.

```text
$ PATH=/var/folders/13/plbf88cn6rx69npgwtgrcs400000gn/T/opencode/fvm-aot-no-cc-path cargo test current_slice -- --nocapture; rc=$?; printf '\nEXIT_CODE=%s\n' "$rc"; exit "$rc"
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.05s
     Running unittests src/main.rs (target/debug/deps/fvm-0820d539226aefd0)

running 7 tests
skipping fvm-aot current_slice test because required tool(s) are missing: cc
skipping fvm-aot current_slice test because required tool(s) are missing: cc
test fvm_aot::tests::current_slice::compiles_static_fields_and_clinit_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_objects_and_arrays_when_toolchain_is_available ... ok
skipping fvm-aot current_slice test because required tool(s) are missing: cc
skipping fvm-aot current_slice test because required tool(s) are missing: cc
test fvm_aot::tests::current_slice::compiles_computed_http_intrinsic_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_simple_println_when_toolchain_is_available ... ok
skipping fvm-aot current_slice test because required tool(s) are missing: cc
test fvm_aot::tests::current_slice::compiles_string_object_array_core_methods_when_toolchain_is_available ... ok
skipping fvm-aot current_slice test because required tool(s) are missing: cc
test fvm_aot::tests::current_slice::compiles_interface_dispatch_and_string_concat_when_toolchain_is_available ... ok
skipping fvm-aot current_slice test because required tool(s) are missing: cc
test fvm_aot::tests::current_slice::compiles_multi_class_closed_world_when_toolchain_is_available ... ok

test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 26 filtered out; finished in 0.30s

     Running unittests src/bin/fvm-init.rs (target/debug/deps/fvm_init-337b66bb2cb76031)
     Running tests/cli_flow.rs (target/debug/deps/cli_flow-bac547771aafec0a)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out; finished in 0.00s


EXIT_CODE=0
```

## LSP Diagnostics

```text
$ lsp_diagnostics src/fvm_aot/mod.rs
LSP daemon unreachable: daemon request timed out.
Socket: /Users/scichocki/.codex/codex-lsp/daemon/v0.1.0/daemon.sock
Logs: /Users/scichocki/.codex/codex-lsp/daemon/v0.1.0/daemon.log

$ lsp_diagnostics src/fvm_aot/tests/current_slice.rs
LSP daemon unreachable: daemon request timed out.
Socket: /Users/scichocki/.codex/codex-lsp/daemon/v0.1.0/daemon.sock
Logs: /Users/scichocki/.codex/codex-lsp/daemon/v0.1.0/daemon.log
```

## Adversarial Classes And Cleanup

- Unsupported tests for exceptions, LambdaMetafactory, Class.forName, long/double primitive gaps, and multidimensional arrays were not deleted or weakened; `cargo test fvm_aot::tests -- --nocapture` still runs them.
- A parallel verification attempt caused same-port collisions by running multiple `current_slice` suites at once; sequential required commands passed. No product change was needed because the required command is a single `cargo test current_slice -- --nocapture` invocation.
- `src/fvm_aot/tests/current_slice.rs` is intentionally marked `SIZE_OK` because it is a grouped behavior-slice fixture module whose purpose is discoverability through the `current_slice` test filter.
- Temporary missing-`cc` PATH symlinks were created outside the repo under `/var/folders/13/plbf88cn6rx69npgwtgrcs400000gn/T/opencode/fvm-aot-no-cc-path`; no temporary PATH mutation or generated artifact remains in the worktree.

## Commit

- Intended commit message: `test: group current fvm-aot slice`.
- Commit hash: reported by `GIT_MASTER=1 git log -1 --oneline` after the single product commit is created.
