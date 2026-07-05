# T13 Evidence: Runtime Compiler IR Model

Task: `src/fvm_aot/ir.rs: Add typed IR data model for runtime compiler - expect snapshot test for empty main IR`

## Changed Files

- `src/fvm_aot/ir.rs`
- `src/fvm_aot/mod.rs`
- `src/fvm_aot/tests/ir.rs`
- `.omo/evidence/task-13-fvm-aot-milestone-todos.md`
- `.omo/notepads/fvm-aot-milestone-todos/learnings.md`

No evaluator, emitter, generated C, Cranelift, LLVM, bytecode lowering, runtime codegen, reachability, `compile_jar`, `scripts/perf-aot`, or `scripts/perf-native` behavior was changed. `.omo/run-continuation/*.json` files were not staged, deleted, or used as evidence.

## Implementation Notes

- Added `src/fvm_aot/ir.rs` as a model-only sibling module.
- Added typed `FunctionIr`, `BasicBlockId`, `ValueId`, `IrType`, and `IrInstr` with operation coverage for params, constants, arithmetic, branches, direct calls, returns, field operations, array operations, allocation, null checks, bounds checks, exception edges, traps, and runtime helper calls.
- `FunctionIr::render_text()` provides a stable text form. The empty `main` snapshot is exactly:

```text
fn main -> void {
bb0:
  return
}
```

- `FunctionIr::verify()` rejects branch, conditional branch, and exception-edge targets that are not present in the function block list.
- Unit tests live in `src/fvm_aot/tests/ir.rs` to keep `src/fvm_aot/ir.rs` below the 250 pure-LOC ceiling.
- `#![allow(dead_code)]` is scoped to `ir.rs` because T13 intentionally lands the IR model before T14-T18 wire lowering/codegen users; `cargo clippy --all-targets -- -D warnings` would otherwise reject the unused model surface.
- No production codegen success is claimed by this task.

## Required Reading And Tool Availability

- Read `src/fvm_aot/mod.rs`, `src/fvm_aot/types.rs`, `Cargo.toml`, existing `src/fvm_aot/tests/*` modules, `.omo/notepads/fvm-aot-milestone-todos/*.md`, `.omo/plans/fvm-aot-milestone-todos.md:230-236`, `docs/fvm-aot-graal-replacement-plan.md:193-222`, `docs/fvm-aot-graal-replacement-plan.md:938-959`, and `docs/fvm-aot-test-strategy.md:78-101`.
- No `AGENTS.md` files were present under `/Users/scichocki/personal/fvm`.
- No `codegraph_explore` tool was exposed in this environment, so this used direct Read/rg/LSP/cargo inspection.

## Command Transcripts

### Red Test

Command:

```bash
cargo test ir_empty_main_snapshot -- --nocapture
```

Output excerpt:

```text
error[E0422]: cannot find struct, variant or union type `FunctionIr` in this scope
error[E0433]: cannot find type `IrType` in this scope
error[E0433]: cannot find type `IrInstr` in this scope
```

Observed result: failed for the expected missing IR model definitions before implementation.

### `cargo test ir_empty_main_snapshot -- --nocapture`

Command:

```bash
cargo test ir_empty_main_snapshot -- --nocapture; rc=$?; printf 'exit=%s\n' "$rc"; exit "$rc"
```

Output excerpt:

```text
running 1 test
test fvm_aot::tests::ir::ir_empty_main_snapshot ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 40 filtered out; finished in 0.00s
exit=0
```

Exit code: 0

### `cargo test ir_ -- --nocapture`

Command:

```bash
cargo test ir_ -- --nocapture; rc=$?; printf 'exit=%s\n' "$rc"; exit "$rc"
```

Output excerpt:

```text
running 3 tests
test fvm_aot::tests::ir::ir_rejects_invalid_branch_target ... ok
test fvm_aot::tests::ir::ir_empty_main_snapshot ... ok
test fvm_aot::test_support::tests::keep_artifacts_returns_tempdir_path ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 38 filtered out; finished in 0.00s
exit=0
```

Exit code: 0

### `cargo fmt --check`

Command:

```bash
cargo fmt --check; rc=$?; printf 'exit=%s\n' "$rc"; exit "$rc"
```

Output:

```text
exit=0
```

Exit code: 0

### `cargo test`

Command:

```bash
cargo test; rc=$?; printf 'exit=%s\n' "$rc"; exit "$rc"
```

Output excerpt:

```text
running 41 tests
...
test result: ok. 41 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.70s

running 1 test
test aot_firecracker_smoke_requires_explicit_linux_kvm_gate ... ignored
test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s

running 4 tests
test unsupported_spring_shape_fails_native_build ... ok
test doctor_non_strict_is_laptop_safe ... ok
test fvm_aot_dry_run_builds_supported_println_subset ... ok
test dry_run_artifact_lifecycle_works ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.43s
exit=0
```

Exit code: 0

### `cargo clippy --all-targets -- -D warnings`

Command:

```bash
cargo clippy --all-targets -- -D warnings; rc=$?; printf 'exit=%s\n' "$rc"; exit "$rc"
```

Output:

```text
Checking fvm v0.1.0 (/Users/scichocki/personal/fvm)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.48s
exit=0
```

Exit code: 0

## LSP Diagnostics

Attempted for `src/fvm_aot/mod.rs`, `src/fvm_aot/ir.rs`, and `src/fvm_aot/tests/ir.rs`.

Result:

```text
LSP daemon unreachable: daemon request timed out.
Socket: /Users/scichocki/.codex/codex-lsp/daemon/v0.1.0/daemon.sock
MCP error -32001: Request timed out
```

LSP diagnostics were unavailable and are not claimed as passing. Cargo, rustfmt, and clippy are the effective diagnostics.

## Pure LOC

Commands:

```bash
awk '!/^[[:space:]]*$/ && !/^[[:space:]]*(\/\/|#|--)/' "src/fvm_aot/ir.rs" | wc -l; rc=$?; printf 'exit=%s\n' "$rc"; exit "$rc"
awk '!/^[[:space:]]*$/ && !/^[[:space:]]*(\/\/|#|--)/' "src/fvm_aot/tests/ir.rs" | wc -l; rc=$?; printf 'exit=%s\n' "$rc"; exit "$rc"
awk '!/^[[:space:]]*$/ && !/^[[:space:]]*(\/\/|#|--)/' "src/fvm_aot/mod.rs" | wc -l; rc=$?; printf 'exit=%s\n' "$rc"; exit "$rc"
```

Results:

```text
src/fvm_aot/ir.rs: 226, exit=0
src/fvm_aot/tests/ir.rs: 33, exit=0
src/fvm_aot/mod.rs: 118, exit=0
```

## Stub/Slop Grep

Required search command:

```bash
rg -n "TODO|FIXME|HACK|xxx|unwrap\(|expect\(|panic!|#\[allow\]|as " "src/fvm_aot/ir.rs" "src/fvm_aot/mod.rs" "src/fvm_aot/tests/ir.rs"; rc=$?; printf 'exit=%s\n' "$rc"; if [ "$rc" -eq 1 ]; then exit 0; else exit "$rc"; fi
```

Output:

```text
exit=1
```

Interpretation: no matches for that literal pattern; raw `rg` exit 1 means no matches and the wrapper exited successfully.

Expanded inner-attribute allow search:

```bash
rg -n "TODO|FIXME|HACK|xxx|unwrap\(|expect\(|panic!|#!?\[allow|as " "src/fvm_aot/ir.rs" "src/fvm_aot/mod.rs" "src/fvm_aot/tests/ir.rs"; rc=$?; printf 'exit=%s\n' "$rc"; if [ "$rc" -eq 1 ]; then exit 0; else exit "$rc"; fi
```

Output:

```text
src/fvm_aot/ir.rs:1:#![allow(dead_code)]
exit=0
```

Justification: this is a narrow model-only milestone allowance so T13 can land before T14-T18 wire actual users. There are no TODO/FIXME/HACK/xxx markers, no `unwrap(`/`expect(`/`panic!`, and no `as ` casts in the changed Rust files.

## Commit

- Commit hash: `PENDING`
- Message: `aot: add runtime compiler ir model`
