use crate::fvm_aot::codegen::cranelift::emit_object;
use crate::fvm_aot::ir::{
    BasicBlockId, BasicBlockIr, FunctionIr, IrConst, IrInstr, IrType, ValueId,
};
use crate::fvm_aot::link::{LinkSpec, link_cranelift_object_with_runtime_stub as link_object};
use crate::fvm_aot::test_support::command_available;
use std::process::Command;

#[test]
fn link_cranelift_object_with_runtime_stub_runs_native_executable() -> anyhow::Result<()> {
    if !command_available("cc") {
        println!("skipping Cranelift object linker test because required tool is missing: cc");
        return Ok(());
    }

    let temp = tempfile::tempdir()?;
    let executable_path = temp.path().join("linked-aot-main");
    let object = emit_object(&int_returning_main())?;

    let linked = link_object(&LinkSpec {
        cc: "cc",
        object_bytes: &object,
        entry_symbol: "fvm_aot_main",
        output_path: &executable_path,
    })?;

    let output = Command::new(linked.path()).output()?;

    assert!(
        output.status.success(),
        "linked executable failed: status={} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[test]
fn link_reports_missing_configured_cc() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let executable_path = temp.path().join("linked-aot-main");
    let missing_cc = "fvm-aot-missing-cc-for-t22";
    let object = emit_object(&int_returning_main())?;

    let message = link_object(&LinkSpec {
        cc: missing_cc,
        object_bytes: &object,
        entry_symbol: "fvm_aot_main",
        output_path: &executable_path,
    })
    .unwrap_err()
    .to_string();

    assert!(message.contains("failed to execute"), "{message}");
    assert!(message.contains(missing_cc), "{message}");
    Ok(())
}

fn int_returning_main() -> FunctionIr {
    FunctionIr {
        name: "main".to_string(),
        descriptor: "()I".to_string(),
        params: Vec::new(),
        return_type: IrType::Int,
        blocks: vec![BasicBlockIr {
            id: BasicBlockId::new(0),
            instrs: vec![
                IrInstr::Constant(ValueId::new(0), IrConst::Int(0)),
                IrInstr::Return(Some(ValueId::new(0))),
            ],
        }],
    }
}
