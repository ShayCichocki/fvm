#![allow(dead_code)]

// allow: SIZE_OK - single runtime compiler IR model; verifier lives in ir_verify.rs.

use anyhow::Result;

mod display;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FunctionIr {
    pub(super) name: String,
    pub(super) descriptor: String,
    pub(super) params: Vec<IrParam>,
    pub(super) return_type: IrType,
    pub(super) blocks: Vec<BasicBlockIr>,
}

impl FunctionIr {
    pub(super) fn render_text(&self) -> String {
        self.to_string()
    }

    pub(super) fn verify(&self) -> Result<()> {
        super::ir_verify::verify_function(self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct IrParam {
    pub(super) value: ValueId,
    pub(super) ty: IrType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct BasicBlockIr {
    pub(super) id: BasicBlockId,
    pub(super) params: Vec<IrParam>,
    pub(super) instrs: Vec<IrInstr>,
}

/// A control-flow edge that passes the predecessor's live frame values as
/// arguments to the target block's parameters (the phi equivalent). `args`
/// is positional and must match the target block's `params` in length, order,
/// and type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct BranchEdge {
    pub(super) block: BasicBlockId,
    pub(super) args: Vec<ValueId>,
}

impl BranchEdge {
    pub(super) fn new(block: BasicBlockId, args: Vec<ValueId>) -> Self {
        Self { block, args }
    }

    pub(super) fn to(block: BasicBlockId) -> Self {
        Self {
            block,
            args: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) struct BasicBlockId(u32);

impl BasicBlockId {
    pub(super) const fn new(raw: u32) -> Self {
        Self(raw)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) struct ValueId(u32);

impl ValueId {
    pub(super) const fn new(raw: u32) -> Self {
        Self(raw)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum IrType {
    Void,
    Int,
    Boolean,
    Char,
    Object(String),
    Array(Box<IrType>),
    Unsupported(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum IrInstr {
    Param(ValueId, u16, IrType),
    Constant(ValueId, IrConst),
    Compare(ValueId, IrCompareOp, ValueId, Option<ValueId>),
    Arithmetic(ValueId, IrArithmeticOp, ValueId, ValueId),
    Unary(ValueId, IrUnaryOp, ValueId),
    Branch(BranchEdge),
    CondBranch(ValueId, BranchEdge, BranchEdge),
    /// Multi-way branch on an int key: each `(match value, edge)` pair jumps to
    /// its edge when the key equals the match; otherwise the default edge is
    /// taken. Models `tableswitch`/`lookupswitch`.
    Switch(ValueId, Vec<(i32, BranchEdge)>, BranchEdge),
    Call(Option<ValueId>, MethodRef, Vec<ValueId>),
    RuntimeCall(Option<ValueId>, RuntimeHelper, Vec<ValueId>),
    Return(Option<ValueId>),
    FieldGet(ValueId, FieldRef, Option<ValueId>),
    FieldPut(FieldRef, Option<ValueId>, ValueId),
    ArrayLoad(ValueId, ValueId, ValueId, IrType),
    ArrayStore(ValueId, ValueId, ValueId, IrType),
    ArrayLength(ValueId, ValueId),
    NewObject(ValueId, String),
    NewArray(ValueId, IrType, ValueId),
    ZeroCheck(ValueId, TrapReason),
    NullCheck(ValueId, TrapReason),
    BoundsCheck(ValueId, ValueId, TrapReason),
    ExceptionEdge(ValueId, BasicBlockId),
    Trap(TrapReason),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum IrConst {
    Int(i32),
    Boolean(bool),
    Char(char),
    Null,
    String(Vec<u8>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum IrArithmeticOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Shl,
    Shr,
    UShr,
    And,
    Or,
    Xor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum IrCompareOp {
    IntEq,
    IntNe,
    IntLt,
    IntGe,
    IntGt,
    IntLe,
    RefEqPlaceholder,
    RefNePlaceholder,
    RefIsNullPlaceholder,
    RefIsNonNullPlaceholder,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum IrUnaryOp {
    Neg,
    IntToByte,
    IntToShort,
    IntToChar,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MethodRef {
    pub(super) class: String,
    pub(super) name: String,
    pub(super) descriptor: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FieldRef {
    pub(super) class: String,
    pub(super) name: String,
    pub(super) ty: IrType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum RuntimeHelper {
    Println,
    PrintlnInt,
    PrintlnString,
    PrintlnEmpty,
    PrintInt,
    PrintString,
    StringBuilderNew,
    StringBuilderAppendInt,
    StringBuilderAppendString,
    StringBuilderFinish,
    HttpRespond,
    StringConcat,
    ArrayClone,
    ObjectHashCode,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum TrapReason {
    NullReference,
    Bounds,
    DivideByZero,
    Unsupported(String),
}
