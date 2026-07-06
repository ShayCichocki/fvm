use crate::fvm_aot::ir::{
    BasicBlockId, BasicBlockIr, FunctionIr, IrArithmeticOp, IrConst, IrInstr, IrParam, IrType,
    ValueId,
};

#[test]
fn ir_verify_accepts_valid_lowered_order() {
    let function = FunctionIr {
        name: "Verifier.valid".to_string(),
        descriptor: "(I)I".to_string(),
        params: vec![IrParam {
            value: ValueId::new(0),
            ty: IrType::Int,
        }],
        return_type: IrType::Int,
        blocks: vec![BasicBlockIr {
            id: BasicBlockId::new(0),
            instrs: vec![
                IrInstr::Param(ValueId::new(0), 0, IrType::Int),
                IrInstr::Constant(ValueId::new(1), IrConst::Int(1)),
                IrInstr::Arithmetic(
                    ValueId::new(2),
                    IrArithmeticOp::Add,
                    ValueId::new(0),
                    ValueId::new(1),
                ),
                IrInstr::Return(Some(ValueId::new(2))),
            ],
        }],
    };

    assert!(function.verify().is_ok());
}

#[test]
fn ir_verify_rejects_missing_branch_target() {
    let function = void_function(
        "Verifier.badBranch",
        vec![IrInstr::Branch(BasicBlockId::new(7))],
    );

    assert_error_contains(
        &function,
        &[
            "Verifier.badBranch()V",
            "branches from bb0",
            "missing target bb7",
        ],
    );
}

#[test]
fn ir_verify_rejects_use_before_definition() {
    let function = int_function(
        "Verifier.useBeforeDef",
        vec![
            IrInstr::Constant(ValueId::new(0), IrConst::Int(1)),
            IrInstr::Arithmetic(
                ValueId::new(1),
                IrArithmeticOp::Add,
                ValueId::new(9),
                ValueId::new(0),
            ),
            IrInstr::Return(Some(ValueId::new(1))),
        ],
    );

    assert_error_contains(
        &function,
        &["Verifier.useBeforeDef()I", "uses v9", "before definition"],
    );
}

#[test]
fn ir_verify_rejects_return_type_mismatch_with_descriptor() {
    let function = FunctionIr {
        name: "Verifier.returnMismatch".to_string(),
        descriptor: "()I".to_string(),
        params: Vec::new(),
        return_type: IrType::Int,
        blocks: vec![BasicBlockIr {
            id: BasicBlockId::new(0),
            instrs: vec![IrInstr::Return(None)],
        }],
    };

    assert_error_contains(
        &function,
        &[
            "Verifier.returnMismatch()I",
            "return type mismatch",
            "expected int",
            "returned void",
        ],
    );
}

#[test]
fn ir_verify_rejects_descriptor_model_return_mismatch() {
    let function = FunctionIr {
        name: "Verifier.descriptorMismatch".to_string(),
        descriptor: "()I".to_string(),
        params: Vec::new(),
        return_type: IrType::Void,
        blocks: vec![BasicBlockIr {
            id: BasicBlockId::new(0),
            instrs: vec![IrInstr::Return(None)],
        }],
    };

    assert_error_contains(
        &function,
        &[
            "Verifier.descriptorMismatch()I",
            "descriptor return type mismatch",
            "descriptor int",
            "modeled void",
        ],
    );
}

#[test]
fn ir_verify_rejects_unsupported_type() {
    let function = FunctionIr {
        name: "Verifier.unsupported".to_string(),
        descriptor: "()J".to_string(),
        params: Vec::new(),
        return_type: IrType::Unsupported("J".to_string()),
        blocks: vec![BasicBlockIr {
            id: BasicBlockId::new(0),
            instrs: vec![IrInstr::Return(None)],
        }],
    };

    assert_error_contains(
        &function,
        &[
            "Verifier.unsupported()J",
            "unsupported return type",
            "unsupported<J>",
        ],
    );
}

fn void_function(name: &str, instrs: Vec<IrInstr>) -> FunctionIr {
    FunctionIr {
        name: name.to_string(),
        descriptor: "()V".to_string(),
        params: Vec::new(),
        return_type: IrType::Void,
        blocks: vec![BasicBlockIr {
            id: BasicBlockId::new(0),
            instrs,
        }],
    }
}

fn int_function(name: &str, instrs: Vec<IrInstr>) -> FunctionIr {
    FunctionIr {
        name: name.to_string(),
        descriptor: "()I".to_string(),
        params: Vec::new(),
        return_type: IrType::Int,
        blocks: vec![BasicBlockIr {
            id: BasicBlockId::new(0),
            instrs,
        }],
    }
}

fn assert_error_contains(function: &FunctionIr, expected: &[&str]) {
    let err = match function.verify() {
        Ok(()) => panic!("IR verified unexpectedly:\n{}", function.render_text()),
        Err(err) => err,
    };
    let message = err.to_string();
    println!("{message}");
    for expected in expected {
        assert!(
            message.contains(expected),
            "verifier error did not contain `{expected}`:\n{message}"
        );
    }
}
