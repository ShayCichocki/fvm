use crate::fvm_aot::ir::IrInstr;

pub(super) fn instruction_category(instr: &IrInstr) -> &'static str {
    match instr {
        IrInstr::Param(..) => "Param",
        IrInstr::Constant(..) => "Constant",
        IrInstr::Compare(..) => "Compare",
        IrInstr::Arithmetic(..) => "Arithmetic",
        IrInstr::Unary(..) => "Unary",
        IrInstr::Branch(..) => "Branch",
        IrInstr::CondBranch(..) => "CondBranch",
        IrInstr::Switch(..) => "Switch",
        IrInstr::Call(..) => "Call",
        IrInstr::RuntimeCall(..) => "RuntimeCall",
        IrInstr::Return(..) => "Return",
        IrInstr::FieldGet(..) => "FieldGet",
        IrInstr::FieldPut(..) => "FieldPut",
        IrInstr::ArrayLoad(..) => "ArrayLoad",
        IrInstr::ArrayStore(..) => "ArrayStore",
        IrInstr::ArrayLength(..) => "ArrayLength",
        IrInstr::NewObject(..) => "NewObject",
        IrInstr::NewArray(..) => "NewArray",
        IrInstr::ZeroCheck(..) => "ZeroCheck",
        IrInstr::NullCheck(..) => "NullCheck",
        IrInstr::BoundsCheck(..) => "BoundsCheck",
        IrInstr::ExceptionEdge(..) => "ExceptionEdge",
        IrInstr::Trap(..) => "Trap",
    }
}
