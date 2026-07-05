# Task 15 Evidence: fvm-aot branch/basic-block lowering

Task: T15. `src/fvm_aot/lower.rs: Lower branches and basic blocks to IR - expect loop fixture has explicit edges`

Commit message: `aot: lower branches into ir blocks`. The final commit hash is reported in the completion report because a commit cannot contain its own hash without a follow-up amend.

## Summary

- Split the test-scoped lowerer into `src/fvm_aot/lower/{bytecode,metadata,method,state}.rs` while preserving `crate::fvm_aot::lower::lower_method_to_ir`.
- Added bytecode block planning for branch leaders, fallthroughs, `goto`, and target validation before IR success.
- Lowered `ifeq`..`ifle`, `if_icmpeq`..`if_icmple`, `if_acmpeq`/`if_acmpne`, `ifnull`/`ifnonnull`, and `goto` to explicit `branch`/`branch_if` terminators.
- Added deterministic IR successor-list rendering and typed compare IR, including explicit reference/null placeholder compare ops.
- `compile_jar` production behavior is unchanged: `src/fvm_aot/mod.rs` was not modified, and the lowerer remains behind the existing `#[cfg(test)] mod lower` wiring.

## Red Test

`cargo test lower_branches_to_blocks -- --nocapture` exit 101 before implementation.

```text
running 1 test
Error: fvm-aot lowerer bytecode error in AotLowerBranches.loop(I)I at bci 6 (opcode 0xa2)

Caused by:
    fvm-aot unsupported opcode 0xa2; supported subset includes int-compatible locals/arithmetic/branches, same-class objects/static helpers/fields, arrays, core String/Object intrinsics, println, and Http.respond
test fvm_aot::tests::lower::lower_branches_to_blocks ... FAILED
```

## Final Verification Commands

`cargo test lower_branches_to_blocks -- --nocapture` exit 0.

```text
running 1 test
fn AotLowerBranches.loop(v0: int) -> int {
bb0 -> [bb1]:
...
bb1 -> [bb6, bb2]:
...
bb5 -> [bb1]:
...
}

fn AotLowerBranches.isNull(v0: ref java/lang/Object) -> int {
bb0 -> [bb2, bb1]:
  param local0 = v0: ref java/lang/Object
  v1 = cmp_ref_is_non_null_placeholder v0
  branch_if v1, bb2, bb1
...
}
test fvm_aot::tests::lower::lower_branches_to_blocks ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 44 filtered out
```

`cargo test lower_int_arithmetic_to_ir -- --nocapture` exit 0.

```text
running 1 test
fn AotLowerInt.arithmetic(v0: int, v1: int) -> int {
bb0 -> []:
  param local0 = v0: int
  param local1 = v1: int
  v2 = add v0, v1
...
  return v18
}
test fvm_aot::tests::lower::lower_int_arithmetic_to_ir ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 44 filtered out
```

`cargo test lower_ -- --nocapture` exit 0.

```text
running 4 tests
test fvm_aot::tests::lower::lower_rejects_branch_target_out_of_range ... ok
test fvm_aot::tests::lower::lower_unsupported_long_reports_primitive_completeness ... ok
test fvm_aot::tests::lower::lower_int_arithmetic_to_ir ... ok
test fvm_aot::tests::lower::lower_branches_to_blocks ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 41 filtered out
```

`cargo fmt --check` exit 0.

```text
<no output>
```

`cargo test` exit 0.

```text
test result: ok. 45 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

`cargo clippy --all-targets -- -D warnings` exit 0.

```text
Checking fvm v0.1.0 (/Users/scichocki/personal/fvm)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.68s
```

## LSP Diagnostics

Attempted on changed Rust files: `src/fvm_aot/lower.rs`, `src/fvm_aot/lower/bytecode.rs`, `src/fvm_aot/lower/metadata.rs`, `src/fvm_aot/lower/method.rs`, `src/fvm_aot/lower/state.rs`, `src/fvm_aot/ir.rs`, `src/fvm_aot/ir/display.rs`, `src/fvm_aot/tests/lower.rs`, and `src/fvm_aot/tests/ir.rs`.

```text
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
rg -n 'TODO|FIXME|HACK|xxx|unwrap\(|expect\(|panic!|#\[allow\]|#!\[allow\]|\bas\s' src/fvm_aot/lower.rs src/fvm_aot/lower/bytecode.rs src/fvm_aot/lower/metadata.rs src/fvm_aot/lower/method.rs src/fvm_aot/lower/state.rs src/fvm_aot/ir.rs src/fvm_aot/ir/display.rs src/fvm_aot/tests/lower.rs src/fvm_aot/tests/ir.rs
```

Exit 1 with no matches.

Stricter allowance scan:

```text
src/fvm_aot/ir.rs:1:#![allow(dead_code)]
```

Justification: this is the inherited T13 scoped allowance for the model-only IR surface until T14-T18 wire production users. No new `unwrap(`, `expect(`, `panic!`, TODO/FIXME/HACK/xxx marker, or `as ` cast was added in the changed Rust files.

## Pure LOC

Measured with `awk '!/^[[:space:]]*$/ && !/^[[:space:]]*(\/\/|#|--)/' <file> | wc -l`.

- `src/fvm_aot/lower.rs`: 5 pure LOC.
- `src/fvm_aot/lower/bytecode.rs`: 229 pure LOC.
- `src/fvm_aot/lower/metadata.rs`: 43 pure LOC.
- `src/fvm_aot/lower/method.rs`: 249 pure LOC.
- `src/fvm_aot/lower/state.rs`: 134 pure LOC.
- `src/fvm_aot/ir.rs`: 169 pure LOC.
- `src/fvm_aot/ir/display.rs`: 187 pure LOC.
- `src/fvm_aot/tests/lower.rs`: 242 pure LOC.
- `src/fvm_aot/tests/ir.rs`: 33 pure LOC.

## Manual QA

- The happy-path lowerer test compiles `AotLowerBranches` with `javac`, runs it on HotSpot, confirms `loop(4)` prints `-1`, parses the `.class`, lowers `loop(I)I`, and asserts exact block successor headers for seven deterministic blocks including the loop back edge `bb5 -> [bb1]:`.
- The same fixture lowers `isNull(Ljava/lang/Object;)I` and asserts explicit reference placeholder rendering through `cmp_ref_is_non_null_placeholder`.
- The failure QA compiles `AotLowerBadBranch`, corrupts the branch offset to `i16::MAX`, and asserts lowering fails before IR success with method, bci/opcode, branch target, and out-of-range context.
- `codegraph_explore` was not available in the tool namespace; codebase impact was mapped through required reads, `rg`, LSP attempts, and cargo/rustfmt/clippy verification.

## Staging Scope

Intended T15 files only:

- `src/fvm_aot/lower.rs`
- `src/fvm_aot/lower/bytecode.rs`
- `src/fvm_aot/lower/metadata.rs`
- `src/fvm_aot/lower/method.rs`
- `src/fvm_aot/lower/state.rs`
- `src/fvm_aot/ir.rs`
- `src/fvm_aot/ir/display.rs`
- `src/fvm_aot/tests/lower.rs`
- `src/fvm_aot/tests/ir.rs`
- `.omo/evidence/task-15-fvm-aot-milestone-todos.md`
- `.omo/notepads/fvm-aot-milestone-todos/learnings.md`

Excluded from staging: `.omo/run-continuation/*.json`, pre-existing `.omo/evidence/task-10...`, pre-existing `.omo/evidence/task-13...`, pre-existing `.omo/evidence/task-14...`, and unrelated local `.omo` state.
