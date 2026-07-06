# T17 Evidence: fvm-aot IR verifier

Task: `src/fvm_aot/ir_verify.rs: Add IR verifier for types stack and block edges`

Commit hash: pending before commit.

## Scope

- Added `src/fvm_aot/ir_verify.rs` with `FunctionIr::verify()` delegating to a real verifier.
- Added helper submodules `src/fvm_aot/ir_verify/types.rs` and `src/fvm_aot/ir_verify/values.rs` to keep each Rust file under the 250 pure-LOC ceiling.
- Added descriptor plumbing on `FunctionIr` so verifier diagnostics can name `function + descriptor`.
- Added `src/fvm_aot/tests/ir_verify.rs` with one valid function and four invalid fixtures.
- Left production `compile_jar` behavior unchanged; verifier remains reachable through `FunctionIr::verify()` and current test-scoped lowering.

## Command Evidence

- `GIT_MASTER=1 git status --short --untracked-files=all`: exit 0. Existing unrelated dirty `.omo/evidence/task-10*`, `task-13*`, `task-14*`, many untracked `.omo/run-continuation/*.json`, and other `.omo` planning files were present before staging and are excluded from T17 staging.
- `cargo test ir_verify -- --nocapture` before implementation: exit 101. Red run had 1 valid fixture pass and 4 invalid fixtures fail because the stub verifier returned `Ok(())`.
- `cargo fmt`: exit 0.
- `cargo fmt --check`: exit 0.
- `cargo test ir_verify -- --nocapture`: exit 0. Output included 5 tests; invalid diagnostics included `Verifier.badBranch()V`, `Verifier.useBeforeDef()I`, `Verifier.returnMismatch()I`, and `Verifier.unsupported()J`.
- `cargo test ir_ -- --nocapture`: exit 0. Output included existing IR model tests plus new verifier tests.
- `cargo test`: exit 0. Output included 52 unit tests passing, 1 ignored Firecracker smoke test, and 4 CLI integration tests passing.
- `cargo clippy --all-targets -- -D warnings`: exit 0.
- LSP diagnostics: attempted for `src/fvm_aot/ir_verify.rs`, `src/fvm_aot/ir_verify/types.rs`, `src/fvm_aot/ir_verify/values.rs`, `src/fvm_aot/tests/ir_verify.rs`, `src/fvm_aot/ir.rs`, `src/fvm_aot/mod.rs`, `src/fvm_aot/lower/method.rs`, and `src/fvm_aot/tests/ir.rs`; every request timed out at `/Users/scichocki/.codex/codex-lsp/daemon/v0.1.0/daemon.sock` or returned `MCP error -32001: Request timed out`.
- Production slop grep: `rg -n "unwrap\(|expect\(|panic!|todo!|unimplemented!" src/fvm_aot/ir_verify.rs src/fvm_aot/ir_verify/types.rs src/fvm_aot/ir.rs src/fvm_aot/lower/method.rs src/fvm_aot/mod.rs`: exit 1 with no matches.

## Pure LOC

- `src/fvm_aot/ir_verify.rs`: 241
- `src/fvm_aot/ir_verify/types.rs`: 72
- `src/fvm_aot/ir_verify/values.rs`: 51
- `src/fvm_aot/ir.rs`: 122
- `src/fvm_aot/lower/method.rs`: 250
- `src/fvm_aot/mod.rs`: 124
- `src/fvm_aot/tests/ir.rs`: 35
- `src/fvm_aot/tests/ir_verify.rs`: 142

## Staging Scope

Stage only T17 files:

- `.omo/evidence/task-17-fvm-aot-milestone-todos.md`
- `.omo/notepads/fvm-aot-milestone-todos/learnings.md`
- `src/fvm_aot/ir.rs`
- `src/fvm_aot/ir_verify.rs`
- `src/fvm_aot/ir_verify/types.rs`
- `src/fvm_aot/ir_verify/values.rs`
- `src/fvm_aot/lower/method.rs`
- `src/fvm_aot/mod.rs`
- `src/fvm_aot/tests/ir.rs`
- `src/fvm_aot/tests/ir_verify.rs`

Do not stage `.omo/run-continuation/*.json` or unrelated dirty `.omo` evidence/planning files.
