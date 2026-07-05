use super::{
    BasicBlockId, BasicBlockIr, FunctionIr, IrArithmeticOp, IrCompareOp, IrConst, IrInstr, IrType,
    IrUnaryOp, TrapReason, ValueId,
};
use std::fmt;

impl fmt::Display for FunctionIr {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(output, "fn {}", self.name)?;
        if !self.params.is_empty() {
            write!(output, "(")?;
            for (index, param) in self.params.iter().enumerate() {
                if index > 0 {
                    write!(output, ", ")?;
                }
                write!(output, "{}: {}", param.value, param.ty)?;
            }
            write!(output, ")")?;
        }
        writeln!(output, " -> {} {{", self.return_type)?;
        for block in &self.blocks {
            write!(output, "{} -> [", block.id)?;
            for (index, successor) in successors(block).iter().enumerate() {
                if index > 0 {
                    write!(output, ", ")?;
                }
                write!(output, "{successor}")?;
            }
            writeln!(output, "]:")?;
            for instr in &block.instrs {
                writeln!(output, "  {instr}")?;
            }
        }
        writeln!(output, "}}")
    }
}

impl fmt::Display for BasicBlockId {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(output, "bb{}", self.0)
    }
}

impl fmt::Display for ValueId {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(output, "v{}", self.0)
    }
}

impl fmt::Display for IrType {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Void => write!(output, "void"),
            Self::Int => write!(output, "int"),
            Self::Boolean => write!(output, "boolean"),
            Self::Char => write!(output, "char"),
            Self::Object(class) => write!(output, "ref {class}"),
            Self::Array(component) => write!(output, "array<{component}>"),
            Self::Unsupported(reason) => write!(output, "unsupported<{reason}>"),
        }
    }
}

impl fmt::Display for IrInstr {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Param(value, local, ty) => write!(output, "param local{local} = {value}: {ty}"),
            Self::Constant(value, constant) => write!(output, "{value} = const {constant}"),
            Self::Compare(value, op, lhs, Some(rhs)) => {
                write!(output, "{value} = {op} {lhs}, {rhs}")
            }
            Self::Compare(value, op, lhs, None) => write!(output, "{value} = {op} {lhs}"),
            Self::Arithmetic(value, op, lhs, rhs) => write!(output, "{value} = {op} {lhs}, {rhs}"),
            Self::Unary(value, op, input) => write!(output, "{value} = {op} {input}"),
            Self::Return(Some(value)) => write!(output, "return {value}"),
            Self::Return(None) => write!(output, "return"),
            Self::Branch(target) => write!(output, "branch {target}"),
            Self::CondBranch(condition, then_target, else_target) => {
                write!(
                    output,
                    "branch_if {condition}, {then_target}, {else_target}"
                )
            }
            Self::ExceptionEdge(exception, target) => {
                write!(output, "exception_edge {exception} -> {target}")
            }
            Self::ZeroCheck(value, reason) => {
                write!(output, "check_nonzero {value} else trap {reason}")
            }
            Self::Trap(reason) => write!(output, "trap {reason}"),
            Self::Call(..)
            | Self::RuntimeCall(..)
            | Self::FieldGet(..)
            | Self::FieldPut(..)
            | Self::ArrayLoad(..)
            | Self::ArrayStore(..)
            | Self::ArrayLength(..)
            | Self::NewObject(..)
            | Self::NewArray(..)
            | Self::NullCheck(..)
            | Self::BoundsCheck(..) => write!(output, "{self:?}"),
        }
    }
}

impl fmt::Display for IrConst {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(value) => write!(output, "int {value}"),
            Self::Boolean(value) => write!(output, "boolean {value}"),
            Self::Char(value) => write!(output, "char {value:?}"),
            Self::Null => write!(output, "null"),
            Self::String(bytes) => write!(output, "string {bytes:?}"),
        }
    }
}

impl fmt::Display for IrArithmeticOp {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Add => write!(output, "add"),
            Self::Sub => write!(output, "sub"),
            Self::Mul => write!(output, "mul"),
            Self::Div => write!(output, "div"),
            Self::Rem => write!(output, "rem"),
        }
    }
}

impl fmt::Display for IrCompareOp {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IntEq => write!(output, "cmp_int_eq"),
            Self::IntNe => write!(output, "cmp_int_ne"),
            Self::IntLt => write!(output, "cmp_int_lt"),
            Self::IntGe => write!(output, "cmp_int_ge"),
            Self::IntGt => write!(output, "cmp_int_gt"),
            Self::IntLe => write!(output, "cmp_int_le"),
            Self::RefEqPlaceholder => write!(output, "cmp_ref_eq_placeholder"),
            Self::RefNePlaceholder => write!(output, "cmp_ref_ne_placeholder"),
            Self::RefIsNullPlaceholder => write!(output, "cmp_ref_is_null_placeholder"),
            Self::RefIsNonNullPlaceholder => {
                write!(output, "cmp_ref_is_non_null_placeholder")
            }
        }
    }
}

impl fmt::Display for IrUnaryOp {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Neg => write!(output, "neg"),
        }
    }
}

impl fmt::Display for TrapReason {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NullReference => write!(output, "null_reference"),
            Self::Bounds => write!(output, "bounds"),
            Self::DivideByZero => write!(output, "divide_by_zero"),
            Self::Unsupported(reason) => write!(output, "unsupported<{reason}>"),
        }
    }
}

fn successors(block: &BasicBlockIr) -> Vec<BasicBlockId> {
    match block.instrs.last() {
        Some(IrInstr::Branch(target)) => vec![*target],
        Some(IrInstr::CondBranch(_, then_target, else_target)) => {
            vec![*then_target, *else_target]
        }
        Some(
            IrInstr::Param(..)
            | IrInstr::Constant(..)
            | IrInstr::Compare(..)
            | IrInstr::Arithmetic(..)
            | IrInstr::Unary(..)
            | IrInstr::Call(..)
            | IrInstr::RuntimeCall(..)
            | IrInstr::Return(..)
            | IrInstr::FieldGet(..)
            | IrInstr::FieldPut(..)
            | IrInstr::ArrayLoad(..)
            | IrInstr::ArrayStore(..)
            | IrInstr::ArrayLength(..)
            | IrInstr::NewObject(..)
            | IrInstr::NewArray(..)
            | IrInstr::ZeroCheck(..)
            | IrInstr::NullCheck(..)
            | IrInstr::BoundsCheck(..)
            | IrInstr::ExceptionEdge(..)
            | IrInstr::Trap(..),
        )
        | None => Vec::new(),
    }
}
