use crate::fvm_aot::codegen::cranelift::emit_object;
use crate::fvm_aot::ir::{
    BasicBlockId, BasicBlockIr, FunctionIr, IrArithmeticOp, IrConst, IrInstr, IrType, ValueId,
};
use cranelift_object::object::{Object, ObjectSymbol};

#[test]
fn cranelift_emits_constant_return_object() -> anyhow::Result<()> {
    let function = int_function(
        "Codegen.constantReturn",
        vec![
            IrInstr::Constant(ValueId::new(0), IrConst::Int(42)),
            IrInstr::Return(Some(ValueId::new(0))),
        ],
    );

    let object = emit_object(&function)?;
    let parsed = cranelift_object::object::File::parse(object.as_slice())?;

    let symbols = parsed
        .symbols()
        .filter_map(|symbol| symbol.name().ok().map(str::to_string))
        .collect::<Vec<_>>();

    assert!(
        symbols
            .iter()
            .any(|symbol| symbol.strip_prefix('_').unwrap_or(symbol)
                == "fvm_aot_Codegen_constantReturn"),
        "expected exported symbol fvm_aot_Codegen_constantReturn in object symbols {symbols:?}"
    );
    Ok(())
}

#[test]
fn cranelift_rejects_unsupported_instruction() {
    let function = int_function(
        "Codegen.unsupportedArithmetic",
        vec![
            IrInstr::Constant(ValueId::new(0), IrConst::Int(40)),
            IrInstr::Constant(ValueId::new(1), IrConst::Int(2)),
            IrInstr::Arithmetic(
                ValueId::new(2),
                IrArithmeticOp::Add,
                ValueId::new(0),
                ValueId::new(1),
            ),
            IrInstr::Return(Some(ValueId::new(2))),
        ],
    );

    let message = emit_object(&function).unwrap_err().to_string();
    println!("{message}");

    assert!(message.contains("unsupported-codegen"));
    assert!(message.contains("Codegen.unsupportedArithmetic()I"));
    assert!(message.contains("instruction=Arithmetic"));
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
