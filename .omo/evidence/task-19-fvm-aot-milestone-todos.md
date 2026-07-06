# Task 19 Evidence: Cranelift Object Backend Dependencies

Timestamp: 2026-07-06T08:00:00Z

## Scope

- Added the Cranelift object-backend family to `Cargo.toml` at one coherent version line: `cranelift-codegen`, `cranelift-frontend`, `cranelift-module`, and `cranelift-object` all at `0.133.1`.
- Added `target-lexicon = "0.13.5"` for later ISA/object-module APIs.
- Did not add LLVM or unrelated compiler backends.

## Version Checks

- `cargo search cranelift-object --limit 5` reported `cranelift-object = "0.133.1"`.
- `cargo search cranelift-codegen --limit 5` reported `cranelift-codegen = "0.133.1"`.
- `cargo info cranelift-object`, `cargo info cranelift-module`, `cargo info cranelift-frontend`, `cargo info cranelift-codegen`, and `cargo info target-lexicon` confirmed the selected crate versions before locking.

## Command Evidence

### Lockfile Resolution

Command: `cargo check`

Exit code: 0

Key output:

```text
Locking 27 packages to latest Rust 1.96.1 compatible versions
Adding cranelift-codegen v0.133.1
Adding cranelift-frontend v0.133.1
Adding cranelift-module v0.133.1
Adding cranelift-object v0.133.1
Adding target-lexicon v0.13.5
Finished `dev` profile [unoptimized + debuginfo] target(s) in 8.11s
```

### Targeted Compiler Pipeline Test

Command: `cargo test compiler_pipeline_lowers_simple_main -- --nocapture`

Exit code: 0

Key output:

```text
test fvm_aot::tests::compiler_pipeline::compiler_pipeline_lowers_simple_main ... ok
```

### Cranelift Dependency Tree

Command: `cargo tree -i cranelift-codegen`

Exit code: 0

Key output:

```text
cranelift-codegen v0.133.1
├── cranelift-frontend v0.133.1
├── cranelift-module v0.133.1
│   └── cranelift-object v0.133.1
└── cranelift-object v0.133.1 (*)
```

### Required Repo-Wide Verification

Commands:

```text
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

Exit code: 0 for all three commands.

## Notes

- No Rust source files were modified for this task.
- `.omo/run-continuation/*.json` remained untouched and unstaged.
