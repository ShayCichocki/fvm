# Task 14 Evidence: fvm-aot integer bytecode lowering

Task: T14. `src/fvm_aot/lower.rs: Lower int constants locals arithmetic returns to IR - expect HotSpot fixture IR stable`

Commit: pending final commit for `aot: lower integer bytecode to ir`. The final hash is recorded after the bounded commit because a commit cannot contain its own hash.

## Summary

- Added a private test-scoped `src/fvm_aot/lower.rs` lowerer for straight-line static integer methods.
- Lowered `iconst_m1..iconst_5`, `bipush`, `sipush`, integer `ldc`/`ldc_w`, `iload*`, `istore*`, `iadd`, `isub`, `imul`, `idiv`, `irem`, `ineg`, `iinc`, `ireturn`, and `return` to T13 IR.
- Added `Unary` and `ZeroCheck` IR instructions plus stable text rendering for params/constants/arithmetic/trap checks.
- `compile_jar` production behavior is unchanged: it still reads the class world, calls evaluator `compile_main`, and emits C through `emit_c`. The lowerer module is behind `#[cfg(test)]` and is not used by production output.

## Red Test

`cargo test lower_int_arithmetic_to_ir -- --nocapture` exit 101 before implementation.

```text
error[E0432]: unresolved import `crate::fvm_aot::lower`
 --> src/fvm_aot/tests/lower.rs:2:21
  |
2 | use crate::fvm_aot::lower::lower_method_to_ir;
  |                     ^^^^^ could not find `lower` in `fvm_aot`
error: could not compile `fvm` (bin "fvm" test) due to 1 previous error
```

## Final Verification Commands

`cargo test lower_int_arithmetic_to_ir -- --nocapture` exit 0.

```text
running 1 test
fn AotLowerInt.arithmetic(v0: int, v1: int) -> int {
bb0:
  param local0 = v0: int
  param local1 = v1: int
  v2 = add v0, v1
  v3 = const int 100
  v4 = add v2, v3
  v5 = const int 300
  v6 = add v4, v5
  v7 = const int 70000
  v8 = add v6, v7
  v9 = const int 3
  v10 = sub v8, v9
  v11 = const int 2
  v12 = mul v10, v11
  check_nonzero v1 else trap divide_by_zero
  v13 = div v12, v1
  v14 = const int 5
  check_nonzero v14 else trap divide_by_zero
  v15 = rem v13, v14
  v16 = neg v15
  v17 = const int 1
  v18 = add v16, v17
  return v18
}
test fvm_aot::tests::lower::lower_int_arithmetic_to_ir ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 42 filtered out
```

`cargo test lower_ -- --nocapture` exit 0.

```text
running 2 tests
test fvm_aot::tests::lower::lower_unsupported_long_reports_primitive_completeness ... ok
test fvm_aot::tests::lower::lower_int_arithmetic_to_ir ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 41 filtered out
```

`cargo fmt --check` exit 0.

```text
<no output>
```

`cargo test` exit 0.

```text
test result: ok. 43 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

`cargo clippy --all-targets -- -D warnings` exit 0.

```text
Checking fvm v0.1.0 (/Users/scichocki/personal/fvm)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.54s
```

## LSP Diagnostics

Attempted on `src/fvm_aot/lower.rs`, `src/fvm_aot/ir.rs`, `src/fvm_aot/mod.rs`, and `src/fvm_aot/tests/lower.rs`.

```text
LSP server rust at /Users/scichocki/personal/fvm exited with code 1
stderr tail: error: Unknown binary 'rust-analyzer' in official toolchain 'stable-aarch64-apple-darwin'.

LSP daemon unreachable: daemon request timed out.
The MCP server is a thin proxy and never runs language servers in-process.
Socket: /Users/scichocki/.codex/codex-lsp/daemon/v0.1.0/daemon.sock
Logs: /Users/scichocki/.codex/codex-lsp/daemon/v0.1.0/daemon.log

MCP error -32001: Request timed out
```

LSP is unavailable, not treated as a pass. `cargo fmt --check`, `cargo test`, and `cargo clippy --all-targets -- -D warnings` are the reliable diagnostics for this task.

## Grep And Slop Check

Command:

```text
rg -n 'TODO|FIXME|HACK|xxx|unwrap\(|expect\(|panic!|#\[allow\]|#!\[allow\]|\bas\s' src/fvm_aot/lower.rs src/fvm_aot/ir.rs src/fvm_aot/mod.rs src/fvm_aot/tests/lower.rs
```

Exit 1 with no matches.

Stricter allowance scan:

```text
src/fvm_aot/ir.rs:1:#![allow(dead_code)]
```

Justification: this is the inherited T13 scoped allowance for the model-only IR surface until T14-T18 wire production users. No new `unwrap(`, `expect(`, `panic!`, TODO/FIXME/HACK/xxx marker, or `as ` cast was added in the changed Rust files.

## Pure LOC

Measured with `awk '!/^[[:space:]]*$/ && !/^[[:space:]]*(\/\/|#|--)/' <file> | wc -l`.

- `src/fvm_aot/lower.rs`: 269 pure LOC. Marked `SIZE_OK` because T14 owns one straight-line opcode lowering state machine; splitting before T15 would separate opcode flow without a stable branch-lowering boundary.
- `src/fvm_aot/ir.rs`: 277 pure LOC. Marked `SIZE_OK` because it remains the single T13 runtime compiler IR model; T17 owns verifier extraction.
- `src/fvm_aot/mod.rs`: 120 pure LOC.
- `src/fvm_aot/tests/lower.rs`: 113 pure LOC.

## Manual QA

- The happy-path lowerer test compiles a Java fixture with `javac`, runs the fixture on HotSpot with `java`, parses the resulting `.class`, lowers `static int arithmetic(int,int)`, prints deterministic IR, and asserts exact text including params, add, return, and divide-by-zero nonzero checks.
- The failure QA fixture compiles Java `long` bytecode, lowers the parsed classfile method, and asserts the error contains `opcode 0x0a`, `required feature: long primitive bytecode`, `planned milestone: primitive-completeness`, and method context.
- `codegraph_explore` was not available in the tool namespace; codebase impact was mapped through required reads, `rg`, and two background explore agents.

## Staging Scope

Intended T14 files only:

- `src/fvm_aot/lower.rs`
- `src/fvm_aot/ir.rs`
- `src/fvm_aot/mod.rs`
- `src/fvm_aot/tests/lower.rs`
- `.omo/evidence/task-14-fvm-aot-milestone-todos.md`
- `.omo/notepads/fvm-aot-milestone-todos/learnings.md`

Excluded from staging: `.omo/run-continuation/*.json`, pre-existing `.omo/evidence/task-10...`, pre-existing `.omo/evidence/task-13...`, and unrelated local `.omo` state.
