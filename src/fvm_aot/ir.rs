#![allow(dead_code)]

use anyhow::{Result, bail};
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FunctionIr {
    pub(super) name: String,
    pub(super) params: Vec<IrParam>,
    pub(super) return_type: IrType,
    pub(super) blocks: Vec<BasicBlockIr>,
}

impl FunctionIr {
    pub(super) fn render_text(&self) -> String {
        self.to_string()
    }

    pub(super) fn verify(&self) -> Result<()> {
        let block_ids = self.blocks.iter().map(|block| block.id).collect::<Vec<_>>();
        for block in &self.blocks {
            for instr in &block.instrs {
                match instr {
                    IrInstr::Branch(target) => self.verify_target(block.id, *target, &block_ids)?,
                    IrInstr::CondBranch(_, then_target, else_target) => {
                        self.verify_target(block.id, *then_target, &block_ids)?;
                        self.verify_target(block.id, *else_target, &block_ids)?;
                    }
                    IrInstr::ExceptionEdge(_, target) => {
                        self.verify_target(block.id, *target, &block_ids)?;
                    }
                    IrInstr::Param(..)
                    | IrInstr::Constant(..)
                    | IrInstr::Arithmetic(..)
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
                    | IrInstr::NullCheck(..)
                    | IrInstr::BoundsCheck(..)
                    | IrInstr::Trap(..) => {}
                }
            }
        }
        Ok(())
    }

    fn verify_target(
        &self,
        source: BasicBlockId,
        target: BasicBlockId,
        block_ids: &[BasicBlockId],
    ) -> Result<()> {
        if block_ids.contains(&target) {
            return Ok(());
        }
        bail!(
            "IR function `{}` branches from {source} to missing target {target}",
            self.name
        );
    }
}

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
            writeln!(output, "{}:", block.id)?;
            for instr in &block.instrs {
                writeln!(output, "  {instr}")?;
            }
        }
        writeln!(output, "}}")
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
    pub(super) instrs: Vec<IrInstr>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) struct BasicBlockId(u32);

impl BasicBlockId {
    pub(super) const fn new(raw: u32) -> Self {
        Self(raw)
    }
}

impl fmt::Display for BasicBlockId {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(output, "bb{}", self.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) struct ValueId(u32);

impl ValueId {
    pub(super) const fn new(raw: u32) -> Self {
        Self(raw)
    }
}

impl fmt::Display for ValueId {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(output, "v{}", self.0)
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum IrInstr {
    Param(ValueId, u16, IrType),
    Constant(ValueId, IrConst),
    Arithmetic(ValueId, IrArithmeticOp, ValueId, ValueId),
    Branch(BasicBlockId),
    CondBranch(ValueId, BasicBlockId, BasicBlockId),
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
    NullCheck(ValueId, TrapReason),
    BoundsCheck(ValueId, ValueId, TrapReason),
    ExceptionEdge(ValueId, BasicBlockId),
    Trap(TrapReason),
}

impl fmt::Display for IrInstr {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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
            Self::Param(..)
            | Self::Constant(..)
            | Self::Arithmetic(..)
            | Self::Call(..)
            | Self::RuntimeCall(..)
            | Self::FieldGet(..)
            | Self::FieldPut(..)
            | Self::ArrayLoad(..)
            | Self::ArrayStore(..)
            | Self::ArrayLength(..)
            | Self::NewObject(..)
            | Self::NewArray(..)
            | Self::NullCheck(..)
            | Self::BoundsCheck(..)
            | Self::Trap(..) => write!(output, "{self:?}"),
        }
    }
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
    HttpRespond,
    StringConcat,
    ArrayClone,
    ObjectHashCode,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum TrapReason {
    NullReference,
    Bounds,
    Unsupported(String),
}
