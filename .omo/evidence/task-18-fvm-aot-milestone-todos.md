# Task 18 Evidence: FVM AOT Compiler Pipeline Skeleton

Timestamp: 2026-07-06T04:03:24Z

## Scope

- Added inert/test-scoped `CompilerPipeline` in `src/fvm_aot/compiler.rs`.
- Added `src/fvm_aot/tests/compiler_pipeline.rs` coverage for simple reachable/lowered methods, current production output, and unsupported `long` diagnostics.
- Added minimal lowering support for static calls and `pop`, split branch/call helpers under `src/fvm_aot/lower/` to keep files under the 250 pure-LOC ceiling.
- Exposed deterministic reachable-method iteration from `src/fvm_aot/reachability.rs`.
- Kept production `compile_jar` output path unchanged: read class world -> `compile_main` evaluator -> `emit_c` -> `cc`.

## Discovery

- CodeGraph: no `codegraph_*` tools were present in the active tool namespace, so discovery continued immediately with `Read`, `rg`, `Glob`, and LSP as instructed.
- Required plan lines read: `.omo/plans/fvm-aot-milestone-todos.md:270` through `:276`.
- Required modules inspected: `mod.rs`, `reachability.rs`, `lower.rs`, `lower/*.rs`, `ir.rs`, `ir_verify.rs`, `tests/lower.rs`, `tests/reachability.rs`, `tests/ir_verify.rs`, and `test_support.rs`.
- Required grep/rg targets located: `compile_jar`, `read_class_world`, `compile_main`, `emit_c`, `analyze_main`, `lower_method_to_ir`, `FunctionIr::verify`, unsupported long diagnostics, and `current_slice` tests.

## Command Evidence

### Targeted Pipeline Happy Path

Command: `cargo test compiler_pipeline_lowers_simple_main -- --nocapture`

Exit code: 0

Excerpt:

```text
running 1 test
compiler_pipeline:
reachable:
classes:
  AotCompilerPipeline
methods:
  AotCompilerPipeline.helper(I)I
  AotCompilerPipeline.main([Ljava/lang/String;)V
fields:
lowered:
  AotCompilerPipeline.helper(I)I verified blocks=1
  AotCompilerPipeline.main([Ljava/lang/String;)V verified blocks=1
diagnostics:
  <none>
test fvm_aot::tests::compiler_pipeline::compiler_pipeline_lowers_simple_main ... ok
```

### Unsupported Long Pipeline Diagnostic

Command: `cargo test compiler_pipeline_reports_unsupported_long_before_codegen -- --nocapture`

Exit code: 0

Excerpt:

```text
diagnostics:
  phase=lower method=AotPipelineLong.main([Ljava/lang/String;)V message=fvm-aot lowerer bytecode error in AotPipelineLong.main([Ljava/lang/String;)V at bci 0 (opcode 0x0a): fvm-aot unsupported opcode 0x0a; required feature: long primitive bytecode; planned milestone: primitive-completeness
test fvm_aot::tests::compiler_pipeline::compiler_pipeline_reports_unsupported_long_before_codegen ... ok
```

### Required Combined Filter Attempt

Command: `cargo test compiler_pipeline current_slice -- --nocapture`

Exit code: 1

Excerpt:

```text
error: unexpected argument 'current_slice' found

Usage: cargo test [OPTIONS] [TESTNAME] [-- [ARGS]...]
```

Notes: This exact command is invalid Cargo CLI syntax before any test binary starts. The equivalent valid filters below passed.

### Valid Compiler Pipeline Filter

Command: `cargo test compiler_pipeline -- --nocapture`

Exit code: 0

Excerpt:

```text
running 3 tests
test fvm_aot::tests::compiler_pipeline::compiler_pipeline_reports_unsupported_long_before_codegen ... ok
test fvm_aot::tests::compiler_pipeline::compiler_pipeline_lowers_simple_main ... ok
test fvm_aot::tests::compiler_pipeline::compiler_pipeline_current_slice_keeps_compile_jar_output ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 53 filtered out
```

### Valid Current Slice Filter

Command: `cargo test current_slice -- --nocapture`

Exit code: 0

Excerpt:

```text
running 8 tests
test fvm_aot::tests::compiler_pipeline::compiler_pipeline_current_slice_keeps_compile_jar_output ... ok
test fvm_aot::tests::current_slice::compiles_static_fields_and_clinit_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_interface_dispatch_and_string_concat_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_multi_class_closed_world_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_computed_http_intrinsic_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_string_object_array_core_methods_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_objects_and_arrays_when_toolchain_is_available ... ok
test fvm_aot::tests::current_slice::compiles_simple_println_when_toolchain_is_available ... ok
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 48 filtered out
```

### Format, Full Test, Clippy

Command: `cargo fmt --check`

Exit code: 0

Excerpt: no output.

Command: `cargo test`

Exit code: 0

Excerpt:

```text
test result: ok. 56 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test aot_firecracker_smoke_requires_explicit_linux_kvm_gate ... ignored
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

Command: `cargo clippy --all-targets -- -D warnings`

Exit code: 0

Excerpt:

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.05s
```

## LSP Diagnostics

Attempted `lsp_diagnostics` on changed Rust files. Result: daemon timed out/unreachable.

Excerpt:

```text
LSP daemon unreachable: daemon request timed out.
Socket: /Users/scichocki/.codex/codex-lsp/daemon/v0.1.0/daemon.sock
```

Later attempts returned `MCP error -32001: Request timed out`. Cargo, rustfmt, and clippy were used as authoritative diagnostics.

## Pure LOC

Command:

```text
for file in src/fvm_aot/compiler.rs src/fvm_aot/mod.rs src/fvm_aot/reachability.rs src/fvm_aot/lower/method.rs src/fvm_aot/lower/calls.rs src/fvm_aot/lower/branches.rs src/fvm_aot/lower/state.rs src/fvm_aot/lower/bytecode.rs src/fvm_aot/tests/compiler_pipeline.rs; do count=$(awk '!/^[[:space:]]*$/ && !/^[[:space:]]*(\/\/|#|--)/' "$file" | wc -l | tr -d ' '); printf '%s %s\n' "$file" "$count"; done
```

Exit code: 0

Output:

```text
src/fvm_aot/compiler.rs 139
src/fvm_aot/mod.rs 126
src/fvm_aot/reachability.rs 246
src/fvm_aot/lower/method.rs 229
src/fvm_aot/lower/calls.rs 46
src/fvm_aot/lower/branches.rs 61
src/fvm_aot/lower/state.rs 150
src/fvm_aot/lower/bytecode.rs 234
src/fvm_aot/tests/compiler_pipeline.rs 136
```

## Grep / Slop Check

Command: `rg "unwrap\(|expect\(|panic!|todo!|unimplemented!" "src/fvm_aot/compiler.rs" "src/fvm_aot/tests/compiler_pipeline.rs" "src/fvm_aot/lower/method.rs" "src/fvm_aot/lower/calls.rs" "src/fvm_aot/lower/branches.rs" "src/fvm_aot/lower/state.rs" "src/fvm_aot/lower/bytecode.rs" "src/fvm_aot/reachability.rs"`

Exit code: 1

Output: no matches.

## Staging Scope

Intended staged files for the T18 commit only:

- `.omo/evidence/task-18-fvm-aot-milestone-todos.md`
- `.omo/notepads/fvm-aot-milestone-todos/learnings.md`
- `src/fvm_aot/compiler.rs`
- `src/fvm_aot/lower.rs`
- `src/fvm_aot/lower/branches.rs`
- `src/fvm_aot/lower/calls.rs`
- `src/fvm_aot/lower/bytecode.rs`
- `src/fvm_aot/lower/method.rs`
- `src/fvm_aot/lower/state.rs`
- `src/fvm_aot/mod.rs`
- `src/fvm_aot/reachability.rs`
- `src/fvm_aot/tests/compiler_pipeline.rs`

Explicitly excluded from staging: `.omo/run-continuation/*.json` and unrelated dirty `.omo` evidence/planning files from earlier tasks.

## Commit

Commit hash: pending before commit; update via `git rev-parse HEAD` after commit if needed.
