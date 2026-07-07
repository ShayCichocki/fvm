use crate::fvm_aot::codegen::cranelift::emit_object;
use crate::fvm_aot::ir::{
    BasicBlockId, BasicBlockIr, FunctionIr, IrConst, IrInstr, IrType, ValueId,
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

    // The descriptor `()I` is hex-escaped into the symbol so overloads stay
    // distinct: `(` -> _28, `)` -> _29.
    assert!(
        symbols
            .iter()
            .any(|symbol| symbol.strip_prefix('_').unwrap_or(symbol)
                == "fvm_aot_Codegen_constantReturn_28_29I"),
        "expected exported symbol fvm_aot_Codegen_constantReturn_28_29I in object symbols {symbols:?}"
    );
    Ok(())
}

#[test]
fn exported_symbol_distinguishes_overloads_by_descriptor() {
    use crate::fvm_aot::codegen::cranelift::exported_symbol;

    // Same class and method name, different parameter types: the old
    // name-only mangling collapsed these into one symbol.
    let take_int = exported_symbol("Overload.f", "(I)I");
    let take_two = exported_symbol("Overload.f", "(II)I");
    let take_string = exported_symbol("Overload.f", "(Ljava/lang/String;)I");

    assert_ne!(take_int, take_two);
    assert_ne!(take_int, take_string);
    assert_ne!(take_two, take_string);
}

#[test]
fn cranelift_rejects_unsupported_instruction() {
    // `emit_object` uses an empty object model, so allocating a class it has no
    // layout for is rejected loudly (rather than emitting a bad allocation).
    let function = int_function(
        "Codegen.unsupportedAllocation",
        vec![
            IrInstr::NewObject(ValueId::new(0), "CodegenObject".to_string()),
            IrInstr::Constant(ValueId::new(1), IrConst::Int(42)),
            IrInstr::Return(Some(ValueId::new(1))),
        ],
    );

    let message = emit_object(&function).unwrap_err().to_string();
    println!("{message}");

    assert!(message.contains("unsupported-codegen"));
    assert!(message.contains("Codegen.unsupportedAllocation()I"));
    assert!(message.contains("instruction=NewObject"));
    assert!(message.contains("no object layout for class CodegenObject"));
}

fn int_function(name: &str, instrs: Vec<IrInstr>) -> FunctionIr {
    FunctionIr {
        name: name.to_string(),
        descriptor: "()I".to_string(),
        params: Vec::new(),
        return_type: IrType::Int,
        blocks: vec![BasicBlockIr {
            id: BasicBlockId::new(0),
            params: Vec::new(),
            instrs,
        }],
    }
}
