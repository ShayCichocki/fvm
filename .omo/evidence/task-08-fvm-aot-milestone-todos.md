# T08 Evidence: fvm-aot Unsupported Diagnostics

## Baseline

- `GIT_MASTER=1 git status --short --untracked-files=all` exit 0 before edits: only pre-existing untracked `.omo/**` planning/evidence/notepad/runtime files were present. `.omo/run-continuation/*.json` files were not staged, deleted, or cleaned.
- `GIT_MASTER=1 git diff --staged --stat` exit 0 before edits: no output.
- `GIT_MASTER=1 git diff --stat` exit 0 before edits: no output.
- `cargo test unsupported -- --nocapture` exit 0 before edits: 5 `fvm_aot::tests::unsupported_*` tests passed and the unrelated `tests/cli_flow.rs::unsupported_spring_shape_fails_native_build` filter match also passed.

## Implementation Summary

- Added `src/fvm_aot/tests/unsupported.rs` and declared it from `#[cfg(test)] mod tests` in `src/fvm_aot/mod.rs` with `mod unsupported;`.
- Moved golden unsupported diagnostics fixtures for `athrow`, `LambdaMetafactory`, `Class.forName`, `long`, `double`, and multidimensional arrays out of `mod.rs` into the dedicated module.
- Added `UnsupportedFixture` and `assert_unsupported_source`, which require each fixture to declare at least two diagnostic substrings and at least one location or milestone context substring before compiling the Java source.
- Preserved T07 `src/fvm_aot/tests/current_slice.rs` behavior; the file was not edited.
- Added failure QA coverage for a dense `tableswitch` fixture that asserts `opcode 0xaa`, `required feature: switch bytecodes`, and `planned milestone: primitive-completeness`.

## Failure QA

- First switch attempt used `args.length`; `cargo test unsupported -- --nocapture` exit 101 because compile-time evaluation hit `opcode 0xbe` with `fvm-aot null array reference during compile-time evaluation` before reaching switch dispatch.
- Second switch attempt used a static int but only cases `0` and `1`; `cargo test unsupported -- --nocapture` exit 101 because `javac --release 17` emitted `lookupswitch` (`opcode 0xab`) rather than the expected `tableswitch`.
- Final switch fixture uses dense cases `0..=5`; `cargo test unsupported -- --nocapture` exit 0 and the fixture passed with `opcode 0xaa` plus `primitive-completeness` milestone coverage.

## Adversarial Classes Checked

- `AotUnsupportedThrow`: asserts class/method/descriptor/bci context, `opcode 0xbf`, and `exceptions/athrow` feature text.
- `AotUnsupportedLambda`: asserts class/method/descriptor/bci context, `opcode 0xba`, `LambdaMetafactory`, `lambdas/method references`, and `planned milestone: dispatch-and-lambdas`.
- `AotUnsupportedClassForName`: asserts class/method/descriptor/bci context, `opcode 0xb8`, `dynamic class loading/Class.forName`, `closed-world reflection metadata`, and `planned milestone: reflection-and-metadata`.
- `AotUnsupportedLong` and `AotUnsupportedDouble`: assert class/method/descriptor/bci context, concrete constant opcodes, primitive feature names, and `planned milestone: primitive-completeness`.
- `AotUnsupportedMultiArray`: asserts class/method/descriptor/bci context, `opcode 0xc5`, `multidimensional arrays`, and `planned milestone: primitive-completeness`.
- `AotUnsupportedTableSwitch`: asserts class/method/descriptor/bci context, `opcode 0xaa`, `switch bytecodes`, and `planned milestone: primitive-completeness`.

## Verification Commands

```text
$ cargo fmt --check
exit 0
```

```text
$ cargo test unsupported -- --nocapture
exit 0
running 6 tests
test fvm_aot::tests::unsupported::unsupported_multidimensional_array_reports_required_feature_and_milestone ... ok
test fvm_aot::tests::unsupported::unsupported_athrow_reports_class_method_and_bci ... ok
test fvm_aot::tests::unsupported::unsupported_dynamic_class_loading_reports_required_feature_and_milestone ... ok
test fvm_aot::tests::unsupported::unsupported_tableswitch_reports_primitive_completeness_milestone ... ok
test fvm_aot::tests::unsupported::unsupported_lambda_reports_required_feature_and_milestone ... ok
test fvm_aot::tests::unsupported::unsupported_long_and_double_report_primitive_gap_and_milestone ... ok
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 28 filtered out
tests/cli_flow.rs filter match: unsupported_spring_shape_fails_native_build ... ok
```

```text
$ cargo test fvm_aot::tests -- --nocapture
exit 0
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 20 filtered out
```

```text
$ cargo test
exit 0
test result: ok. 34 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
tests/cli_flow.rs: 4 passed; 0 failed
```

```text
$ cargo clippy --all-targets -- -D warnings
exit 0
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.46s
```

```text
$ lsp_diagnostics src/fvm_aot/mod.rs
exit n/a
LSP daemon unreachable: daemon request timed out. Socket: /Users/scichocki/.codex/codex-lsp/daemon/v0.1.0/daemon.sock

$ lsp_diagnostics src/fvm_aot/tests/unsupported.rs
exit n/a
LSP daemon unreachable: daemon request timed out. Socket: /Users/scichocki/.codex/codex-lsp/daemon/v0.1.0/daemon.sock
```

```text
$ awk '!/^[[:space:]]*$/ && !/^[[:space:]]*(\/\/|#|--)/' src/fvm_aot/mod.rs | wc -l
exit 0
114

$ awk '!/^[[:space:]]*$/ && !/^[[:space:]]*(\/\/|#|--)/' src/fvm_aot/tests/unsupported.rs | wc -l
exit 0
224
```

## Cleanup And Scope

- Product behavior remains test-only: no compiler/runtime semantics, generated C, evaluator behavior, CLI behavior, examples, or docs changed.
- `src/fvm_aot/tests/current_slice.rs` stayed unchanged.
- The new unsupported module is 224 pure LOC, which is below the 250 pure LOC defect threshold but in the 200-250 warning band; it owns one short responsibility: golden unsupported diagnostics.
- Commit: pending.
