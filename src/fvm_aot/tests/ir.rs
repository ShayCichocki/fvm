use crate::fvm_aot::ir::{BasicBlockId, BasicBlockIr, FunctionIr, IrInstr, IrType};

#[test]
fn ir_empty_main_snapshot() {
    let function = FunctionIr {
        name: "main".to_string(),
        params: Vec::new(),
        return_type: IrType::Void,
        blocks: vec![BasicBlockIr {
            id: BasicBlockId::new(0),
            instrs: vec![IrInstr::Return(None)],
        }],
    };

    assert_eq!(
        function.render_text(),
        "fn main -> void {\nbb0:\n  return\n}\n"
    );
    assert!(function.verify().is_ok());
}

#[test]
fn ir_rejects_invalid_branch_target() {
    let function = FunctionIr {
        name: "bad_branch".to_string(),
        params: Vec::new(),
        return_type: IrType::Void,
        blocks: vec![BasicBlockIr {
            id: BasicBlockId::new(0),
            instrs: vec![IrInstr::Branch(BasicBlockId::new(9))],
        }],
    };

    let message = function.verify().err().map(|err| err.to_string());
    assert_eq!(
        message.as_deref(),
        Some("IR function `bad_branch` branches from bb0 to missing target bb9")
    );
}
