# T16 Evidence: closed-world reachability skeleton

Task: `src/fvm_aot/reachability.rs: Add closed-world entry graph skeleton - expect direct static helper discovered`

## Scope

- Added inert `src/fvm_aot/reachability.rs` closed-world graph skeleton.
- Added test module wiring in `src/fvm_aot/mod.rs` and `src/fvm_aot/tests/reachability.rs`.
- Preserved production `compile_jar`: it still reads the class world, calls evaluator `compile_main`, emits C with `emit_c`, and compiles via `cc`.
- Excluded unrelated dirty `.omo/evidence/task-10-fvm-aot-milestone-todos.md`, `.omo/evidence/task-13-fvm-aot-milestone-todos.md`, `.omo/evidence/task-14-fvm-aot-milestone-todos.md`, and `.omo/run-continuation/*.json` from staging.

## Command Evidence

### Required inspection

- `Read .omo/plans/fvm-aot-milestone-todos.md:254-260`: confirmed T16 requires main-entry graph, direct `invokestatic`, class initializers, descriptors, fields, stable snapshot, no production pipeline wiring.
- `Read docs/fvm-aot-graal-replacement-plan.md:166-192`: confirmed closed-world analyzer outputs classes, methods, fields, and unsupported feature rejections.
- `Read docs/fvm-aot-test-strategy.md:54-76`: confirmed stable graph snapshots and dynamic loading failure behavior.
- `Read src/fvm_aot/mod.rs`, `classfile.rs`, `types.rs`, `test_support.rs`, `tests/current_slice.rs`, `tests/unsupported.rs`: reused existing classfile member and descriptor helpers.
- `Grep Class.forName|invokestatic|descriptor|field in src/fvm_aot`: located evaluator dynamic-loading diagnostic, `method_ref`/`field_ref`, descriptor parsing, and existing static helper behavior.

### LSP diagnostics

- `lsp_diagnostics src/fvm_aot/reachability.rs`: failed, daemon request timed out at `/Users/scichocki/.codex/codex-lsp/daemon/v0.1.0/daemon.sock`.
- `lsp_diagnostics src/fvm_aot/tests/reachability.rs`: failed, rust server exited with `error: Unknown binary 'rust-analyzer' in official toolchain 'stable-aarch64-apple-darwin'.`
- `lsp_diagnostics src/fvm_aot/mod.rs`: failed, daemon request timed out at `/Users/scichocki/.codex/codex-lsp/daemon/v0.1.0/daemon.sock`.

### Targeted QA

- `cargo test reachability_direct_static_helper -- --nocapture`: exit 0. Snapshot excerpt:

```text
classes:
  AotReachability
methods:
  AotReachability.<clinit>()V
  AotReachability.helper()I
  AotReachability.main([Ljava/lang/String;)V
fields:
  AotReachability.seed:I
```

- `cargo test unsupported_dynamic_class_loading_reports_required_feature -- --nocapture`: exit 0. Existing unsupported dynamic class loading diagnostic test passed unchanged.

### Full QA

- `cargo fmt --check`: exit 0.
- `cargo test`: exit 0. Main suite reported `47 passed`; `tests/aot_firecracker.rs` kept its explicit ignored Linux/KVM gate; `tests/cli_flow.rs` reported `4 passed`.
- `cargo clippy --all-targets -- -D warnings`: exit 0.

### LOC and slop checks

- `awk '!/^[[:space:]]*$/ && !/^[[:space:]]*(\/\/|#|--)/' src/fvm_aot/reachability.rs | wc -l`: `241` pure LOC.
- `awk '!/^[[:space:]]*$/ && !/^[[:space:]]*(\/\/|#|--)/' src/fvm_aot/tests/reachability.rs | wc -l`: `117` pure LOC.
- `rg 'unwrap\(|expect\(|panic!|#!\[allow\(' src/fvm_aot/reachability.rs src/fvm_aot/tests/reachability.rs`: only `src/fvm_aot/reachability.rs:#![allow(dead_code)]`, intentionally scoped because T16 keeps the skeleton inert until T18 wires the compiler pipeline.

## Staging Scope

Intended staged files only:

- `.omo/evidence/task-16-fvm-aot-milestone-todos.md`
- `.omo/notepads/fvm-aot-milestone-todos/learnings.md`
- `src/fvm_aot/mod.rs`
- `src/fvm_aot/reachability.rs`
- `src/fvm_aot/tests/reachability.rs`

Commit hash: placeholder before commit.
