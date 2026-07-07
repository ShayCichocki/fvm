use super::ir::{
    BasicBlockId, BasicBlockIr, BranchEdge, FunctionIr, IrCompareOp, IrConst, IrInstr, IrType,
    MethodRef, RuntimeHelper, TrapReason, ValueId,
};
mod types;
mod values;

use anyhow::{Result, bail};
use std::collections::HashSet;
use types::{
    constant_type, descriptor_return_type, return_compatible, runtime_return_type,
    verify_descriptor_model_return, verify_supported_trap, verify_supported_type,
};
use values::ValueScope;

pub(super) fn verify_function(function: &FunctionIr) -> Result<()> {
    Verifier::new(function)?.verify()
}

struct Verifier<'a> {
    function: &'a FunctionIr,
    block_ids: HashSet<BasicBlockId>,
    scope: ValueScope,
}

impl<'a> Verifier<'a> {
    fn new(function: &'a FunctionIr) -> Result<Self> {
        let mut block_ids = HashSet::new();
        for block in &function.blocks {
            if !block_ids.insert(block.id) {
                bail!(
                    "IR function `{}` declares duplicate block {}",
                    label(function),
                    block.id
                );
            }
        }
        Ok(Self {
            function,
            block_ids,
            scope: ValueScope::new(label(function)),
        })
    }

    fn verify(mut self) -> Result<()> {
        self.seed_params()?;
        self.verify_type("return type", &self.function.return_type)?;
        verify_descriptor_model_return(
            &self.label(),
            &self.function.descriptor,
            &self.function.return_type,
        )?;
        for block in &self.function.blocks {
            self.verify_block(block)?;
        }
        Ok(())
    }

    fn seed_params(&mut self) -> Result<()> {
        for param in &self.function.params {
            self.verify_type("parameter type", &param.ty)?;
            self.scope.define_global(param.value, param.ty.clone())?;
        }
        Ok(())
    }

    /// Verify a single block: its parameters come into scope first (the phi
    /// equivalent), then instructions run in order. Control flow must reach
    /// exactly one terminator, as the block's last instruction — no straight-line
    /// code may follow a terminator, and every block must end in one.
    fn verify_block(&mut self, block: &BasicBlockIr) -> Result<()> {
        self.scope.enter_block();
        for param in &block.params {
            self.verify_type("block parameter type", &param.ty)?;
            self.scope.define(param.value, param.ty.clone(), false)?;
        }

        let mut terminated = false;
        for instr in &block.instrs {
            if terminated {
                bail!(
                    "IR function `{}` has an instruction after the terminator of {}",
                    self.label(),
                    block.id
                );
            }
            self.verify_instr(block.id, instr)?;
            terminated = is_terminator(instr);
        }
        if !terminated {
            bail!(
                "IR function `{}` block {} does not end in a terminator",
                self.label(),
                block.id
            );
        }
        Ok(())
    }

    fn verify_instr(&mut self, block: BasicBlockId, instr: &IrInstr) -> Result<()> {
        match instr {
            IrInstr::Param(value, _local, ty) => {
                self.verify_type("parameter type", ty)?;
                self.define_value(*value, ty.clone(), true)
            }
            IrInstr::Constant(value, constant) => {
                self.define_value(*value, constant_type(constant), false)
            }
            IrInstr::Compare(value, op, lhs, rhs) => {
                self.verify_compare(block, *value, *op, *lhs, *rhs)
            }
            IrInstr::Arithmetic(value, _op, lhs, rhs) => {
                self.scope.require_int_like(block, *lhs)?;
                self.scope.require_int_like(block, *rhs)?;
                self.define_value(*value, IrType::Int, false)
            }
            IrInstr::Unary(value, _op, input) => {
                self.scope.require_int_like(block, *input)?;
                self.define_value(*value, IrType::Int, false)
            }
            IrInstr::Branch(edge) => self.verify_edge(block, edge),
            IrInstr::CondBranch(condition, then_target, else_target) => {
                self.scope.use_value(block, *condition)?;
                self.verify_edge(block, then_target)?;
                self.verify_edge(block, else_target)
            }
            IrInstr::Switch(key, cases, default) => {
                self.scope.require_int_like(block, *key)?;
                for (_match_value, edge) in cases {
                    self.verify_edge(block, edge)?;
                }
                self.verify_edge(block, default)
            }
            IrInstr::Call(value, method, args) => self.verify_call(block, *value, method, args),
            IrInstr::RuntimeCall(value, helper, args) => {
                for arg in args {
                    self.scope.use_value(block, *arg)?;
                }
                match (value, runtime_return_type(helper)) {
                    (Some(value), Some(ty)) => self.define_value(*value, ty, false),
                    (None, None) => Ok(()),
                    (Some(_), None) | (None, Some(_)) => bail!(
                        "IR function `{}` has runtime helper return mismatch in {block}",
                        self.label()
                    ),
                }
            }
            IrInstr::Return(value) => self.verify_return(block, *value),
            IrInstr::FieldGet(value, field, receiver) => {
                if let Some(receiver) = receiver {
                    self.scope.use_value(block, *receiver)?;
                }
                self.verify_type("field type", &field.ty)?;
                self.define_value(*value, field.ty.clone(), false)
            }
            IrInstr::FieldPut(field, receiver, value) => {
                if let Some(receiver) = receiver {
                    self.scope.use_value(block, *receiver)?;
                }
                self.verify_type("field type", &field.ty)?;
                self.scope.use_value(block, *value).map(|_| ())
            }
            IrInstr::ArrayLoad(value, array, index, element) => {
                self.scope.use_value(block, *array)?;
                self.scope.require_int_like(block, *index)?;
                self.verify_type("array element type", element)?;
                // Sub-word loads (`baload`/`caload`/`saload`) widen to `int` on
                // the operand stack; reference loads keep the element type.
                let loaded = if element.is_int_like() {
                    IrType::Int
                } else {
                    element.clone()
                };
                self.define_value(*value, loaded, false)
            }
            IrInstr::ArrayStore(array, index, value, element) => {
                self.scope.use_value(block, *array)?;
                self.scope.require_int_like(block, *index)?;
                self.verify_type("array element type", element)?;
                self.scope.use_value(block, *value).map(|_| ())
            }
            IrInstr::ArrayLength(value, array) => {
                self.scope.use_value(block, *array)?;
                self.define_value(*value, IrType::Int, false)
            }
            IrInstr::NewObject(value, class) => {
                self.define_value(*value, IrType::Object(class.clone()), false)
            }
            IrInstr::NewArray(value, element, length) => {
                self.verify_type("array element type", element)?;
                self.scope.require_int_like(block, *length)?;
                self.define_value(*value, IrType::Array(Box::new(element.clone())), false)
            }
            IrInstr::ZeroCheck(value, reason) => {
                self.verify_trap(reason)?;
                self.scope.require_int_like(block, *value).map(|_| ())
            }
            IrInstr::NullCheck(value, reason) => {
                self.verify_trap(reason)?;
                self.scope.use_value(block, *value).map(|_| ())
            }
            IrInstr::BoundsCheck(index, length, reason) => {
                self.verify_trap(reason)?;
                self.scope.require_int_like(block, *index)?;
                self.scope.require_int_like(block, *length).map(|_| ())
            }
            IrInstr::ExceptionEdge(exception, target) => {
                self.scope.use_value(block, *exception)?;
                self.verify_target(block, *target)
            }
            IrInstr::Trap(reason) => self.verify_trap(reason),
        }
    }

    fn verify_call(
        &mut self,
        block: BasicBlockId,
        value: Option<ValueId>,
        method: &MethodRef,
        args: &[ValueId],
    ) -> Result<()> {
        for arg in args {
            self.scope.use_value(block, *arg)?;
        }
        let return_type = descriptor_return_type(&method.descriptor)?;
        self.verify_type("call return type", &return_type)?;
        match (value, return_type) {
            (Some(_), IrType::Void) => bail!(
                "IR function `{}` has call return mismatch for {}.{}{} in {block}",
                self.label(),
                method.class,
                method.name,
                method.descriptor
            ),
            (None, IrType::Void) => Ok(()),
            (None, _) => bail!(
                "IR function `{}` has call return mismatch for {}.{}{} in {block}",
                self.label(),
                method.class,
                method.name,
                method.descriptor
            ),
            (Some(value), ty) => self.define_value(value, ty, false),
        }
    }

    /// Type-check a `Compare`. Integer comparisons take two int-compatible
    /// operands; reference equality takes two references; the null tests take a
    /// single reference. Operand arity is enforced so a null test never carries a
    /// second operand and a binary comparison never drops one.
    fn verify_compare(
        &mut self,
        block: BasicBlockId,
        value: ValueId,
        op: IrCompareOp,
        lhs: ValueId,
        rhs: Option<ValueId>,
    ) -> Result<()> {
        match op {
            IrCompareOp::IntEq
            | IrCompareOp::IntNe
            | IrCompareOp::IntLt
            | IrCompareOp::IntGe
            | IrCompareOp::IntGt
            | IrCompareOp::IntLe => {
                let rhs = self.require_binary_operand(block, op, rhs)?;
                self.scope.require_int_like(block, lhs)?;
                self.scope.require_int_like(block, rhs)?;
            }
            IrCompareOp::RefEqPlaceholder | IrCompareOp::RefNePlaceholder => {
                let rhs = self.require_binary_operand(block, op, rhs)?;
                self.scope.require_reference(block, lhs)?;
                self.scope.require_reference(block, rhs)?;
            }
            IrCompareOp::RefIsNullPlaceholder | IrCompareOp::RefIsNonNullPlaceholder => {
                if rhs.is_some() {
                    bail!(
                        "IR function `{}` compare {op} in {block} takes a single operand but has two",
                        self.label()
                    );
                }
                self.scope.require_reference(block, lhs)?;
            }
        }
        self.define_value(value, IrType::Boolean, false)
    }

    fn require_binary_operand(
        &self,
        block: BasicBlockId,
        op: IrCompareOp,
        rhs: Option<ValueId>,
    ) -> Result<ValueId> {
        rhs.ok_or_else(|| {
            anyhow::anyhow!(
                "IR function `{}` compare {op} in {block} requires two operands but has one",
                self.label()
            )
        })
    }

    fn verify_return(&self, block: BasicBlockId, value: Option<ValueId>) -> Result<()> {
        match (&self.function.return_type, value) {
            (IrType::Void, None) => Ok(()),
            (IrType::Void, Some(value)) => bail!(
                "IR function `{}` return type mismatch in {block}: expected void, returned {}",
                self.label(),
                value
            ),
            (expected, None) => bail!(
                "IR function `{}` return type mismatch in {block}: expected {expected}, returned void",
                self.label()
            ),
            (expected, Some(value)) => {
                let actual = self.scope.use_value(block, value)?;
                if return_compatible(expected, &actual) {
                    return Ok(());
                }
                bail!(
                    "IR function `{}` return type mismatch in {block}: expected {expected}, returned {actual}",
                    self.label()
                )
            }
        }
    }

    fn verify_target(&self, source: BasicBlockId, target: BasicBlockId) -> Result<()> {
        if self.block_ids.contains(&target) {
            return Ok(());
        }
        bail!(
            "IR function `{}` branches from {source} to missing target {target}",
            self.label()
        )
    }

    /// Verify a control-flow edge: the target block exists, every argument is a
    /// defined value, and the argument count and types line up positionally
    /// with the target block's parameters.
    fn verify_edge(&self, source: BasicBlockId, edge: &BranchEdge) -> Result<()> {
        self.verify_target(source, edge.block)?;
        let mut arg_types = Vec::with_capacity(edge.args.len());
        for arg in &edge.args {
            arg_types.push(self.scope.use_value(source, *arg)?);
        }
        let target = self
            .function
            .blocks
            .iter()
            .find(|block| block.id == edge.block)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "IR function `{}` branches from {source} to missing target {}",
                    self.label(),
                    edge.block
                )
            })?;
        if target.params.len() != edge.args.len() {
            bail!(
                "IR function `{}` edge from {source} to {} passes {} argument(s) but the block declares {} parameter(s)",
                self.label(),
                edge.block,
                edge.args.len(),
                target.params.len()
            );
        }
        for (param, actual) in target.params.iter().zip(&arg_types) {
            if !return_compatible(&param.ty, actual) {
                bail!(
                    "IR function `{}` edge from {source} to {} passes {actual} for parameter {} of type {}",
                    self.label(),
                    edge.block,
                    param.value,
                    param.ty
                );
            }
        }
        Ok(())
    }

    fn define_value(&mut self, value: ValueId, ty: IrType, allow_existing: bool) -> Result<()> {
        self.verify_type("value type", &ty)?;
        self.scope.define(value, ty, allow_existing)
    }

    fn verify_type(&self, role: &str, ty: &IrType) -> Result<()> {
        verify_supported_type(&self.label(), role, ty)
    }

    fn verify_trap(&self, reason: &TrapReason) -> Result<()> {
        verify_supported_trap(&self.label(), reason)
    }

    fn label(&self) -> String {
        label(self.function)
    }
}

fn label(function: &FunctionIr) -> String {
    format!("{}{}", function.name, function.descriptor)
}

/// Whether an instruction ends a block's control flow. Branches transfer to a
/// successor, `Return` leaves the function, and `Trap` aborts; each is the last
/// instruction its block may contain.
fn is_terminator(instr: &IrInstr) -> bool {
    matches!(
        instr,
        IrInstr::Branch(_)
            | IrInstr::CondBranch(..)
            | IrInstr::Switch(..)
            | IrInstr::Return(_)
            | IrInstr::Trap(_)
    )
}
